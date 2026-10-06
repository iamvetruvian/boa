# Boa-specific mutant set (P7.2)

Generic `cargo-mutants` operators (return-value swaps, operator flips) miss
engine semantics: a `<`-vs-`<=` swap in the bytecode slow path, a dropped
prototype-chain step, or a forced-megamorphic IC are not single-operator
mutations of one function. This set covers those, plus the three product
files `cargo-mutants` cannot see (see `scripts/check-mutation-filter.sh`):
`core/engine/src/value/inner/nan_boxed.rs` (BM-NAN),
`core/runtime/src/fetch/tests/mod.rs` (BM-FETCH), and
`core/engine/src/sys/fallback/mod.rs` (vacuous — proven below, no mutant).

## Rules

- **Pinned toolchain.** All runs use the P0-pinned `1.94.0` toolchain, same
  as `scripts/run-mutation.sh` enforces. Mutant diffs drift across toolchains.
- **Never mutate the oracles.** The `fuzz`/`trace` shims, the coverage
  instrumentation (`vm-coverage`), and test harnesses are not targets.
- **Scratch discipline.** Each mutant is applied, run, and reverted in the
  working tree (sequential introduce→run→revert, zero residue verified by
  `git diff` after each — the P6 seeded-UB drill protocol). No commits.
- **Kill bar.** Every mutant must be killed by its listed killer (suite goes
  red). A survivor gets a new killing test or a written justification in
  `docs/mutation-survivors.md`, reviewed like an ignore entry.
- **Statuses.** `PROVEN` = executed 2026-10-04 with the red signal observed;
  `DESIGNED` = transform + killer specified, execution pending.

## The set

| ID         | Class (plan §7.2)                 | File                                                              | Killer                                                                               |
| ---------- | --------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| BM-CMP-1   | comparison-boundary flip          | `core/engine/src/value/operations.rs` (`lt_fast`)                 | `cargo test -p boa_engine --lib control_flow::loops`                                 |
| BM-IC-1    | skipped IC guard (plan-literal)   | `core/engine/src/vm/inline_cache/mod.rs` (`set`)                  | `cargo test -p boa_engine --features vm-coverage ic_transitions`                     |
| BM-IC-2    | skipped IC guard (inverted)       | `core/engine/src/vm/inline_cache/mod.rs` (`get`)                  | `cargo test -p boa_engine --lib inline_cache`                                        |
| BM-PROTO-1 | dropped prototype-chain step      | `core/engine/src/object/internal_methods/mod.rs` (`ordinary_get`) | `cargo test -p boa_engine --lib object`                                              |
| BM-REG-1   | operand perturbation              | `core/engine/src/vm/opcode/mod.rs` (`RegisterOperand::new`)       | `cargo test -p boa_engine --lib value::tests::abstract_equality_comparison`          |
| BM-ERR-1   | swapped error type                | `core/engine/src/builtins/string/mod.rs` (`starts_with`)          | `cargo test -p boa_engine --lib builtins::string::tests::starts_with_with_regex_arg` |
| BM-GC-1    | removed GC root accounting        | `core/gc/src/lib.rs` (`sweep`)                                    | `cargo test -p boa_gc weak`                                                          |
| BM-NAN-1   | codec boundary (blind-file cover) | `core/engine/src/value/inner/nan_boxed.rs` (`tag_f64`)            | `cargo kani -p boa_engine --harness kani_codec_f64_canonical`                        |
| BM-FETCH-1 | fake-breaking (blind-file cover)  | `core/runtime/src/fetch/tests/mod.rs` (`add_response`)            | `cargo test -p boa_runtime fetch::tests`                                             |

### BM-CMP-1 — `lt_fast` means `<=` (PROVEN)

Transform (`core/engine/src/value/operations.rs`, both arms of `lt_fast`):
`Some(x < y)` → `Some(x <= y)` (i32 path and f64 path). The slow path
(`lt` via `abstract_relation`) still means `<`, so fast/slow disagree as
well as the operator being wrong. Killer: `control_flow::loops` (10 loop
tests compare variables at runtime). Gotcha learned in execution: the
obvious killer (`abstract_relational_comparison`, all-literal assertions)
stays GREEN because constant folding
(`optimizer/pass/constant_folding.rs:250`) evaluates literal `<` at compile
time via the slow path — `lt_fast` only sees non-foldable operands. Full
suite red: 16 failed / 1152 passed.

### BM-IC-1 — caching disabled via forced megamorphic (PROVEN)

Transform (`InlineCache::new`): `Cell::new(false)` → `Cell::new(true)`.
Every cache is born megamorphic; `set` early-returns via the existing guard,
so no caching ever happens. Lookups
stay _correct_ (slow path is the reference), so the behavioral `object` suite
(121 tests) stays GREEN — perf-only as far as behavior goes. Two layers pin
the optimization itself and both go red: the IC's own unit tests (3 failed:
`test_polymorphic_inline_cache`,
`set/get_property_by_name_set_inline_cache_on_property_load`) and the P7.1
state-dimension test `ic_transitions_emit_cells` (no transitions emitted).
(Correction 2026-10-04: an earlier draft claimed only the coverage test could
kill this; the PR-simulation of `set with ()` suggested otherwise and a
direct rerun confirmed the unit tests kill it too. The coverage kill stands
as an independent second layer, not an exclusive one.)

### BM-IC-2 — shape guard inverted (PROVEN)

Transform (`InlineCache::get`): `upgraded.to_addr_usize() == shape_addr` →
`!=`. The cache returns the first _non-matching_ entry's slot. Killer: the
IC's own unit tests (`inline_cache` filter:
`test_polymorphic_inline_cache`, `test_megamorphic_inline_cache`). Analysis
learned in execution: the `object` filter (120 tests) stays GREEN because
every site there is monomorphic — duplicate entries push until the cache
goes megamorphic, and the slow path keeps results correct (2.5x slower,
188s full suite). Only genuinely polymorphic sites corrupt. Full-suite red:
2 failed / 1166 passed.

### BM-PROTO-1 — parent walk dropped (PROVEN)

Transform (`ordinary_get`, `None` arm): replace the
`if let Some(parent) = ...` / `else` block with `Ok(JsValue::undefined())`.
Inherited properties become invisible. Killer: `object` filter (inheritance
is load-bearing across builtins, Test262, and the P5 battery).
Observed: 46 failed / 74 passed under the `object` filter.

### BM-REG-1 — register indices shifted (PROVEN)

Transform (`RegisterOperand::new`): `Self(value)` →
`Self(value.wrapping_add(1))`. Every register reference in every bytecode
stream shifts by one: total breakage. This is a pipeline-liveness mutant —
if any suite stays green, the suite is vacuous. Killer: a single fast test
(`abstract_equality_comparison`); expect red within seconds of rebuild.
(Sibling `IndexOperand::new` noted, not mutated: same class, same killer.)

### BM-ERR-1 — TypeError swapped for RangeError (PROVEN)

Transform (`starts_with`, regexp guard): `JsNativeError::typ()` →
`JsNativeError::range()`. Killer `starts_with_with_regex_arg` asserts
`JsNativeErrorKind::Type` plus the exact message: one test, seconds.

### BM-GC-1 — sweep skips root-count reset (PROVEN)

Transform (`sweep`, marked strong-node arm): delete the
`node_ref.reset_non_root_count();` line. Non-root counts accumulate across
collections, garbage looks rooted, nothing is swept (deterministic leak, no
UB — deleting a safe call is safe). Killer: `weak` filter.
Observed (stronger than predicted): stale counts corrupt the sweep —
deterministic SIGABRT (`unaligned tcache chunk`, glibc heap check), 3/3 runs
(`weak` ×2, full gc lib ×1), not a mere assertion failure. Scratch-only,
reverted immediately.

### BM-NAN-1 — NaN not canonicalized (PROVEN)

Transform (`bits::tag_f64`): `f64::NAN.to_bits()` → `value.to_bits()` (keep
the `is_nan()` branch shape). Non-canonical NaN payloads leak into tagged
values. Precedent: P6 seeded-UB drill Bug 1 at this exact site — native
GREEN, Miri GREEN, Kani RED naming `NaN canonicalizes`. Killer is therefore
Kani alone (`kani_codec_f64_canonical`); a Kani RED both kills the mutant and
proves the proof layer is load-bearing for the codec. Observed:
`VERIFICATION:- FAILED`, `Failed Checks: NaN canonicalizes`, KANI_EXIT=1
(`target/kani-bm-nan.log`). Matches the P6 drill Bug 1 precedent exactly.

### BM-FETCH-1 — TestFetcher drops mappings (PROVEN)

Transform (`TestFetcher::add_response`): replace `self.request_mapper.insert(url, response);`
with an empty body. Tests that map a stub response then fetch it must fail
with `No response found for URL`. If they pass, the fetch tests never exercise
the fake — a vacuous-harness finding, not a product finding. Killer:
`cargo test -p boa_runtime fetch::tests`. Observed: 9 failed / 32 passed —
the fake is load-bearing, the harness is not vacuous.

## Vacuity: `core/engine/src/sys/fallback/mod.rs`

The file is a 3-line `pub(crate) use std::time;` reexport with no branch,
operator, or call. Proven zero-mutant-generating:

    cargo mutants --no-config --Zmutate-file core/engine/src/sys/fallback/mod.rs
    # (no output — EXIT=0)

`scripts/check-mutation-filter.sh` re-proves this on every run (kind
`VACUOUS`): if the file ever gains mutable code, the gate fails and forces
reclassification. No mutant is defined for it.

## Execution log

(append-only; one line per mutant execution)

- 2026-10-04 BM-ERR-1: killer red (1 failed / 1167 filtered), reverted clean.
- 2026-10-04 BM-CMP-1: `abstract_relational_comparison` green (folding bypass,
  documented above); full suite red (16 failed / 1152 passed); scoped killer
  `control_flow::loops` red; reverted clean.
- 2026-10-04 BM-REG-1: killer red (panic at bytecompiler/mod.rs:2837),
  reverted clean (remaining opcode/mod.rs diff is pre-existing P5.1 work).
- 2026-10-04 BM-PROTO-1: killer red (46 failed / 74 passed), reverted clean.
- 2026-10-04 BM-IC-2: `object` filter green (mono-site degradation,
  documented above); full suite red (2 failed / 1166 passed, the IC's own
  poly/mega unit tests); reverted clean.
- 2026-10-04 BM-IC-1: coverage killer red (`ic_transitions_emit_cells`),
  behavioral `object` suite green (121 passed); IC unit tests also red
  (3 failed, confirmed by direct rerun); reverted clean.
- 2026-10-04 BM-GC-1: killer SIGABRT (deterministic, 3/3 runs), reverted clean.
- 2026-10-04 BM-FETCH-1: killer red (9 failed / 32 passed), reverted clean.
- 2026-10-04 BM-NAN-1: Kani red (`Failed Checks: NaN canonicalizes`,
  KANI_EXIT=1, `target/kani-bm-nan.log`), reverted clean.
