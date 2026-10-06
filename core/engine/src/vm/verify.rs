//! Structural bytecode verifier (P5.1 validity model).
//!
//! [`verify`] checks a [`CodeBlock`] against every machine-checkable invariant
//! in `docs/bytecode-validity.md`: total instruction decode, jump-target and
//! handler well-formedness, register/constant/binding/IC/global referential
//! integrity (bounds and expected types), and block-level facts (tail
//! `Return`, fixed register prefix, async handlers).
//!
//! The verifier is total: it never panics, including on truncated streams,
//! hostile length prefixes, and deeply nested constants. It is also sound:
//! every block produced by `ByteCompiler::finish` must verify — any rejection
//! of compiler output is a checker bug until proven a compiler bug (the full
//! Test262 corpus is the acceptance set; see the walk test below).
//!
//! Wiring: a hook at the end of `ByteCompiler::finish` runs `verify` in
//! `#[cfg(test)]` and `feature = "verify-bytecode"` builds (never production
//! by default); fuzzers call it via `CodeBlock::verify`
//! (`tests/fuzz/src/semantic.rs::verify_hook`).

use std::collections::{HashMap, HashSet};

use super::{
    CallFrame, CodeBlock, CodeBlockFlags,
    code_block::Constant,
    opcode::{Address, IndexOperand, Instruction, Opcode, RegisterOperand},
};

/// Maximum `Constant::Function` nesting `verify` recurses into.
///
/// Source nesting is already bounded by the parser's stack guard; this cap is
/// defense in depth for fuzzer-assembled blocks. Genuine code nests far below
/// it (the Test262 maximum is in the tens).
const MAX_NESTING_DEPTH: u32 = 1024;

/// `DUMMY_ADDRESS` (`u32::MAX`) is the emit-then-patch placeholder; any
/// surviving occurrence in a finished block is a compiler bug.
const DUMMY_ADDRESS: u32 = u32::MAX;

/// Structural validity violation. Every variant carries the offending
/// program counter and opcode where the fault is instruction-local; header
/// faults carry the offending table and indices instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// Operands truncated: decode failed at `pc` for `opcode`.
    TruncatedOperand {
        /// Byte offset of the truncated instruction.
        pc: usize,
        /// Opcode whose operands run past the end of the stream.
        opcode: &'static str,
    },
    /// An unassigned `ReservedN` opcode appears in the stream.
    ReservedOpcode {
        /// Byte offset of the reserved instruction.
        pc: usize,
        /// Raw opcode discriminant (195..=255).
        discriminant: u8,
    },
    /// The block has no bytecode at all (`finish` always appends `Return`).
    EmptyBytecode,
    /// The last instruction is not the `Return` appended by `finish`.
    MissingReturn {
        /// Byte offset of the last instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
    },
    /// A jump target is `DUMMY_ADDRESS`, past the end, or mid-instruction.
    BadJumpTarget {
        /// Byte offset of the jumping instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
        /// The offending target offset.
        target: u32,
    },
    /// A `TemplateLookup`'s jump target does not immediately follow its
    /// `TemplateCreate` (or there is no preceding create at all).
    TemplateLookupWithoutCreate {
        /// Byte offset of the lookup.
        pc: usize,
    },
    /// A `TemplateLookup`/`TemplateCreate` pair disagrees on `site`.
    TemplateSiteMismatch {
        /// Byte offset of the lookup.
        pc: usize,
        /// The lookup's site key.
        lookup_site: u64,
        /// The following create's site key.
        create_site: u64,
    },
    /// A `TemplateCreate` carries an odd number of cooked/raw registers.
    OddTemplateValues {
        /// Byte offset of the create.
        pc: usize,
        /// The odd element count.
        len: usize,
    },
    /// A register index is `>= register_count`.
    RegisterOutOfBounds {
        /// Byte offset of the instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
        /// The offending register index.
        register: u32,
        /// The block's register count.
        register_count: u32,
    },
    /// A constant index is `>= constants.len()`.
    ConstantOutOfBounds {
        /// Byte offset of the instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
        /// The offending constant index.
        index: u32,
    },
    /// A constant has the wrong variant for its use site.
    ConstantTypeMismatch {
        /// Byte offset of the instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
        /// The constant index.
        index: u32,
        /// What the site requires (e.g. `"String"`, `"String|BigInt"`).
        expected: &'static str,
        /// What the table holds.
        found: &'static str,
    },
    /// A `binding_index` operand is `>= bindings.len()`.
    BindingOutOfBounds {
        /// Byte offset of the instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
        /// The offending binding index.
        index: u32,
    },
    /// An `ic_index` operand is `>= ic.len()`.
    IcOutOfBounds {
        /// Byte offset of the instruction.
        pc: usize,
        /// That instruction's opcode.
        opcode: &'static str,
        /// The offending IC index.
        index: u32,
    },
    /// An IC slot is referenced by more than one instruction (slots are 1:1).
    SharedIcSlot {
        /// The shared slot.
        index: u32,
        /// Byte offset of the first user.
        first_pc: usize,
        /// Byte offset of the second user.
        second_pc: usize,
    },
    /// An IC slot is referenced by no instruction (slots are dense).
    OrphanIcSlot {
        /// The unreferenced slot.
        index: u32,
    },
    /// A `GetNameGlobal` IC name disagrees with its binding's name.
    IcNameMismatch {
        /// Byte offset of the instruction.
        pc: usize,
        /// The binding index operand.
        binding_index: u32,
        /// The IC index operand.
        ic_index: u32,
    },
    /// An `ImportCall` phase is outside `0..=2` (evaluation/defer/source).
    BadImportPhase {
        /// Byte offset of the instruction.
        pc: usize,
        /// The offending phase value.
        phase: u32,
    },
    /// A handler range is inverted, dummy, past the end, or misaligned.
    BadHandlerRange {
        /// Index into `handlers`.
        index: usize,
        /// The range start offset.
        start: u32,
        /// The range end offset (exclusive).
        end: u32,
    },
    /// A global table entry indexes past `constants.len()`.
    GlobalOutOfBounds {
        /// Which table (`"global_lexs"`, `"global_vars"`, `"global_fns.name"`,
        /// `"global_fns.function"`).
        which: &'static str,
        /// The offending constant index.
        index: u32,
    },
    /// A global table entry references a wrongly-typed constant.
    GlobalTypeMismatch {
        /// Which table (see [`VerifyError::GlobalOutOfBounds`]).
        which: &'static str,
        /// The constant index.
        index: u32,
        /// What the table requires (`"String"` or `"Function"`).
        expected: &'static str,
        /// What the table holds.
        found: &'static str,
    },
    /// `register_count` is below the fixed frame prefix (§3 of the model).
    RegisterCountTooSmall {
        /// The block's register count.
        register_count: u32,
        /// The required minimum (1, 4 when async, 5 when async-generator).
        needed: u32,
    },
    /// `length` exceeds `parameter_length`.
    LengthExceedsParameters {
        /// The block's `length`.
        length: u32,
        /// The block's `parameter_length`.
        parameter_length: u32,
    },
    /// An `IS_ASYNC` block carries no exception handler.
    AsyncWithoutHandler,
    /// `Constant::Function` nesting exceeds `MAX_NESTING_DEPTH`.
    NestingTooDeep {
        /// The depth at which verification gave up.
        depth: u32,
    },
}

/// Check a block against the validity model (`docs/bytecode-validity.md`).
///
/// Total: returns `Err` on any violation, never panics.
pub(crate) fn verify(block: &CodeBlock) -> Result<(), VerifyError> {
    verify_inner(block, 0)
}

fn verify_inner(block: &CodeBlock, depth: u32) -> Result<(), VerifyError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(VerifyError::NestingTooDeep { depth });
    }

    let flags = block.flags.get();
    check_header(block, flags)?;
    check_globals(block)?;

    // Pass 1: total decode walk. Collects every (pc, opcode, instruction);
    // the pc set doubles as the jump-boundary set for pass 2.
    let mut decoded = Vec::new();
    let mut boundaries = HashSet::new();
    let mut pc = 0;
    let len = block.bytecode.bytes.len();
    while pc < len {
        let opcode_byte = block.bytecode.bytes[pc];
        let opcode = Opcode::decode(opcode_byte);
        if is_reserved(opcode) {
            return Err(VerifyError::ReservedOpcode {
                pc,
                discriminant: opcode_byte,
            });
        }
        let Some((instruction, next_pc)) = block.bytecode.try_next_instruction(pc) else {
            return Err(VerifyError::TruncatedOperand {
                pc,
                opcode: opcode.as_str(),
            });
        };
        boundaries.insert(pc);
        decoded.push((pc, opcode, instruction));
        // `try_next_instruction` always advances past the opcode byte.
        pc = next_pc;
    }
    // Tail rule: `finish` appends `Return` last.
    let Some((last_pc, last_opcode, _)) = decoded.last() else {
        return Err(VerifyError::EmptyBytecode);
    };
    let (last_pc, last_opcode) = (*last_pc, *last_opcode);
    if last_opcode != Opcode::Return {
        return Err(VerifyError::MissingReturn {
            pc: last_pc,
            opcode: last_opcode.as_str(),
        });
    }

    // Pass 2: per-instruction operand checks + jump-target checks. Needs the
    // full boundary set (jump targets may point forward).
    let mut checker = Checker {
        block,
        boundaries,
        ic_uses: HashMap::new(),
        lookups: Vec::new(),
    };
    for (pc, opcode, instruction) in &decoded {
        checker.check_instruction(*pc, *opcode, instruction)?;
    }

    check_handlers(block, &checker.boundaries)?;
    checker.check_ic_correspondence()?;
    checker.check_template_pairs(&decoded)?;

    // Recurse into nested functions.
    for constant in &block.constants {
        if let Constant::Function(nested) = constant {
            verify_inner(nested, depth + 1)?;
        }
    }

    Ok(())
}

/// Whether `opcode` is one of the 61 unassigned `ReservedN` discriminants.
///
/// Spelled as an explicit match (not a discriminant range) so adding a real
/// opcode anywhere in the enum cannot silently reclassify a reserved one.
fn is_reserved(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::Reserved1
            | Opcode::Reserved2
            | Opcode::Reserved3
            | Opcode::Reserved4
            | Opcode::Reserved5
            | Opcode::Reserved6
            | Opcode::Reserved7
            | Opcode::Reserved8
            | Opcode::Reserved9
            | Opcode::Reserved10
            | Opcode::Reserved11
            | Opcode::Reserved12
            | Opcode::Reserved13
            | Opcode::Reserved14
            | Opcode::Reserved15
            | Opcode::Reserved16
            | Opcode::Reserved17
            | Opcode::Reserved18
            | Opcode::Reserved19
            | Opcode::Reserved20
            | Opcode::Reserved21
            | Opcode::Reserved22
            | Opcode::Reserved23
            | Opcode::Reserved24
            | Opcode::Reserved25
            | Opcode::Reserved26
            | Opcode::Reserved27
            | Opcode::Reserved28
            | Opcode::Reserved29
            | Opcode::Reserved30
            | Opcode::Reserved31
            | Opcode::Reserved32
            | Opcode::Reserved33
            | Opcode::Reserved34
            | Opcode::Reserved35
            | Opcode::Reserved36
            | Opcode::Reserved37
            | Opcode::Reserved38
            | Opcode::Reserved39
            | Opcode::Reserved40
            | Opcode::Reserved41
            | Opcode::Reserved42
            | Opcode::Reserved43
            | Opcode::Reserved44
            | Opcode::Reserved45
            | Opcode::Reserved46
            | Opcode::Reserved47
            | Opcode::Reserved48
            | Opcode::Reserved49
            | Opcode::Reserved50
            | Opcode::Reserved51
            | Opcode::Reserved52
            | Opcode::Reserved53
            | Opcode::Reserved54
            | Opcode::Reserved55
            | Opcode::Reserved56
            | Opcode::Reserved57
            | Opcode::Reserved58
            | Opcode::Reserved59
            | Opcode::Reserved60
            | Opcode::Reserved61
    )
}

/// Block-header facts: fixed register prefix, `length` bound, async handler.
fn check_header(block: &CodeBlock, flags: CodeBlockFlags) -> Result<(), VerifyError> {
    // Fixed frame prefix (§3): r0 undefined always, r1..=r3 promise triple
    // when async, r4 async-generator object when async-generator.
    let mut needed = CallFrame::UNDEFINED_REGISTER_INDEX as u32 + 1;
    if flags.contains(CodeBlockFlags::IS_ASYNC) {
        needed = CallFrame::PROMISE_CAPABILITY_REJECT_REGISTER_INDEX as u32 + 1;
        if flags.contains(CodeBlockFlags::IS_GENERATOR) {
            needed = CallFrame::ASYNC_GENERATOR_OBJECT_REGISTER_INDEX as u32 + 1;
        }
    }
    if block.register_count < needed {
        return Err(VerifyError::RegisterCountTooSmall {
            register_count: block.register_count,
            needed,
        });
    }

    if block.length > block.parameter_length {
        return Err(VerifyError::LengthExceedsParameters {
            length: block.length,
            parameter_length: block.parameter_length,
        });
    }

    if flags.contains(CodeBlockFlags::IS_ASYNC) && block.handlers.is_empty() {
        return Err(VerifyError::AsyncWithoutHandler);
    }

    Ok(())
}

/// Global tables reference well-typed constants (§7).
fn check_globals(block: &CodeBlock) -> Result<(), VerifyError> {
    for index in block.global_lexs.iter().chain(block.global_vars.iter()) {
        check_global_constant(block, "global_lexs/vars", *index, "String", is_string)?;
    }
    for binding in &block.global_fns {
        check_global_constant(
            block,
            "global_fns.name",
            binding.name_index,
            "String",
            is_string,
        )?;
        check_global_constant(
            block,
            "global_fns.function",
            binding.function_index,
            "Function",
            is_function,
        )?;
    }
    Ok(())
}

fn check_global_constant(
    block: &CodeBlock,
    which: &'static str,
    index: u32,
    expected: &'static str,
    matches: fn(&Constant) -> bool,
) -> Result<(), VerifyError> {
    let Some(constant) = block.constants.get(index as usize) else {
        return Err(VerifyError::GlobalOutOfBounds { which, index });
    };
    if !matches(constant) {
        return Err(VerifyError::GlobalTypeMismatch {
            which,
            index,
            expected,
            found: constant_name(constant),
        });
    }
    Ok(())
}

/// Handler ranges are ordered, dummy-free, in-bounds, aligned (§2).
fn check_handlers(block: &CodeBlock, boundaries: &HashSet<usize>) -> Result<(), VerifyError> {
    let len = block.bytecode.bytes.len();
    for (index, handler) in block.handlers.iter().enumerate() {
        let start = handler.start.as_u32();
        let end = handler.end.as_u32();
        let aligned = |addr: u32| -> bool {
            addr != DUMMY_ADDRESS && (addr as usize) < len && boundaries.contains(&(addr as usize))
        };
        if start > end || !aligned(start) || !aligned(end) {
            return Err(VerifyError::BadHandlerRange { index, start, end });
        }
    }
    Ok(())
}

/// Short display name of a constant variant (error context only).
fn constant_name(constant: &Constant) -> &'static str {
    match constant {
        Constant::String(_) => "String",
        Constant::Function(_) => "Function",
        Constant::BigInt(_) => "BigInt",
        Constant::Scope(_) => "Scope",
    }
}

/// Per-instruction checking state.
struct Checker<'block> {
    block: &'block CodeBlock,
    /// Every instruction-start pc (the jump-boundary set).
    boundaries: HashSet<usize>,
    /// `ic_index -> first-use pc` (exact-once correspondence, §6).
    ic_uses: HashMap<u32, usize>,
    /// `(pc, site, target)` of every `TemplateLookup` (pairing, §2).
    lookups: Vec<(usize, u64, u32)>,
}

impl Checker<'_> {
    /// A register index must be below `register_count` (§3).
    fn check_register(
        &self,
        pc: usize,
        opcode: Opcode,
        register: RegisterOperand,
    ) -> Result<(), VerifyError> {
        let register = u32::from(register);
        if register >= self.block.register_count {
            return Err(VerifyError::RegisterOutOfBounds {
                pc,
                opcode: opcode.as_str(),
                register,
                register_count: self.block.register_count,
            });
        }
        Ok(())
    }

    /// A `u32`-typed register index (jump-table scrutinee, template/copy
    /// register lists) must be below `register_count` (§3).
    fn check_register_u32(
        &self,
        pc: usize,
        opcode: Opcode,
        register: u32,
    ) -> Result<(), VerifyError> {
        if register >= self.block.register_count {
            return Err(VerifyError::RegisterOutOfBounds {
                pc,
                opcode: opcode.as_str(),
                register,
                register_count: self.block.register_count,
            });
        }
        Ok(())
    }

    /// A jump target must be dummy-free, in-bounds, and aligned (§2).
    fn check_jump(&self, pc: usize, opcode: Opcode, target: Address) -> Result<(), VerifyError> {
        let target = target.as_u32();
        let aligned = target != DUMMY_ADDRESS
            && (target as usize) < self.block.bytecode.bytes.len()
            && self.boundaries.contains(&(target as usize));
        if !aligned {
            return Err(VerifyError::BadJumpTarget {
                pc,
                opcode: opcode.as_str(),
                target,
            });
        }
        Ok(())
    }

    /// A constant reference must be in bounds and of the expected type (§4).
    ///
    /// `expected`/`matches` encode one row of the §4 table.
    fn check_constant(
        &self,
        pc: usize,
        opcode: Opcode,
        index: IndexOperand,
        expected: &'static str,
        matches: fn(&Constant) -> bool,
    ) -> Result<(), VerifyError> {
        let index = u32::from(index);
        let Some(constant) = self.block.constants.get(index as usize) else {
            return Err(VerifyError::ConstantOutOfBounds {
                pc,
                opcode: opcode.as_str(),
                index,
            });
        };
        if !matches(constant) {
            return Err(VerifyError::ConstantTypeMismatch {
                pc,
                opcode: opcode.as_str(),
                index,
                expected,
                found: constant_name(constant),
            });
        }
        Ok(())
    }

    /// A `u32`-typed constant index (private-environment names) (§4).
    fn check_constant_u32(
        &self,
        pc: usize,
        opcode: Opcode,
        index: u32,
        expected: &'static str,
        matches: fn(&Constant) -> bool,
    ) -> Result<(), VerifyError> {
        let Some(constant) = self.block.constants.get(index as usize) else {
            return Err(VerifyError::ConstantOutOfBounds {
                pc,
                opcode: opcode.as_str(),
                index,
            });
        };
        if !matches(constant) {
            return Err(VerifyError::ConstantTypeMismatch {
                pc,
                opcode: opcode.as_str(),
                index,
                expected,
                found: constant_name(constant),
            });
        }
        Ok(())
    }

    /// A `binding_index` operand must be below `bindings.len()` (§5).
    fn check_binding(
        &self,
        pc: usize,
        opcode: Opcode,
        index: IndexOperand,
    ) -> Result<(), VerifyError> {
        let index = u32::from(index);
        if (index as usize) >= self.block.bindings.len() {
            return Err(VerifyError::BindingOutOfBounds {
                pc,
                opcode: opcode.as_str(),
                index,
            });
        }
        Ok(())
    }

    /// An `ic_index` operand must be below `ic.len()`; records the use for
    /// the exact-once correspondence check (§6).
    fn check_ic(
        &mut self,
        pc: usize,
        opcode: Opcode,
        index: IndexOperand,
    ) -> Result<(), VerifyError> {
        let index = u32::from(index);
        if (index as usize) >= self.block.ic.len() {
            return Err(VerifyError::IcOutOfBounds {
                pc,
                opcode: opcode.as_str(),
                index,
            });
        }
        if let Some(first_pc) = self.ic_uses.insert(index, pc) {
            return Err(VerifyError::SharedIcSlot {
                index,
                first_pc,
                second_pc: pc,
            });
        }
        Ok(())
    }

    /// Template pairing (§2): a lookup's jump target is the instruction
    /// immediately following its `TemplateCreate`, with an equal `site`.
    /// (Part-register stores sit between the lookup and the create, so the
    /// pairing is checked at the target, not at the fall-through.)
    fn check_template_pairs(
        &self,
        decoded: &[(usize, Opcode, Instruction)],
    ) -> Result<(), VerifyError> {
        for (pc, site, target) in &self.lookups {
            // Predecessor of the target: instructions are contiguous and
            // `decoded` is pc-ordered, so this abuts the target exactly.
            let pred = decoded.partition_point(|(pred_pc, _, _)| (*pred_pc as u32) < *target);
            let Some((
                _,
                _,
                Instruction::TemplateCreate {
                    site: create_site, ..
                },
            )) = pred.checked_sub(1).and_then(|index| decoded.get(index))
            else {
                return Err(VerifyError::TemplateLookupWithoutCreate { pc: *pc });
            };
            if *create_site != *site {
                return Err(VerifyError::TemplateSiteMismatch {
                    pc: *pc,
                    lookup_site: *site,
                    create_site: *create_site,
                });
            }
        }
        Ok(())
    }

    /// Every IC slot is referenced exactly once (shares already rejected) (§6).
    fn check_ic_correspondence(&self) -> Result<(), VerifyError> {
        for index in 0..self.block.ic.len() as u32 {
            if !self.ic_uses.contains_key(&index) {
                return Err(VerifyError::OrphanIcSlot { index });
            }
        }
        Ok(())
    }

    /// Per-instruction operand checks (§§2–6). Every real opcode has an
    /// explicit arm — no wildcard — so adding an opcode fails to compile
    /// until its operands are classified here.
    #[allow(clippy::too_many_lines)]
    // Identical bodies across arms are intentional: arms stay one-per-opcode
    // (grouped by model section) so any opcode's checks are found by search.
    // Merging across sections would trade auditability for brevity.
    #[allow(clippy::match_same_arms)]
    fn check_instruction(
        &mut self,
        pc: usize,
        opcode: Opcode,
        instruction: &Instruction,
    ) -> Result<(), VerifyError> {
        match instruction {
            // No operands, or operands total by construction. Reserved
            // variants are unreachable (rejected in pass 1) but listed
            // explicitly to keep the match exhaustive without a wildcard.
            Instruction::Reserved1
            | Instruction::Reserved2
            | Instruction::Reserved3
            | Instruction::Reserved4
            | Instruction::Reserved5
            | Instruction::Reserved6
            | Instruction::Reserved7
            | Instruction::Reserved8
            | Instruction::Reserved9
            | Instruction::Reserved10
            | Instruction::Reserved11
            | Instruction::Reserved12
            | Instruction::Reserved13
            | Instruction::Reserved14
            | Instruction::Reserved15
            | Instruction::Reserved16
            | Instruction::Reserved17
            | Instruction::Reserved18
            | Instruction::Reserved19
            | Instruction::Reserved20
            | Instruction::Reserved21
            | Instruction::Reserved22
            | Instruction::Reserved23
            | Instruction::Reserved24
            | Instruction::Reserved25
            | Instruction::Reserved26
            | Instruction::Reserved27
            | Instruction::Reserved28
            | Instruction::Reserved29
            | Instruction::Reserved30
            | Instruction::Reserved31
            | Instruction::Reserved32
            | Instruction::Reserved33
            | Instruction::Reserved34
            | Instruction::Reserved35
            | Instruction::Reserved36
            | Instruction::Reserved37
            | Instruction::Reserved38
            | Instruction::Reserved39
            | Instruction::Reserved40
            | Instruction::Reserved41
            | Instruction::Reserved42
            | Instruction::Reserved43
            | Instruction::Reserved44
            | Instruction::Reserved45
            | Instruction::Reserved46
            | Instruction::Reserved47
            | Instruction::Reserved48
            | Instruction::Reserved49
            | Instruction::Reserved50
            | Instruction::Reserved51
            | Instruction::Reserved52
            | Instruction::Reserved53
            | Instruction::Reserved54
            | Instruction::Reserved55
            | Instruction::Reserved56
            | Instruction::Reserved57
            | Instruction::Reserved58
            | Instruction::Reserved59
            | Instruction::Reserved60
            | Instruction::Reserved61
            | Instruction::Pop
            | Instruction::DeleteSuperThrow
            | Instruction::SuperCallSpread
            | Instruction::SuperCallDerived
            | Instruction::CallSpread
            | Instruction::NewSpread
            | Instruction::CheckReturn
            | Instruction::Return
            | Instruction::AsyncGeneratorClose
            | Instruction::Generator
            | Instruction::AsyncGenerator
            | Instruction::ReThrow
            | Instruction::PopEnvironment
            | Instruction::IncrementLoopIteration
            | Instruction::IteratorNext
            | Instruction::CreatePromiseCapability
            | Instruction::PopPrivateEnvironment => Ok(()),

            // --- Jumps (§2) ---
            Instruction::Jump { address } => self.check_jump(pc, opcode, *address),
            Instruction::JumpIfTrue { address, value }
            | Instruction::JumpIfFalse { address, value }
            | Instruction::JumpIfNotUndefined { address, value }
            | Instruction::JumpIfNullOrUndefined { address, value } => {
                self.check_jump(pc, opcode, *address)?;
                self.check_register(pc, opcode, *value)
            }
            Instruction::JumpIfNotLessThan { address, lhs, rhs }
            | Instruction::JumpIfNotLessThanOrEqual { address, lhs, rhs }
            | Instruction::JumpIfNotGreaterThan { address, lhs, rhs }
            | Instruction::JumpIfNotGreaterThanOrEqual { address, lhs, rhs }
            | Instruction::JumpIfNotEqual { address, lhs, rhs } => {
                self.check_jump(pc, opcode, *address)?;
                self.check_register(pc, opcode, *lhs)?;
                self.check_register(pc, opcode, *rhs)
            }
            Instruction::LogicalAnd { address, value }
            | Instruction::LogicalOr { address, value }
            | Instruction::Coalesce { address, value } => {
                self.check_jump(pc, opcode, *address)?;
                self.check_register(pc, opcode, *value)
            }
            Instruction::Case {
                address,
                value,
                condition,
            } => {
                self.check_jump(pc, opcode, *address)?;
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *condition)
            }
            Instruction::JumpTable { index, addresses } => {
                self.check_register_u32(pc, opcode, *index)?;
                for address in addresses {
                    self.check_jump(pc, opcode, *address)?;
                }
                Ok(())
            }
            Instruction::TemplateLookup { address, site, dst } => {
                self.check_jump(pc, opcode, *address)?;
                self.check_register(pc, opcode, *dst)?;
                self.lookups.push((pc, *site, address.as_u32()));
                Ok(())
            }

            // --- Constants (§4) ---
            Instruction::StoreLiteral { dst, index } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_constant(pc, opcode, *index, "String|BigInt", is_string_or_bigint)
            }
            Instruction::StoreRegexp {
                dst,
                pattern_index,
                flags_index,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_constant(pc, opcode, *pattern_index, "String", is_string)?;
                self.check_constant(pc, opcode, *flags_index, "String", is_string)
            }
            Instruction::GetFunction { dst, index } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_constant(pc, opcode, *index, "Function", is_function)
            }
            Instruction::CallEval { scope_index, .. } => {
                self.check_constant(pc, opcode, *scope_index, "Scope", is_scope)
            }
            Instruction::CallEvalSpread { scope_index }
            | Instruction::PushScope { scope_index } => {
                self.check_constant(pc, opcode, *scope_index, "Scope", is_scope)
            }
            Instruction::InPrivate { dst, index, rhs } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_constant(pc, opcode, *index, "String", is_string)?;
                self.check_register(pc, opcode, *rhs)
            }
            Instruction::GetMethod { object, name_index } => {
                self.check_register(pc, opcode, *object)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::ThrowMutateImmutable { index } => {
                self.check_constant(pc, opcode, *index, "String", is_string)
            }
            Instruction::ThrowNewTypeError { message }
            | Instruction::ThrowNewReferenceError { message } => {
                self.check_constant(pc, opcode, *message, "String", is_string)
            }
            Instruction::DefineOwnPropertyByName {
                object,
                value,
                name_index,
            }
            | Instruction::SetPropertyGetterByName {
                object,
                value,
                name_index,
            }
            | Instruction::SetPropertySetterByName {
                object,
                value,
                name_index,
            } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *value)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::DefineClassStaticMethodByName {
                value,
                object,
                name_index,
            }
            | Instruction::DefineClassMethodByName {
                value,
                object,
                name_index,
            }
            | Instruction::DefineClassStaticGetterByName {
                value,
                object,
                name_index,
            }
            | Instruction::DefineClassGetterByName {
                value,
                object,
                name_index,
            }
            | Instruction::DefineClassStaticSetterByName {
                value,
                object,
                name_index,
            }
            | Instruction::DefineClassSetterByName {
                value,
                object,
                name_index,
            } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *object)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::SetPrivateField {
                value,
                object,
                name_index,
            } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *object)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::DefinePrivateField {
                object,
                value,
                name_index,
            }
            | Instruction::SetPrivateMethod {
                object,
                value,
                name_index,
            }
            | Instruction::SetPrivateSetter {
                object,
                value,
                name_index,
            }
            | Instruction::SetPrivateGetter {
                object,
                value,
                name_index,
            } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *value)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::GetPrivateField {
                dst,
                object,
                name_index,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *object)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::PushClassFieldPrivate {
                object,
                value,
                name_index,
            }
            | Instruction::PushClassPrivateGetter {
                object,
                value,
                name_index,
            }
            | Instruction::PushClassPrivateSetter {
                object,
                value,
                name_index,
            } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *value)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::PushClassPrivateMethod {
                object,
                proto,
                value,
                name_index,
            } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *proto)?;
                self.check_register(pc, opcode, *value)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::DeletePropertyByName { object, name_index } => {
                self.check_register(pc, opcode, *object)?;
                self.check_constant(pc, opcode, *name_index, "String", is_string)
            }
            Instruction::PushPrivateEnvironment {
                class,
                name_indices,
            } => {
                self.check_register(pc, opcode, *class)?;
                for index in name_indices {
                    self.check_constant_u32(pc, opcode, *index, "String", is_string)?;
                }
                Ok(())
            }

            // --- Bindings (§5) ---
            Instruction::DefVar { binding_index }
            | Instruction::DefEvalVar { binding_index }
            | Instruction::GetLocator { binding_index } => {
                self.check_binding(pc, opcode, *binding_index)
            }
            Instruction::DefInitVar { src, binding_index }
            | Instruction::PutLexicalValue { src, binding_index } => {
                self.check_register(pc, opcode, *src)?;
                self.check_binding(pc, opcode, *binding_index)
            }
            Instruction::GetName { dst, binding_index }
            | Instruction::GetNameAndLocator { dst, binding_index }
            | Instruction::GetNameOrUndefined { dst, binding_index } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_binding(pc, opcode, *binding_index)
            }
            Instruction::SetName { src, binding_index } => {
                self.check_register(pc, opcode, *src)?;
                self.check_binding(pc, opcode, *binding_index)
            }
            Instruction::DeleteName { dst, binding_index } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_binding(pc, opcode, *binding_index)
            }

            // --- Inline caches (§6) ---
            Instruction::GetNameGlobal {
                dst,
                binding_index,
                ic_index,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_binding(pc, opcode, *binding_index)?;
                self.check_ic(pc, opcode, *ic_index)?;
                // Name correspondence: both come from the same binding.
                // Direct indexing is safe: bounds were just checked.
                let binding_name = self.block.bindings[u32::from(*binding_index) as usize].name();
                let ic_name = &self.block.ic[u32::from(*ic_index) as usize].name;
                if binding_name != ic_name {
                    return Err(VerifyError::IcNameMismatch {
                        pc,
                        binding_index: u32::from(*binding_index),
                        ic_index: u32::from(*ic_index),
                    });
                }
                Ok(())
            }
            Instruction::GetLengthProperty {
                dst,
                value,
                ic_index,
            }
            | Instruction::GetPropertyByName {
                dst,
                value,
                ic_index,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *value)?;
                self.check_ic(pc, opcode, *ic_index)
            }
            Instruction::GetPropertyByNameWithThis {
                dst,
                receiver,
                value,
                ic_index,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *receiver)?;
                self.check_register(pc, opcode, *value)?;
                self.check_ic(pc, opcode, *ic_index)
            }
            Instruction::SetPropertyByName {
                value,
                object,
                ic_index,
            } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *object)?;
                self.check_ic(pc, opcode, *ic_index)
            }
            Instruction::SetPropertyByNameWithThis {
                value,
                receiver,
                object,
                ic_index,
            } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *receiver)?;
                self.check_register(pc, opcode, *object)?;
                self.check_ic(pc, opcode, *ic_index)
            }

            // --- Mixed total operands + templates ---
            Instruction::GetArgument { dst, .. } => self.check_register(pc, opcode, *dst),
            Instruction::SetFunctionName { function, name, .. } => {
                self.check_register(pc, opcode, *function)?;
                self.check_register(pc, opcode, *name)
            }
            Instruction::PushClassField {
                object,
                name,
                value,
                ..
            } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *name)?;
                self.check_register(pc, opcode, *value)
            }
            Instruction::CreateIteratorResult { value, .. } => {
                self.check_register(pc, opcode, *value)
            }
            Instruction::ImportCall {
                specifier,
                options,
                phase,
            } => {
                self.check_register(pc, opcode, *specifier)?;
                self.check_register(pc, opcode, *options)?;
                let phase = u32::from(*phase);
                if phase > 2 {
                    return Err(VerifyError::BadImportPhase { pc, phase });
                }
                Ok(())
            }
            Instruction::SuperCall { .. } | Instruction::Call { .. } | Instruction::New { .. } => {
                Ok(())
            }
            Instruction::TemplateCreate { dst, values, .. } => {
                self.check_register(pc, opcode, *dst)?;
                if values.len() % 2 != 0 {
                    return Err(VerifyError::OddTemplateValues {
                        pc,
                        len: values.len(),
                    });
                }
                for value in values {
                    self.check_register_u32(pc, opcode, *value)?;
                }
                Ok(())
            }
            Instruction::CopyDataProperties {
                object,
                source,
                excluded_keys,
            } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *source)?;
                for key in excluded_keys {
                    self.check_register(pc, opcode, *key)?;
                }
                Ok(())
            }
            Instruction::ConcatToString { dst, values } => {
                self.check_register(pc, opcode, *dst)?;
                for value in values {
                    self.check_register(pc, opcode, *value)?;
                }
                Ok(())
            }

            // --- Register-only (§3) ---
            Instruction::StoreZero { dst }
            | Instruction::StoreOne { dst }
            | Instruction::StoreNan { dst }
            | Instruction::StorePositiveInfinity { dst }
            | Instruction::StoreNegativeInfinity { dst }
            | Instruction::StoreNull { dst }
            | Instruction::StoreTrue { dst }
            | Instruction::StoreFalse { dst }
            | Instruction::StoreUndefined { dst }
            | Instruction::StoreEmptyObject { dst }
            | Instruction::StoreNewArray { dst }
            | Instruction::This { dst }
            | Instruction::ThisForObjectEnvironmentName { dst }
            | Instruction::SetRegisterFromAccumulator { dst }
            | Instruction::PopIntoRegister { dst }
            | Instruction::IteratorDone { dst }
            | Instruction::IteratorValue { dst }
            | Instruction::IteratorResult { dst }
            | Instruction::IteratorStackEmpty { dst }
            | Instruction::RestParameterInit { dst }
            | Instruction::NewTarget { dst }
            | Instruction::ImportMeta { dst }
            | Instruction::CreateMappedArgumentsObject { dst }
            | Instruction::CreateUnmappedArgumentsObject { dst }
            | Instruction::Exception { dst } => self.check_register(pc, opcode, *dst),
            Instruction::StoreInt8 { dst, .. }
            | Instruction::StoreInt16 { dst, .. }
            | Instruction::StoreInt32 { dst, .. }
            | Instruction::StoreFloat { dst, .. }
            | Instruction::StoreDouble { dst, .. } => self.check_register(pc, opcode, *dst),
            Instruction::GetHomeObject { function } => self.check_register(pc, opcode, *function),
            Instruction::GetFunctionObject { function_object } => {
                self.check_register(pc, opcode, *function_object)
            }
            Instruction::GetPrototype { object } => self.check_register(pc, opcode, *object),
            Instruction::PushElisionToArray { array }
            | Instruction::PushIteratorToArray { array } => self.check_register(pc, opcode, *array),
            Instruction::BitNot { value }
            | Instruction::TypeOf { value }
            | Instruction::LogicalNot { value }
            | Instruction::Pos { value }
            | Instruction::Neg { value }
            | Instruction::IsObject { value }
            | Instruction::BindThisValue { value } => self.check_register(pc, opcode, *value),
            Instruction::SetAccumulator { src }
            | Instruction::PushFromRegister { src }
            | Instruction::PushObjectEnvironment { src }
            | Instruction::CreateForInIterator { src }
            | Instruction::GetIterator { src }
            | Instruction::GetAsyncIterator { src }
            | Instruction::GeneratorYield { src }
            | Instruction::AsyncGeneratorYield { src }
            | Instruction::Await { src }
            | Instruction::Throw { src }
            | Instruction::ValueNotNullOrUndefined { src }
            | Instruction::SetNameByLocator { src } => self.check_register(pc, opcode, *src),
            Instruction::IteratorUpdateResult { result } => {
                self.check_register(pc, opcode, *result)
            }
            Instruction::ToInt32 { dst, src }
            | Instruction::Move { dst, src }
            | Instruction::Inc { dst, src }
            | Instruction::Dec { dst, src } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *src)
            }
            Instruction::ToPropertyKey { src, dst } => {
                self.check_register(pc, opcode, *src)?;
                self.check_register(pc, opcode, *dst)
            }
            Instruction::SetHomeObject { function, home } => {
                self.check_register(pc, opcode, *function)?;
                self.check_register(pc, opcode, *home)
            }
            Instruction::SetPrototype { object, prototype } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *prototype)
            }
            Instruction::PushValueToArray { value, array } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *array)
            }
            Instruction::IteratorPop { iterator, next }
            | Instruction::IteratorPush { iterator, next } => {
                self.check_register(pc, opcode, *iterator)?;
                self.check_register(pc, opcode, *next)
            }
            Instruction::MaybeException {
                has_exception,
                exception,
            } => {
                self.check_register(pc, opcode, *has_exception)?;
                self.check_register(pc, opcode, *exception)
            }
            Instruction::StoreClassPrototype {
                dst,
                class,
                superclass,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *class)?;
                self.check_register(pc, opcode, *superclass)
            }
            Instruction::SetClassPrototype {
                dst,
                prototype,
                class,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *prototype)?;
                self.check_register(pc, opcode, *class)
            }
            Instruction::Add { dst, lhs, rhs }
            | Instruction::Sub { dst, lhs, rhs }
            | Instruction::Div { dst, lhs, rhs }
            | Instruction::Mul { dst, lhs, rhs }
            | Instruction::Mod { dst, lhs, rhs }
            | Instruction::Pow { dst, lhs, rhs }
            | Instruction::ShiftRight { dst, lhs, rhs }
            | Instruction::ShiftLeft { dst, lhs, rhs }
            | Instruction::UnsignedShiftRight { dst, lhs, rhs }
            | Instruction::BitOr { dst, lhs, rhs }
            | Instruction::BitAnd { dst, lhs, rhs }
            | Instruction::BitXor { dst, lhs, rhs }
            | Instruction::In { dst, lhs, rhs }
            | Instruction::Eq { dst, lhs, rhs }
            | Instruction::StrictEq { dst, lhs, rhs }
            | Instruction::NotEq { dst, lhs, rhs }
            | Instruction::StrictNotEq { dst, lhs, rhs }
            | Instruction::GreaterThan { dst, lhs, rhs }
            | Instruction::GreaterThanOrEq { dst, lhs, rhs }
            | Instruction::LessThan { dst, lhs, rhs }
            | Instruction::LessThanOrEq { dst, lhs, rhs }
            | Instruction::InstanceOf { dst, lhs, rhs } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *lhs)?;
                self.check_register(pc, opcode, *rhs)
            }
            Instruction::DefineOwnPropertyByValue { value, key, object }
            | Instruction::DefineClassStaticMethodByValue { value, key, object }
            | Instruction::DefineClassMethodByValue { value, key, object }
            | Instruction::SetPropertyGetterByValue { value, key, object }
            | Instruction::DefineClassStaticGetterByValue { value, key, object }
            | Instruction::DefineClassGetterByValue { value, key, object }
            | Instruction::SetPropertySetterByValue { value, key, object }
            | Instruction::DefineClassStaticSetterByValue { value, key, object }
            | Instruction::DefineClassSetterByValue { value, key, object } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *key)?;
                self.check_register(pc, opcode, *object)
            }
            Instruction::GetPropertyByValue {
                dst,
                key,
                receiver,
                object,
            }
            | Instruction::GetPropertyByValuePush {
                dst,
                key,
                receiver,
                object,
            } => {
                self.check_register(pc, opcode, *dst)?;
                self.check_register(pc, opcode, *key)?;
                self.check_register(pc, opcode, *receiver)?;
                self.check_register(pc, opcode, *object)
            }
            Instruction::SetPropertyByValue {
                value,
                key,
                receiver,
                object,
            } => {
                self.check_register(pc, opcode, *value)?;
                self.check_register(pc, opcode, *key)?;
                self.check_register(pc, opcode, *receiver)?;
                self.check_register(pc, opcode, *object)
            }
            Instruction::DeletePropertyByValue { object, key } => {
                self.check_register(pc, opcode, *object)?;
                self.check_register(pc, opcode, *key)
            }
        }
    }
}

/// `Constant::String` predicate (§4 table).
fn is_string(constant: &Constant) -> bool {
    matches!(constant, Constant::String(_))
}

/// `Constant::String | Constant::BigInt` predicate (§4 table).
fn is_string_or_bigint(constant: &Constant) -> bool {
    matches!(constant, Constant::String(_) | Constant::BigInt(_))
}

/// `Constant::Function` predicate (§4 table).
fn is_function(constant: &Constant) -> bool {
    matches!(constant, Constant::Function(_))
}

/// `Constant::Scope` predicate (§4 table).
fn is_scope(constant: &Constant) -> bool {
    matches!(constant, Constant::Scope(_))
}

#[cfg(test)]
mod tests {
    use super::{
        super::{
            InlineCache,
            code_block::{GlobalFunctionBinding, Handler},
            opcode::BytecodeEmitter,
        },
        *,
    };
    use crate::{Context, Script, Source, js_string};
    use boa_ast::scope::Scope;
    use boa_gc::Gc;
    use thin_vec::thin_vec;

    /// Assemble a minimal block around hand-emitted bytecode.
    fn assemble(emitter: BytecodeEmitter, register_count: u32) -> CodeBlock {
        let mut block = CodeBlock::new(js_string!("verify-test"), 0, false);
        block.bytecode = emitter.into_bytecode();
        block.register_count = register_count;
        block
    }

    /// Compile `source` (script goal) the way the engine does.
    fn compile(source: &str) -> Gc<CodeBlock> {
        let mut context = Context::default();
        let script = Script::parse(Source::from_bytes(source), None, &mut context)
            .expect("parse test source");
        script.codeblock(&mut context).expect("compile test source")
    }

    fn emitter_with_return() -> BytecodeEmitter {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_return();
        emitter
    }

    #[test]
    fn minimal_return_verifies() {
        let block = assemble(emitter_with_return(), 1);
        verify(&block).expect("bare Return must verify");
    }

    /// Real compiler output across tricky shapes must verify. (The `finish`
    /// hook already fires inside `codeblock()`; the explicit call pins the
    /// public path and the `Ok` value.)
    #[test]
    fn real_compiles_verify() {
        for source in [
            "1 + 2;",
            "if (x) { y(); } else { z(); }",
            "while (i < 10) { i++; }",
            "try { f(); } catch (e) { g(e); } finally { h(); }",
            "obj.prop; obj.prop = 1;",
            "new C(); new C(...args);",
            "a?.b?.c;",
            "switch (x) { case 1: a(); break; default: b(); }",
            "tag`a${x}b${y}c`;",
            "async function f() { await g(); }",
            "function* g() { yield 1; }",
            "async function* h() { yield await 1; }",
            "class C { #p = 1; m() { return this.#p; } }",
            "class D extends C { constructor() { super(); } }",
            "for (const k in o) { f(k); }",
            "for (const v of arr) { f(v); }",
            "for (let i = 0; i < 10; i++) { f(i); }",
            "do { x(); } while (c);",
            "import('m');",
            "function f(a, b = 2, ...r) { return arguments; }",
            "function f() { return arguments; }",
            "42n; 'str'; /ab+c/gi;",
            "x &&= y; x ||= y; x ??= y;",
            "with (o) { x; }",
            "eval('1'); (0, eval)('1');",
            "label: for (;;) { break label; }",
            "a ? b : c; `${x}${y}`.length;",
            "delete o.p; delete o?.p;",
            "x ** y ** z;",
            "[a, ...b]; ({ ...o });",
            "x in o; x instanceof C;",
            "typeof x; void 0; -x; +x; ~x; !x;",
            "x++; --y;",
            "`head${a}mid${b}tail`;",
        ] {
            verify(&compile(source)).expect("compiler output must verify");
        }
    }

    #[test]
    fn nested_functions_recurse() {
        let block = compile(
            "function f() { function g() { function h() { return 1; } return h(); } return g(); }",
        );
        verify(&block).expect("nested functions must verify");
    }

    #[test]
    fn empty_bytecode_rejected() {
        let block = assemble(BytecodeEmitter::new(), 1);
        assert_eq!(verify(&block), Err(VerifyError::EmptyBytecode));
    }

    #[test]
    fn missing_return_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_pop();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::MissingReturn {
                pc: 0,
                opcode: "Pop",
            })
        );
    }

    #[test]
    fn truncated_operand_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_jump(Address::new(0));
        let mut block = assemble(emitter, 1);
        let bytes = block.bytecode.bytes.to_vec();
        block.bytecode.bytes = bytes[..3].to_vec().into_boxed_slice();
        assert_eq!(
            verify(&block),
            Err(VerifyError::TruncatedOperand {
                pc: 0,
                opcode: "Jump"
            })
        );
    }

    #[test]
    fn reserved_opcode_rejected() {
        let mut block = assemble(BytecodeEmitter::new(), 1);
        block.bytecode.bytes = vec![255u8].into_boxed_slice();
        assert_eq!(
            verify(&block),
            Err(VerifyError::ReservedOpcode {
                pc: 0,
                discriminant: 255,
            })
        );
    }

    #[test]
    fn bad_jump_targets_rejected() {
        // dummy / past-end / misaligned (2 lands mid-operand of the jump).
        for target in [u32::MAX, 9999, 2] {
            let mut emitter = BytecodeEmitter::new();
            emitter.emit_jump(Address::new(target));
            emitter.emit_return();
            let block = assemble(emitter, 1);
            assert_eq!(
                verify(&block),
                Err(VerifyError::BadJumpTarget {
                    pc: 0,
                    opcode: "Jump",
                    target,
                }),
                "target {target}"
            );
        }
    }

    #[test]
    fn jump_table_operands_checked() {
        // OOB scrutinee register.
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_jump_table(9, thin_vec![Address::new(0)]);
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::RegisterOutOfBounds {
                pc: 0,
                opcode: "JumpTable",
                register: 9,
                register_count: 1,
            })
        );

        // OOB table entry.
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_jump_table(0, thin_vec![Address::new(9999)]);
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::BadJumpTarget {
                pc: 0,
                opcode: "JumpTable",
                target: 9999,
            })
        );
    }

    #[test]
    fn register_oob_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_store_zero(RegisterOperand::new(5));
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::RegisterOutOfBounds {
                pc: 0,
                opcode: "StoreZero",
                register: 5,
                register_count: 1,
            })
        );
    }

    #[test]
    fn constant_oob_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_store_literal(RegisterOperand::new(0), IndexOperand::new(3));
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::ConstantOutOfBounds {
                pc: 0,
                opcode: "StoreLiteral",
                index: 3,
            })
        );
    }

    #[test]
    fn constant_type_mismatch_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_store_literal(RegisterOperand::new(0), IndexOperand::new(0));
        emitter.emit_return();
        let mut block = assemble(emitter, 1);
        block.constants.push(Constant::Scope(Scope::new_global()));
        assert_eq!(
            verify(&block),
            Err(VerifyError::ConstantTypeMismatch {
                pc: 0,
                opcode: "StoreLiteral",
                index: 0,
                expected: "String|BigInt",
                found: "Scope",
            })
        );
    }

    #[test]
    fn binding_oob_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_def_var(IndexOperand::new(0));
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::BindingOutOfBounds {
                pc: 0,
                opcode: "DefVar",
                index: 0,
            })
        );
    }

    #[test]
    fn ic_oob_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_get_property_by_name(
            RegisterOperand::new(0),
            RegisterOperand::new(0),
            IndexOperand::new(0),
        );
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::IcOutOfBounds {
                pc: 0,
                opcode: "GetPropertyByName",
                index: 0,
            })
        );
    }

    #[test]
    fn shared_ic_slot_rejected() {
        let mut emitter = BytecodeEmitter::new();
        for _ in 0..2 {
            emitter.emit_get_property_by_name(
                RegisterOperand::new(0),
                RegisterOperand::new(0),
                IndexOperand::new(0),
            );
        }
        emitter.emit_return();
        let mut block = assemble(emitter, 1);
        block.ic = vec![InlineCache::new(js_string!("p"))].into_boxed_slice();
        // Each GetPropertyByName is 13 bytes: pcs 0 and 13 share slot 0.
        assert_eq!(
            verify(&block),
            Err(VerifyError::SharedIcSlot {
                index: 0,
                first_pc: 0,
                second_pc: 13,
            })
        );
    }

    #[test]
    fn orphan_ic_slot_rejected() {
        let mut block = assemble(emitter_with_return(), 1);
        block.ic = vec![InlineCache::new(js_string!("p"))].into_boxed_slice();
        assert_eq!(verify(&block), Err(VerifyError::OrphanIcSlot { index: 0 }));
    }

    #[test]
    fn ic_name_mismatch_rejected() {
        let compiled = compile("x;");
        let mut block = (*compiled).clone();
        block.ic[0].name = js_string!("other");
        let err = verify(&block).expect_err("renamed IC must fail");
        assert!(
            matches!(err, VerifyError::IcNameMismatch { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn bad_import_phase_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_import_call(
            RegisterOperand::new(0),
            RegisterOperand::new(0),
            IndexOperand::new(9),
        );
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::BadImportPhase { pc: 0, phase: 9 })
        );
    }

    #[test]
    fn odd_template_values_rejected() {
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_template_create(7, RegisterOperand::new(0), thin_vec![0u32]);
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::OddTemplateValues { pc: 0, len: 1 })
        );
    }

    #[test]
    fn template_lookup_requires_paired_create() {
        // Lookup whose fall-through is Return, not Create.
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_template_lookup(Address::new(17), 7, RegisterOperand::new(0));
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::TemplateLookupWithoutCreate { pc: 0 })
        );

        // Lookup + create disagree on site. True layout: lookup is 17 bytes
        // (1 + 4 + 8 + 4), a part-register store is 5 bytes, create at 22 is
        // 25 bytes (1 + 8 + 4 + 4 + 2*4), Return at 47; the lookup jumps to
        // 47 on cache hit.
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_template_lookup(Address::new(47), 1, RegisterOperand::new(0));
        emitter.emit_store_zero(RegisterOperand::new(0));
        emitter.emit_template_create(2, RegisterOperand::new(0), thin_vec![0u32, 0u32]);
        emitter.emit_return();
        let block = assemble(emitter, 1);
        assert_eq!(
            verify(&block),
            Err(VerifyError::TemplateSiteMismatch {
                pc: 0,
                lookup_site: 1,
                create_site: 2,
            })
        );

        // Matching pair verifies.
        let mut emitter = BytecodeEmitter::new();
        emitter.emit_template_lookup(Address::new(47), 5, RegisterOperand::new(0));
        emitter.emit_store_zero(RegisterOperand::new(0));
        emitter.emit_template_create(5, RegisterOperand::new(0), thin_vec![0u32, 0u32]);
        emitter.emit_return();
        let block = assemble(emitter, 1);
        verify(&block).expect("paired template ops must verify");
    }

    #[test]
    fn bad_handler_ranges_rejected() {
        // Dummy end.
        let mut block = assemble(emitter_with_return(), 1);
        block.handlers.push(Handler {
            start: Address::new(0),
            end: Address::new(u32::MAX),
            environment_count: 0,
        });
        assert!(
            matches!(
                verify(&block),
                Err(VerifyError::BadHandlerRange { index: 0, .. })
            ),
            "dummy handler end must fail"
        );

        // Inverted range.
        let mut block = assemble(emitter_with_return(), 1);
        block.handlers.push(Handler {
            start: Address::new(1),
            end: Address::new(0),
            environment_count: 0,
        });
        assert!(
            matches!(
                verify(&block),
                Err(VerifyError::BadHandlerRange { index: 0, .. })
            ),
            "inverted handler range must fail"
        );
    }

    #[test]
    fn register_count_prefix_enforced() {
        // Fresh block has count 0: below even the r0 minimum.
        let block = CodeBlock::new(js_string!("empty"), 0, false);
        assert_eq!(
            verify(&block),
            Err(VerifyError::RegisterCountTooSmall {
                register_count: 0,
                needed: 1,
            })
        );

        // Async needs the promise triple.
        let block = assemble(emitter_with_return(), 1);
        block.flags.set(CodeBlockFlags::IS_ASYNC);
        assert_eq!(
            verify(&block),
            Err(VerifyError::RegisterCountTooSmall {
                register_count: 1,
                needed: 4,
            })
        );

        // Async-generator needs r4 too.
        let block = assemble(emitter_with_return(), 4);
        block
            .flags
            .set(CodeBlockFlags::IS_ASYNC | CodeBlockFlags::IS_GENERATOR);
        assert_eq!(
            verify(&block),
            Err(VerifyError::RegisterCountTooSmall {
                register_count: 4,
                needed: 5,
            })
        );
    }

    #[test]
    fn async_without_handler_rejected() {
        let block = assemble(emitter_with_return(), 4);
        block.flags.set(CodeBlockFlags::IS_ASYNC);
        assert_eq!(verify(&block), Err(VerifyError::AsyncWithoutHandler));
    }

    #[test]
    fn length_exceeds_parameters_rejected() {
        let mut block = CodeBlock::new(js_string!("f"), 3, false);
        block.register_count = 1;
        assert_eq!(
            verify(&block),
            Err(VerifyError::LengthExceedsParameters {
                length: 3,
                parameter_length: 0,
            })
        );
    }

    #[test]
    fn global_tables_checked() {
        // Referenced string constant: ok.
        let mut block = assemble(emitter_with_return(), 1);
        block.constants.push(Constant::String(js_string!("g")));
        block.global_lexs = vec![0u32].into_boxed_slice();
        verify(&block).expect("well-formed globals must verify");

        // Dangling index.
        let mut block = assemble(emitter_with_return(), 1);
        block.global_lexs = vec![5u32].into_boxed_slice();
        assert_eq!(
            verify(&block),
            Err(VerifyError::GlobalOutOfBounds {
                which: "global_lexs/vars",
                index: 5,
            })
        );

        // Function slot holding a string.
        let mut block = assemble(emitter_with_return(), 1);
        block.constants.push(Constant::String(js_string!("g")));
        block.global_fns = vec![GlobalFunctionBinding {
            name_index: 0,
            function_index: 0,
        }]
        .into_boxed_slice();
        assert_eq!(
            verify(&block),
            Err(VerifyError::GlobalTypeMismatch {
                which: "global_fns.function",
                index: 0,
                expected: "Function",
                found: "String",
            })
        );
    }

    #[test]
    fn nesting_cap_enforced() {
        let mut inner = Gc::new(assemble(emitter_with_return(), 1));
        for _ in 0..1100 {
            let mut outer = assemble(emitter_with_return(), 1);
            outer.constants.push(Constant::Function(inner));
            inner = Gc::new(outer);
        }
        assert_eq!(
            verify(&inner),
            Err(VerifyError::NestingTooDeep { depth: 1025 })
        );
    }

    /// Recursively collect `.js` files (no new deps for a three-line walk).
    fn collect_js_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_js_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "js") {
                out.push(path);
            }
        }
    }

    /// Acceptance gate: every script-goal file in the pinned Test262 checkout
    /// that parses and compiles must verify. (Module-goal files need the
    /// loader-driven compile path; they are covered by the `finish` hook
    /// whenever the suite compiles them. A missing checkout skips silently —
    /// the hook still guards everything the suite compiles.)
    #[test]
    fn test262_corpus_acceptance() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test262/test");
        if !root.is_dir() {
            return;
        }
        let mut files = Vec::new();
        collect_js_files(&root, &mut files);
        assert!(
            files.len() > 40_000,
            "hollow test262 checkout? only {} files",
            files.len()
        );

        // (mode, file) pairs: like the tester, every file is attempted in
        // both strictness modes (the parser/compiler paths differ). Each
        // pair gets a FRESH context: `codeblock` instantiates the file's
        // globals into the realm scope, so reuse would fail every file after
        // the first redeclaration (measured: reuse collapses to ~1/3 pairs).
        let (mut compiled, mut skipped) = (0u32, 0u32);
        for path in &files {
            let Ok(source) = std::fs::read(path) else {
                skipped += 2;
                continue;
            };
            for strict in [false, true] {
                let mut context = Context::default();
                context.strict(strict);
                let Ok(script) = Script::parse(Source::from_bytes(&source), None, &mut context)
                else {
                    // Module-goal files, negative parse tests, mode-invalid.
                    skipped += 1;
                    continue;
                };
                let Ok(block) = script.codeblock(&mut context) else {
                    // Early errors raised at compile time.
                    skipped += 1;
                    continue;
                };
                if let Err(err) = verify(&block) {
                    panic!(
                        "verify rejected {} (strict={strict}): {err:?}",
                        path.display()
                    );
                }
                compiled += 1;
            }
        }
        // Calibrated 2026-10-03 (see failure output for the observed value):
        // the threshold sits well below it so checkout updates don't flap,
        // while a collapsed walk (harness bug, goal regression) fails loudly.
        assert!(
            compiled > 60_000,
            "too few compiles ({compiled}); script-goal coverage collapsed?"
        );
        assert_eq!(
            compiled + skipped,
            files.len() as u32 * 2,
            "every (mode, file) pair is either compiled or skipped"
        );
    }
}

/// Kani proofs for the bytecode validity checker (P6.4c).
///
/// What is proven: header admission matches its specified predicate over
/// all inputs (§3 frame prefix, length bound, async-handler rule).
///
/// Bounds: the kernel is loop-free over symbolic scalars (complete, no
/// unwinding). Two narrowings, both attempted first: (1) whole-`verify`
/// proofs — even the empty-bytecode harness OOM-kills `goto-instrument`
/// (all 195 opcode arms stay call-graph-reachable through the decode
/// dispatch; the 1-byte concrete `Return` block exhausts CBMC itself);
/// (2) the `check_jump`/`check_register` kernels — both need a `Checker`,
/// whose `std` `HashSet`/`HashMap` fields pull lazy-static `futex` init
/// (`RandomState::new`), an unsupported construct Kani fails on.
/// Per-instruction and jump/register behavior stay covered by tested
/// argument — the P5 differential battery that runs `verify` on every
/// successful compile plus the 27 unit tests in this file.
///
/// The block is assembled literally (mirroring the test `assemble` helper)
/// instead of via `CodeBlock::new`, whose thread-local id counter Kani
/// cannot compile; `verify` never reads `debug_id`.
/// Run with `cargo kani -p boa_engine --harness kani_verify_*`.
#[cfg(kani)]
mod kani_verify {
    use std::cell::Cell;

    use thin_vec::ThinVec;

    use crate::{
        builtins::function::ThisMode,
        js_string,
        spanned_source_text::SpannedSourceText,
        vm::{
            CodeBlock, CodeBlockFlags,
            code_block::Handler,
            opcode::{Address, Bytecode},
            source_info::{SourceInfo, SourceMap, SourcePath},
        },
    };

    /// Minimal block around the given bytecode (no constants/handlers/ICs).
    fn assemble(bytecode: Bytecode) -> CodeBlock {
        CodeBlock {
            bytecode,
            constants: ThinVec::default(),
            bindings: Box::default(),
            flags: Cell::new(CodeBlockFlags::empty()),
            length: 0,
            register_count: 1,
            this_mode: ThisMode::Global,
            mapped_arguments_binding_indices: ThinVec::new(),
            parameter_length: 0,
            handlers: ThinVec::default(),
            ic: Box::default(),
            source_info: SourceInfo::new(
                SourceMap::new(Box::default(), SourcePath::None),
                js_string!("kani"),
                SpannedSourceText::new_empty(),
            ),
            global_lexs: Box::default(),
            global_fns: Box::default(),
            global_vars: Box::default(),
            debug_id: 0,
            #[cfg(feature = "trace")]
            traced: Cell::new(false),
        }
    }

    /// Header admission matches the §3/length/async rules over all inputs.
    #[kani::proof]
    fn kani_verify_header_rules() {
        let is_async: bool = kani::any();
        let is_gen: bool = kani::any();
        let mut flags = CodeBlockFlags::empty();
        flags.set(CodeBlockFlags::IS_ASYNC, is_async);
        flags.set(CodeBlockFlags::IS_GENERATOR, is_gen);

        let register_count: u32 = kani::any();
        let length: u32 = kani::any();
        let parameter_length: u32 = kani::any();
        let with_handler: bool = kani::any();

        let mut block = assemble(Bytecode::default());
        block.register_count = register_count;
        block.length = length;
        block.parameter_length = parameter_length;
        block.flags.set(flags);
        if with_handler {
            block.handlers.push(Handler {
                start: Address::new(0),
                end: Address::new(0),
                environment_count: 0,
            });
        }

        // Restated oracle: r0 always (1 slot), promise triple when async
        // (reject slot 3 + 1), async-generator object when both (slot 4 + 1).
        let needed = if is_async {
            if is_gen { 5 } else { 4 }
        } else {
            1
        };
        let expect_ok =
            register_count >= needed && length <= parameter_length && (!is_async || with_handler);
        kani::assert(
            super::check_header(&block, flags).is_ok() == expect_ok,
            "header matches its rules",
        );
    }
}
