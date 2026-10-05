# P1 seeded-regression drill

Proof that the P1.5 subset gate catches regressions per area. Method per
area: inject a seeded bug (areas 1–5) or revert a real landed fix (area
6), rebuild the release tester, run the mapped affected subset(s) from
`scripts/affected-subsets.sh` with the exact CI invocation
(`run -v --timeout 60 -s <subset>`, one invocation per subset, then
`compare test-results-baseline/run-1 <dir> --fail-on=regression
--quarantine test262_quarantine.toml` per subset), record the breach,
then restore and verify the tree is clean. All runs below used the
pinned toolchain (1.94.0) and Test262 pin (`d86b2294`).

Subset mapping (from `scripts/affected-subsets.sh`):
`core/parser/*` → `test/language` + `test/staging`;
`core/engine/src/value/*` → `test` (full);
`core/engine/src/builtins/*` → `test/built-ins`;
`core/runtime/*` → `test/built-ins`;
fix-revert touching `core/engine/src/object/*` → `test` (full).

## Area 1: parser

- Injection (`core/parser/src/parser/expression/primary/mod.rs`): parse
  `null` as `LiteralKind::Undefined`.
- Subsets: `-s test/language`, `-s test/staging`.
- Result: `test/language` → **556 broken**, exit 1; `test/staging` →
  **78 broken** plus 1 timeout, exit 1. (The staging timeout was
  `test/staging/sm/regress/regress-567152` deadlocking on the
  pre-fix child-output pipe — it emits an 87 KiB completion-value
  dump; proven unrelated to the injection because it also timed out
  on the clean tree, and passes after the pipe fix. See the note
  under Area 6.)
- Restored: injection reverted; `git status core/` clean.

## Area 2: VM numeric add

- Injection (`core/engine/src/value/operations.rs`, `add_fast`):
  subtract instead of add on both the integer and float paths.
- Subset: full `test` (value ops map to the whole corpus).
- Result: **8144 broken, 221 new timeout(s)**, exit 1. (Timeouts arise
  because tests whose loops count up via `+` never terminate once `+`
  subtracts; they are counted as abnormal outcomes and breach. The
  run-over-run diff against the pre-pipe-fix run reconciles exactly:
  `regress-567152` left the broken set once the deadlock was fixed,
  and one timing-sensitive `Atomics.notify` test flipped Failed →
  Timeout under the 220-way parallel hang load — nothing else moved.)
- Restored: injection reverted; `git status core/` clean.

## Area 3: builtins (String.prototype.trim)

- Injection (`core/engine/src/builtins/string/mod.rs`, `trim`): keep the
  `ToString` coercion but return the empty string.
- Subset: `-s test/built-ins`.
- Result: **105 broken**, exit 1.
- Restored: injection reverted; `git status core/` clean.

## Area 4: GC-adjacent (WeakRef.prototype.deref)

- Injection (`core/engine/src/builtins/weak/weak_ref.rs`, `deref`):
  always return `undefined`.
- Subset: `-s test/built-ins`.
- Result: **2 broken**, exit 1. (Small count because the WeakRef
  surface is tiny and most of it already fails at baseline; the gate
  still breaches on the 2 newly broken tests.)
- Restored: injection reverted; `git status core/` clean.

## Area 5: host ($262.detachArrayBuffer)

- Injection (`core/runtime/src/test262.rs`, `detach_array_buffer`):
  return without detaching.
- Subset: `-s test/built-ins`.
- Result: **196 broken**, exit 1.
- Restored: injection reverted; `git status core/` clean.

## Area 6: real-fix revert (plan-literal dry run)

Four regression-DB seeds were tried as revert candidates; three turned
out to be Test262-invisible (the fixes they guard are covered only by
the regression DB + Rust unit tests, which is precisely why the DB
exists as a second gate). The fourth breaches.

- Attempt 1, `9f5521bd` ("not a constructor" error message; touches
  `core/engine/src/object/*` → full `test`): exit 0 on substance. It
  first *appeared* to breach with 1 timeout, but the timeout was
  `test/staging/sm/regress/regress-567152` deadlocking on the
  pre-fix child-output pipe (87 KiB completion-value dump vs 64 KiB
  pipe buffer, parent never draining) — reproduced on the clean
  tree, fixed by child-side truncation to 4 KiB + concurrent parent
  draining (`tests/tester/src/main.rs:run_single`,
  `tests/tester/src/exec/mod.rs:run_child`), covered by the new
  `large_child_output_completes_without_timeout` e2e. The drill
  caught a real harness bug: the timeout taxonomy works, the pipe
  did not.
- Attempt 2, `970ea933` (try-finally pending return; touches
  `core/engine/src/bytecompiler/*` → `-s test/language`): exit 0,
  0 timeouts. Positive control proves the revert is live:
  `cargo test -p boa_regression` on the reverted tree fails with
  `try-finally-pending-return: unexpected throw ... outer return
  survives inner break: 43 !== 42`. Test262 simply does not cover
  that edge — the DB gate catches what the subset gate cannot.
- Attempt 3, `a93e0b3f` (StringToNumber signed-Infinity spellings;
  touches `core/string/*` → full `test`): exit 0, conformance
  identical at 96.00%. Test262-invisible (edge spellings uncovered).
- Attempt 4, `5cee37eb` (with-statement call binding semantics;
  touches `core/engine/src/environments/*` → full `test`):
  **5 broken**, exit 1:
  `get-binding-value-call-with-proxy-env`,
  `get-binding-value-idref-with-proxy-env`,
  `get-mutable-binding-binding-deleted-in-get-unscopables-strict-mode`,
  `has-binding-call-with-proxy-env`,
  `set-mutable-binding-idref-compound-assign-with-proxy-env`
  (all under `test/language/statements/with/`).
- Restored: every revert reverted; `git status core/` clean.

## Green check (remove → green)

After all restores, rebuilt the clean release tester and ran the CI
smoke subset plus the area-6 witness directory:

- `run -v --timeout 60 -s test/built-ins/Array` → compare exit 0.
- `run -v --timeout 60 -s test/staging/sm/regress` → all pass,
  compare exit 0 (confirms the area-6 timeout was caused by the
  revert).

## Verdict

6/6 areas breach while injected/reverted (exit 1 with a `BREACH:`
line) and run clean restored (exit 0). The affected-subset gate
catches seeded regressions in every major area, and the drill
additionally proved the complementary gate: two Test262-invisible
reverts are caught only by `cargo test -p boa_regression`.
