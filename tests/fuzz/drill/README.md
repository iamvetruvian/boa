# Seeded-crash drill

Validates the crash→minimize→dedupe→regress pipeline against KNOWN crashers
(P4 validation gate: five entries, distinct signatures).

## Run

```bash
./run.sh
```

Exit 0 when every inventory entry crashed with a distinct signature.
An entry that exits cleanly is reported `RETIRED` and fails the drill until
a human graduates it (fixed bugs move to the regression DB; the inventory
only lists live crashers).

## Inventory status

`inventory.toml` holds 1/5 live entries:

- `huge-sparse-join-abort` — allocation-failure abort under the `cli-exec`
  `fuzz-target` harness (migrated from `boa-eval` in P4.3; the `tmin` leg
  runs). Deliberately left live: OOM-guard policy is engine design work,
  out of scope for the fuzz foundations.

Graduated to the regression DB (fixed; the drill's fail-closed rule
required their removal once they stopped crashing):

- `cyclic-proto-walk-stack-overflow` — found live by `cli-exec` on its
  first run (seeded input reproduced the SIGSEGV immediately), fixed with
  stack-aware walk guards + iteration budgets (landed, `throw RangeError`).
- `parser-deep-nesting-stack-overflow` — found by the `source-bytes`
  harness during P4.3 construction, fixed with a stack-aware parser guard
  (landed, `throw SyntaxError`).

Growth plan: the background vm/bytecompiler soaks, the nightly fleet, and
continued P4.4–P4.7 adversarial running supply entries 2–5.

## P4 crash gate: `crash-gate.sh` + `inventory-gate.toml` (5/5 green)

The gate reintroduces five *historical* crash bugs via
`crash-patches/*.break.patch` (revert-patch pattern from `logic/patches/`),
runs the seeded fleet against `inventory-gate.toml`, and restores the tree
byte-identical (snapshot hash + `Cargo.lock` bytes; an EXIT trap makes
interrupts safe). Never commits.

The five (distinct message+location signatures; pids are normalized out of
dedupe so two same-message crashes can never pass as distinct):

- `parser-deep-nesting-stack-overflow` — WT fix revert (bug #22);
  boa-eval, native-SO line. Input is exact-minimal (217 parens; bisected
  216-ok/217-crash, recalibrate if the toolchain moves it).
- `padstart-oom` — reverted 419a8ed5; boa-eval, allocation-failure line.
- `function-ctor-nested-lexical` — reverted 957e6f83; cli-exec,
  `must be declarative environment` at define/mod.rs:126 (tmin-ok).
- `class-scope-index` — reverted 46e92c75; cli-exec, index-OOB with len
  in scope_analyzer.rs (tmin-ok).
- `dataview-unaligned-assert` — reverted a6101fe1; cli-exec,
  debug_assert! at array_buffer/utils.rs:654 (tmin-ok; index 6 of a
  9-byte SAB is the tripping alignment, probed 0–8).

Two deliberate exclusions (documented in the inventory header):
cyclic-proto shares parser-nesting's exact SO message (never
signature-distinct; the parser class rule covers both), and
sloppy-this-assign's todo!() arm is unreachable (the parser rejects
``this =`` before bytecompile — found by the gate's own forensics,
which also noted sloppy `this = 5` *should* be a legal no-op per spec).

Harness rule learned here: parse-only targets (source-bytes) cannot
re-trip compile/eval-time bugs — match the harness to the crash layer
(full-eval cli-exec/boa-eval for anything past parsing). The runner saves
each entry's reproduce output to `/tmp/drill-<id>.out` and tolerates
hang-class entries (timeout IS the crash; tmin skipped, mirroring
triage-crash.sh).

## Signatures

Dedupe keys are stderr substrings (`signature_match`). Hang entries key on
their exit code (`exit=124`); two hangs sharing it fail dedupe on purpose —
a human must confirm whether they are the same bug.

## Triaging a crash without sanitizer artifacts

The fleet runs `-s none` (no sanitizer), so a SIGSEGV/SIGABRT kills the
fuzzer child WITHOUT writing a `crash-*` artifact and without printing the
offending input — libFuzzer warns about the missing
`__sanitizer_acquire_crash_state` hooks on every run. When a target dies
this way:

1. Don't re-fuzz blindly. First sweep the seeds directly through the
   target binary — a seeded reproducer is the most likely cause and this
   identifies it in seconds:
   `for f in corpus/<target>/*; do target/x86_64-unknown-linux-gnu/debug/<target> "$f" || echo "CRASH: $f"; done`
   (This is exactly how `cli-exec`'s SIGSEGV was traced to the
   `cyclic-proto-walk-stack-overflow` seed.)
2. If no seed reproduces, the input was fuzzer-generated and is lost;
   re-run with `-s address` (slower, but then artifacts work) or narrow
   with `-seed=` bisection.
3. Minimize with `cargo fuzz tmin` once the input is in hand; the drill's
   `fuzz-target` harness runs the `tmin` leg automatically.
