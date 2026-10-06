# Bytecode validity model

What `ByteCompiler::finish` (`core/engine/src/bytecompiler/mod.rs`) establishes
and what the VM may assume. Companion to the machine checker
`CodeBlock::verify` (`core/engine/src/vm/verify.rs`), which enforces every
`[V]` invariant below and returns a precise `VerifyError` otherwise. Anything
marked `[D]` (dynamic) is intentionally unchecked: it depends on runtime state
verify cannot see.

Field names below are current as of P5 (2026-10-03); the checker, not this
document, is authoritative when they drift.

## 0. Contract status

- The VM only executes bytecode emitted and patched by Boa (`finish` plus the
  `DUMMY_ADDRESS` patch protocol). There is **no surface that accepts
  untrusted bytecode**: no bytecode loader, serializer, or deserializer
  exists in the tree.
- Behavior on invalid bytecode is therefore **explicitly out of contract**:
  the VM's operand decoders assert on short reads
  (`core/engine/src/vm/opcode/args.rs`) and its table accessors panic on
  type/arity mismatch rather than returning recoverable errors. That is by
  design — a decode failure proves a compiler bug — and `verify` exists to
  turn those bugs into precise pre-execution errors in tests and fuzzers.
- `verify` runs in `#[cfg(test)]` builds (via a hook at the end of `finish`)
  and in any build with the `verify-bytecode` engine feature (fuzzing). It
  never runs in production builds; whether it should is a Project-2
  decision, default no.

## 1. Instruction stream (`Bytecode.bytes: Box<[u8]>`)

`[V]` The stream decodes fully from pc 0: every byte belongs to exactly one
instruction (opcode byte + operands per `generate_opcodes!`). No trailing
bytes, no truncated operands. `verify` uses its own bounds-checked decoder
and returns `Err` on short reads; it never panics.

`[V]` No `Reserved1..=Reserved61` opcode appears (`Opcode` discriminants
195..=255 are unassigned; executing one is a dispatch-table bug, not a
no-op — reserved handlers map to the `Reserved` operation).

`[V]` The last instruction of every block is `Return`, appended by `finish`.
(A block whose decode walk ends mid-instruction or on any other opcode was
not produced by `finish`.)

## 2. Jump targets (`Address` operands, jump tables, handlers)

An `Address` is a byte offset into `bytes`. `DUMMY_ADDRESS = u32::MAX` is the
emit-then-patch placeholder (`BytecodeEmitter::patch_jump/patch_jump_table`);
any surviving dummy is a compiler bug.

`[V]` Every `Address` operand (`Jump*`, `LogicalAnd/Or`, `Coalesce`, `Case`,
`TemplateLookup`) and every entry of every `JumpTable.addresses: ThinVec<Address>`
lands on an instruction boundary strictly below `bytes.len()`, and is never
`DUMMY_ADDRESS`.

`[V]` `TemplateLookup { address, site, .. }` pairs with the `TemplateCreate`
immediately _preceding_ its jump target, with an equal `site`. The lookup
jumps to `address` on cache hit (skipping creation); the create runs on
miss. Part-register stores sit between the lookup and the create, so the
pairing is checked at the target, not at the fall-through.

`[V]` Every `Handler { start, end, environment_count }`: `start <= end`
(`patch_handler` asserts this), both are instruction boundaries below `len`,
neither is `DUMMY_ADDRESS`. `end` is exclusive (`Handler::contains` is
`[start, end)`); `finish` appends `Return` after all patching, so `end` is
always strictly below `len`.

`[D]` `environment_count <= open environments at runtime`. Environment depth
is dynamic (push/pop along the taken path); verify cannot compute it
statically. Mismatches unwind the wrong number of scopes at throw time.

## 3. Registers (`RegisterOperand`, `register_count`)

`[V]` Every `RegisterOperand` (`dst/src/value/...`), plus the `u32`-typed
register indices (`JumpTable.index`, every element of
`TemplateCreate.values` and `CopyDataProperties.excluded_keys`,
`ConcatToString.values`), is strictly below `register_count`
(`RegisterAllocator::finish` = slots used; the frame is sized from it at
call time).

`[V]` Fixed frame prefix: `register_count >= 1` always (`r0` is the
undefined register); `>= 4` when `IS_ASYNC` (promise triple `r1..=r3`);
`>= 5` when `IS_ASYNC && IS_GENERATOR` (async-generator object `r4`).
Indices are `CallFrame::*_REGISTER_INDEX`.

`[V]` `TemplateCreate.values` has even length (cooked/raw register pairs).

`[D]` Liveness (a register is written before it is read on every path).
`finish` only `debug_assert`s allocator hygiene (no leaked non-persistent
registers); use-before-def is possible in principle and would read
`undefined` from the zeroed frame. Not checked.

## 4. Constants (`constants: ThinVec<Constant>`)

`Constant::{String, Function, BigInt, Scope}`. Consumers panic (`constant_*`
helpers, direct indexing, or `unreachable!`) on arity or type mismatch, so
every reference is checked for bounds **and** expected type:

| Site                                                                                                                  | Expected type        |
| --------------------------------------------------------------------------------------------------------------------- | -------------------- |
| `StoreLiteral.index`                                                                                                  | `String` or `BigInt` |
| `StoreRegexp.{pattern,flags}_index`                                                                                   | `String`, `String`   |
| `GetFunction.index`                                                                                                   | `Function`           |
| `CallEval.scope_index`, `CallEvalSpread.scope_index`, `PushScope.scope_index`                                         | `Scope`              |
| `InPrivate.index`, `GetMethod.name_index`, `ThrowMutateImmutable.index`, `ThrowNew{Type,Reference}Error.message`      | `String`             |
| Every `*_by_name name_index` (`Define*`, `Set*`, `GetPrivateField`, `DeletePropertyByName`, private-field/method ops) | `String`             |
| `PushPrivateEnvironment.name_indices` (each element)                                                                  | `String`             |

`[V]` `verify` enforces the whole table, then recurses into every nested
`Constant::Function` (depth-capped; the parser already bounds source
nesting, the cap is defense in depth for fuzzer-built blocks).

Not constant references (checked elsewhere or total by construction):
`GetArgument.index` (argument position; OOB reads `undefined`),
`{Super,}Call/New.argument_count` (plain counts), `SetFunctionName.prefix`
(`1`/`2`/`_` total), `CreateIteratorResult.done` and
`PushClassField.is_anonymous_function` (`!= 0` booleans),
`ImportCall.phase` (see below), `JumpTable.index` (a register, §3).

`[V]` `ImportCall.phase <= 2` (`0` evaluation, `1` defer, `2` source —
the only values the compiler emits).

## 5. Bindings (`bindings: Box<[BindingLocator]>`)

`[V]` Every `binding_index` operand (`DefVar`, `DefEvalVar`, `DefInitVar`,
`PutLexicalValue`, `DeleteName`, `GetName`, `GetNameGlobal`, `GetLocator`,
`GetNameAndLocator`, `GetNameOrUndefined`, `SetName`) is strictly below
`bindings.len()`. (`SetNameByLocator` carries no index.)

`[D]` `BindingLocator { scope, binding_index }` resolution against the
runtime environment (global object `0`, global declarative `1`, stack
`n + 2`). Scope-stack depth and per-scope binding counts are dynamic, so a
statically well-formed locator can still index out of bounds at runtime —
this is exactly the `using-binding-index-oob` regression-DB class. A
static environment-depth dataflow is future work, explicitly not in P5.

## 6. Inline caches (`ic: Box<[InlineCache]>`)

`[V]` Every `ic_index` operand (`GetNameGlobal`, `GetLengthProperty`,
`GetPropertyByName{,WithThis}`, `SetPropertyByName{,WithThis}`) is strictly
below `ic.len()`.

`[V]` IC slots are dense and used exactly once: the compiler assigns
`ic_index = self.ic.len()` then pushes, at each emit site, so the set of
referenced indices must equal `0..ic.len()`. A shared or orphan slot is a
compiler bug.

`[V]` Name correspondence for the checkable site: `GetNameGlobal`'s
`ic[ic_index].name` equals `bindings[binding_index].name()` (both come from
the same binding). Property-access IC names live only in the cache entry —
there is no second copy to compare against — so only bounds + uniqueness
apply. IC `entries` are runtime-populated and never checked.

## 7. Globals (`global_lexs/global_fns/global_vars`)

`[V]` Every `global_lexs`/`global_vars` entry indexes a `String` constant;
every `GlobalFunctionBinding { name_index, function_index }` indexes a
`String` and a `Function` constant respectively.

## 8. Block-level facts (`CodeBlock` header)

`[V]` §1 tail rule (ends with `Return`).

`[V]` Fixed-register prefix (§3).

`[V]` `length <= parameter_length` (`length` counts parameters before the
first default/rest; it never exceeds the declared parameter count).

`[V]` `IS_ASYNC` blocks carry at least one `Handler` (`finish` patches the
async handler that rejects the capability on throw).

Not checked: `this_mode` (no invalid value exists), `flags` bits beyond the
above, `source_info` (debug data; may be empty), `debug_id` (unique counter),
`mapped_arguments_binding_indices` (values index the runtime function
environment — dynamic, §5).

## 9. Checker properties

- Total: `verify` never panics, including on truncated streams, absurd
  `ThinVec` length prefixes (no `with_capacity` on untrusted lengths), and
  deeply nested constants (depth cap).
- Precise: every error carries the offending pc, the opcode, and the
  violated invariant.
- Sound (no false positives): `verify` accepts 100% of compiler-produced
  blocks across the full Test262 corpus (acceptance test in `verify.rs`;
  any rejection is a checker bug until proven a compiler bug).
- Complete on the machine side: a crafted invalid set (bad jumps,
  truncated operands, reserved opcodes, OOB registers/constants/ICs/
  bindings, type-mismatched constants, unterminated blocks) is rejected
  with the expected variants.
