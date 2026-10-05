//! VM opcode/operand coverage instrumentation (P5.4, `vm-coverage` feature).
//!
//! Test-only measurement: a process-global histogram of executed VM cells,
//! plus the static opcode enumerator shared by every matrix consumer. There
//! is no production footprint — the feature is off by default, and every
//! hook call site is `#[cfg]`-gated.
//!
//! # Cell grammar
//!
//! Every executed instruction bumps its opcode's base cell (`Call`). Ops
//! whose operands genuinely vary bump one refinement cell as well:
//!
//! - Conditional jumps: `JumpIfTrue|taken` / `JumpIfTrue|nottaken` (one pair
//!   per conditional opcode). Unconditional `Jump` always takes and
//!   `JumpTable` is multi-target, so both emit the base cell only.
//! - Fixed-arity calls: `Call|argv-N` for `N in 0..=7`, `Call|argv-many`
//!   above that. Spread call ops carry no arity operand (the opcode name
//!   already says spread), so they emit the base cell only.
//! - `StoreLiteral`: `StoreLiteral|k-str` / `StoreLiteral|k-bigint` by
//!   constant variant. Every other constant-indexed op (`GetFunction`,
//!   `StoreRegexp`, `CallEval`, `PushScope`, `InPrivate`, `GetMethod`,
//!   `ThrowMutateImmutable`, `ThrowNewTypeError`, `ThrowNewReferenceError`)
//!   is model-fixed to one constant type (see `verify.rs` §4), so a shape
//!   cell would always equal the base count; they emit the base cell only.
//! - Errors: `op|err-T` where `T` is the surfacing native error kind
//!   (`TypeError`, `RangeError`, …), `opaque` for user-thrown values, or
//!   `engine` for uncatchable engine errors. Semantics is *surfacing*: the
//!   error passed through `Context::handle_error` while `op` was current,
//!   which also covers errors deferred via `pending_exception`.
//! - IC transitions (P7.1c): `ic|to-mono`, `ic|to-poly2/3/4`, `ic|to-mega`
//!   bumped from `InlineCache::set` as one cache walks
//!   empty→mono→poly→megamorphic. Steady states (hits, mega no-ops) bump
//!   nothing: transitions only.
//!
//! # Invariants
//!
//! - Zero dust: the dump contains touched cells only, never zero counts.
//! - JSON-safe alphabet: cells are engine-generated from ASCII alphanumerics
//!   plus `|`, `-`, `.` (enforced by `debug_assert` on every bump), so the
//!   dump serializes without an escaping pass.
//! - Measurement never panics: decode failures and out-of-range constant
//!   indices yield `k-oob` (or silence), never an unwrap. A poisoned
//!   histogram mutex is recovered, not propagated.
//! - Atomicity: hooks run before/after whole handlers only — never between
//!   `PushEnv` and its environment use — and classify errors via
//!   [`JsError::as_native`], which runs no user code.
//!

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use crate::vm::code_block::{CodeBlock, Constant};
use crate::vm::opcode::{Instruction, InstructionIterator, Opcode};
use crate::{Context, JsError, JsNativeErrorKind};

/// Process-global executed-cell histogram.
///
/// Lock contention is irrelevant for a test-only feature; a poisoned mutex
/// (a panicking test mid-bump) is recovered via [`histogram`], never
/// propagated.
static HISTOGRAM: LazyLock<Mutex<HashMap<String, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Lock the histogram, recovering from poisoning.
///
/// Measurement must never panic: a poisoned mutex means a previous holder
/// panicked, and its partial counts are still usable.
fn histogram() -> MutexGuard<'static, HashMap<String, u64>> {
    HISTOGRAM.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Bump `cell` by one.
fn bump(cell: &str) {
    debug_assert!(
        cell_is_json_safe(cell),
        "coverage cell outside the JSON-safe alphabet: {cell:?}"
    );
    *histogram().entry(cell.to_owned()).or_default() += 1;
}

/// The JSON-safe cell alphabet: ASCII alphanumerics plus `|`, `-`, `.`.
///
/// All cell fragments (opcode `NAME`s, `taken`/`nottaken`, `argv-N`/`many`,
/// `k-str`/`k-bigint`/`k-oob`, `err-*`) are engine-generated from this
/// alphabet; nothing user-controlled ever enters a cell name.
fn cell_is_json_safe(cell: &str) -> bool {
    !cell.is_empty()
        && cell
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'|' | b'-' | b'.'))
}

/// Clear all counts.
///
/// Call before each measured run: counts accumulate process-wide, so test
/// isolation and per-file attribution both require an explicit reset.
pub fn coverage_clear() {
    histogram().clear();
}

/// Dump nonzero cells as `(cell, count)` pairs sorted by cell name.
///
/// Zero dust by construction: untouched cells are absent, never zero.
#[must_use]
pub fn coverage_dump() -> Vec<(String, u64)> {
    let map = histogram();
    let mut rows: Vec<(String, u64)> = map
        .iter()
        .map(|(cell, count)| (cell.clone(), *count))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// Serialize the current dump as one JSON object `{"cell":count,...}`.
///
/// Single serialization implementation shared by the matrix tool and the
/// tester flag. No escaping pass: [`cell_is_json_safe`] holds for every
/// emitted cell.
#[must_use]
pub fn coverage_dump_json() -> String {
    let mut out = String::from("{");
    for (index, (cell, count)) in coverage_dump().iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(cell);
        out.push_str("\":");
        out.push_str(&count.to_string());
    }
    out.push('}');
    out
}

/// Statically enumerate the cells emitted in `block`, recursing into
/// nested function constants.
///
/// Single static-scan implementation shared by every matrix consumer: the
/// recursion follows `Constant::Function` exactly like
/// [`CodeBlock::verify`](crate::vm::code_block::CodeBlock::verify)'s
/// traversal. Each instruction contributes its base cell plus its
/// statically-known refinement ([`refinement_cell`]); branch direction and
/// error surfacing are dynamic-only. Callers must verify compiler-produced
/// blocks first (an invalid block is a compiler bug and must crash loud
/// there, not here): the iterator below assumes well-formed bytecode.
pub fn emitted_cells(block: &CodeBlock) -> Vec<String> {
    let mut cells = Vec::new();
    emitted_cells_into(block, &mut cells);
    cells
}

/// Worker for [`emitted_cells`].
fn emitted_cells_into(block: &CodeBlock, cells: &mut Vec<String>) {
    for (_, opcode, instruction) in InstructionIterator::new(&block.bytecode) {
        let name = opcode.as_str();
        cells.push(name.to_owned());
        if let Some(cell) = refinement_cell(name, &instruction, block) {
            cells.push(cell);
        }
    }
    for constant in &block.constants {
        if let Constant::Function(nested) = constant {
            emitted_cells_into(nested, cells);
        }
    }
}

/// The refinement cell for `instruction`, if its operands genuinely vary.
///
/// Single grammar shared by the dynamic pre-hook and the static scan:
/// `StoreLiteral` splits by constant variant, fixed-arity calls by arity
/// (capped at `argv-many`). Everything else is model-fixed (constant
/// use-sites with one legal type, spread calls) or dynamic-only (branch
/// direction, error surfacing) and yields no static cell.
fn refinement_cell(name: &str, instruction: &Instruction, block: &CodeBlock) -> Option<String> {
    match instruction {
        Instruction::StoreLiteral { index, .. } => {
            let shape = match block.constants.get(usize::from(*index)) {
                Some(Constant::String(_)) => "k-str",
                Some(Constant::BigInt(_)) => "k-bigint",
                Some(_) => "k-mistype",
                None => "k-oob",
            };
            Some(format!("{name}|{shape}"))
        }
        Instruction::Call { argument_count }
        | Instruction::SuperCall { argument_count }
        | Instruction::New { argument_count }
        | Instruction::CallEval { argument_count, .. } => {
            let argc = u32::from(*argument_count);
            if argc <= 7 {
                Some(format!("{name}|argv-{argc}"))
            } else {
                Some(format!("{name}|argv-many"))
            }
        }
        _ => None,
    }
}

/// Pre-handler hook: bump the base cell, record the current opcode for
/// error surfacing, and bump one refinement cell where the operands vary.
///
/// Runs from `execute_one` before the handler. Decode failure (invalid
/// bytecode is out of contract) yields no refinement cell, never a panic.
pub(crate) fn coverage_pre(context: &mut Context, opcode: Opcode) {
    let name = opcode.as_str();
    bump(name);
    context.vm.coverage_current = Some(opcode);

    let pc = context.vm.frame().pc as usize;
    let block = context.vm.frame().code_block.clone();
    let Some((instruction, _)) = block.bytecode.try_next_instruction(pc) else {
        return;
    };
    if let Some(cell) = refinement_cell(name, &instruction, &block) {
        bump(&cell);
    }
}

/// Post-handler hook for conditional jumps: bump the taken/not-taken arm.
///
/// Runs from `execute_one` after a `Continue` result only: on `Break` the
/// frame may be gone and `pc` is meaningless. `pre_pc` is the jump
/// instruction's own address (captured pre-handler). A `pc` matching
/// neither the target nor the fall-through (a handler redirect) is
/// attributed to neither arm.
pub(crate) fn coverage_post(context: &mut Context, opcode: Opcode, pre_pc: usize) {
    if !matches!(
        opcode,
        Opcode::JumpIfTrue
            | Opcode::JumpIfFalse
            | Opcode::JumpIfNotUndefined
            | Opcode::JumpIfNullOrUndefined
            | Opcode::JumpIfNotLessThan
            | Opcode::JumpIfNotLessThanOrEqual
            | Opcode::JumpIfNotGreaterThan
            | Opcode::JumpIfNotGreaterThanOrEqual
            | Opcode::JumpIfNotEqual
    ) {
        return;
    }
    let block = context.vm.frame().code_block.clone();
    let Some((instruction, fallthrough)) = block.bytecode.try_next_instruction(pre_pc) else {
        return;
    };
    let target = match instruction {
        Instruction::JumpIfTrue { address, .. }
        | Instruction::JumpIfFalse { address, .. }
        | Instruction::JumpIfNotUndefined { address, .. }
        | Instruction::JumpIfNullOrUndefined { address, .. }
        | Instruction::JumpIfNotLessThan { address, .. }
        | Instruction::JumpIfNotLessThanOrEqual { address, .. }
        | Instruction::JumpIfNotGreaterThan { address, .. }
        | Instruction::JumpIfNotGreaterThanOrEqual { address, .. }
        | Instruction::JumpIfNotEqual { address, .. } => address.as_u32(),
        _ => return,
    };
    let name = opcode.as_str();
    let pc_after = context.vm.frame().pc as usize;
    if target as usize == pc_after {
        bump(&format!("{name}|taken"));
    } else if fallthrough == pc_after {
        bump(&format!("{name}|nottaken"));
    }
}

/// Error hook: bump the `op|err-T` surfacing cell.
///
/// Runs from `handle_error` entry with the currently executing opcode.
/// Classification via [`JsError::as_native`] runs no user code, so the
/// hook cannot re-enter the VM.
pub(crate) fn coverage_error(opcode: Opcode, err: &JsError) {
    let name = opcode.as_str();
    let kind = err.as_native().map_or_else(
        || {
            if err.is_catchable() {
                "opaque"
            } else {
                "engine"
            }
        },
        |native| match native.kind() {
            JsNativeErrorKind::Aggregate(..) => "AggregateError",
            JsNativeErrorKind::Error => "Error",
            JsNativeErrorKind::Eval => "EvalError",
            JsNativeErrorKind::Range => "RangeError",
            JsNativeErrorKind::Reference => "ReferenceError",
            JsNativeErrorKind::Syntax => "SyntaxError",
            JsNativeErrorKind::Type => "TypeError",
            JsNativeErrorKind::Uri => "URIError",
        },
    );
    bump(&format!("{name}|err-{kind}"));
}

/// IC hook: bump the transition cell for one `InlineCache::set`.
///
/// `entries_after` is the entry count after a successful push (1..=4);
/// `went_mega` reports the full-cache transition instead. Steady states
/// (mega no-ops) never reach here — transitions only.
pub(crate) fn coverage_ic_transition(entries_after: usize, went_mega: bool) {
    let cell = if went_mega {
        "ic|to-mega"
    } else {
        match entries_after {
            1 => "ic|to-mono",
            2 => "ic|to-poly2",
            3 => "ic|to-poly3",
            _ => "ic|to-poly4",
        }
    };
    bump(cell);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Script, Source};
    use std::sync::Mutex;

    /// Serializes the coverage tests against each other.
    ///
    /// The histogram is process-global and every engine test that evaluates
    /// code bumps it when the feature is on. Other tests only ever *add*
    /// unrelated cells (harmless for presence assertions and unique probe
    /// cells), but two coverage tests must never interleave `clear` with
    /// `bump`/`dump`, so each test holds this guard for its whole body.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Run `source` (script goal) through a fresh context, ignoring the
    /// outcome. Errors are fine: throwing programs still execute hooks.
    fn run(source: &str) {
        let mut context = Context::default();
        let script = Script::parse(Source::from_bytes(source), None, &mut context)
            .expect("parse test source");
        drop(script.evaluate(&mut context));
    }

    /// Compile `source` (script goal) without running it.
    fn compile(source: &str) -> boa_gc::Gc<CodeBlock> {
        let mut context = Context::default();
        let script = Script::parse(Source::from_bytes(source), None, &mut context)
            .expect("parse test source");
        script.codeblock(&mut context).expect("compile test source")
    }

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[test]
    fn dump_is_empty_after_clear() {
        let _guard = serial();
        coverage_clear();
        assert!(coverage_dump().is_empty());
    }

    #[test]
    fn ic_transitions_emit_cells() {
        use crate::builtins::OrdinaryObject;
        use crate::vm::inline_cache::InlineCache;
        use crate::{JsString, js_string};

        let _guard = serial();
        coverage_clear();

        let context = &mut Context::default();
        let o = context
            .intrinsics()
            .templates()
            .ordinary_object()
            .create(OrdinaryObject, Vec::default());
        let cache = InlineCache::new(js_string!("p"));
        // Five sets walk one cache empty→mono→poly(2..4)→mega.
        for i in 0..5 {
            let property: crate::property::PropertyKey = JsString::from(format!("p{i}")).into();
            o.set(property.clone(), i, true, context)
                .expect("set should not fail");
            let shape = o.borrow().shape().clone();
            let slot = shape.lookup(&property).expect("fresh prop lookup");
            cache.set(&shape, slot);
        }
        assert!(cache.megamorphic.get(), "five sets must go megamorphic");
        let dump = coverage_dump();
        for cell in [
            "ic|to-mono",
            "ic|to-poly2",
            "ic|to-poly3",
            "ic|to-poly4",
            "ic|to-mega",
        ] {
            assert!(
                dump.iter().any(|(c, _)| c == cell),
                "missing {cell} in {dump:?}"
            );
        }
    }

    #[test]
    fn dump_cells_are_sorted_and_json_safe() {
        let _guard = serial();
        coverage_clear();
        run("let x = 1 + 2; if (x) { x = 3; }");
        let dump = coverage_dump();
        assert!(!dump.is_empty(), "executing code must bump cells");
        assert!(
            dump.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "dump must be sorted by cell"
        );
        assert!(
            dump.iter().all(|(cell, _)| cell_is_json_safe(cell)),
            "every dumped cell must hold the JSON-safe alphabet"
        );
    }

    #[test]
    fn counts_accumulate_on_a_unique_probe_cell() {
        let _guard = serial();
        coverage_clear();
        // A cell no other test bumps: exact counts are race-free here.
        bump("P54AccumProbeCell");
        bump("P54AccumProbeCell");
        let dump = coverage_dump();
        let found = dump
            .iter()
            .find(|(cell, _)| cell == "P54AccumProbeCell")
            .map(|(_, count)| *count);
        assert_eq!(found, Some(2));
    }

    #[test]
    fn store_literal_splits_string_and_bigint_shapes() {
        let _guard = serial();
        coverage_clear();
        run("'a';");
        run("10n;");
        let dump = coverage_dump();
        let cells: Vec<&str> = dump.iter().map(|(cell, _)| cell.as_str()).collect();
        assert!(
            cells.contains(&"StoreLiteral"),
            "string literal must bump the base cell: {cells:?}"
        );
        assert!(
            cells.contains(&"StoreLiteral|k-str"),
            "string literal must bump k-str: {cells:?}"
        );
        assert!(
            cells.contains(&"StoreLiteral|k-bigint"),
            "bigint literal must bump k-bigint: {cells:?}"
        );
    }

    #[test]
    fn fixed_arity_call_records_argv_cell() {
        let _guard = serial();
        coverage_clear();
        run("function f(a, b) { return a; } f(1, 2);");
        let dump = coverage_dump();
        let cells: Vec<&str> = dump.iter().map(|(cell, _)| cell.as_str()).collect();
        assert!(
            cells.contains(&"Call|argv-2"),
            "two-argument call must bump Call|argv-2: {cells:?}"
        );
    }

    #[test]
    fn loop_condition_records_both_jump_arms() {
        let _guard = serial();
        coverage_clear();
        run("for (let i = 0; i < 2; i++) {}");
        let dump = coverage_dump();
        let taken = dump.iter().any(|(cell, _)| cell.ends_with("|taken"));
        let not_taken = dump.iter().any(|(cell, _)| cell.ends_with("|nottaken"));
        assert!(
            taken && not_taken,
            "loop back-edge must record both arms: {dump:?}"
        );
    }

    #[test]
    fn thrown_type_error_records_err_cell() {
        let _guard = serial();
        coverage_clear();
        run("null.x;");
        let dump = coverage_dump();
        assert!(
            dump.iter()
                .any(|(cell, _)| cell.ends_with("|err-TypeError")),
            "throwing TypeError must record an err cell: {dump:?}"
        );
    }

    #[test]
    fn static_scan_enumerates_base_and_static_refinements() {
        let _guard = serial();
        let block = compile("function f(a, b) { return a; } f(1, 2); 'lit';");
        let cells = emitted_cells(&block);
        for want in ["Call", "Call|argv-2", "Return", "StoreLiteral|k-str"] {
            assert!(
                cells.contains(&want.to_owned()),
                "static scan must emit {want}: {cells:?}"
            );
        }
    }

    #[test]
    fn dump_json_has_object_shape() {
        let _guard = serial();
        coverage_clear();
        run("1;");
        let json = coverage_dump_json();
        assert!(json.starts_with("{\""));
        assert!(json.ends_with('}'));
        assert!(json.contains("\":"));
    }
}
