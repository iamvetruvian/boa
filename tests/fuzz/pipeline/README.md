# Fuzz corpus pipeline (P4.6)

Every crash flows: **reproduce → dedupe → minimize → regression DB**, and
every fix flows back: **seed → corpus → coverage ratchet**. This directory
holds the machinery; `known-signatures.txt` and `coverage-baseline.toml`
are the committed state.

## Crash intake: `triage-crash.sh <target> <artifact> [-s sanitizer]`

1. Reproduces through the fleet binary (fleet-visibility proof), capturing
   the drill-taxonomy signature (`exit=N :: marker-line`).
2. Dedupes against `known-signatures.txt` _before_ the expensive step.
   Open-rule matches exit as duplicates; landed-rule matches still get a
   skeleton (possible regression — fail safe); bare hangs always get a
   skeleton for manual dedupe (two hangs never auto-dedupe).
3. Minimizes via `cargo fuzz tmin` under the _same_ sanitizer (hangs skip;
   hang minimization is manual bisection).
4. Writes `triage/<slug>/`: `minimized.bin`, `repro.js` (byte targets) or
   `rendered.js` (structured targets, via `cargo run --example render`),
   `signature.txt`, and a `skeleton.toml` (`finder="fuzz"`, `status=open`).

Human admission: move the repro into `tests/regression/cases/<id>/`, fill
the skeleton into `regressions.toml`, stage `minimized.bin` as
`tests/fuzz/seeds/<id>.js` (bytes are bytes — libFuzzer ignores the
extension), run the DB test, extend `known-signatures.txt`.

## Seed bank

`seeds/` is the committed bank (P2 fix reproducers via the DB's `fuzz_seed`
requirement + P3 mismatch minimizations). `./sync-seeds.sh` copies it into
every `corpus/<target>/` at the start of each scheduled run.

- `export-diff-seeds.py`: pulls `minimized_program` bodies from
  `differential-runs/*/triage-queue.json`, strips the Test262 harness
  prelude (leading harness-known lines; order-preserving line deletion
  makes this exact), dedupes, and writes `seeds/diff-*.js`. Default:
  `boa-bug` verdicts only; `--verdicts`/`--max-total` extend, `--dry-run`
  previews. Re-run after each differential campaign.

Merge-back is the weekly coverage job: new seeds ride `sync-seeds.sh`,
`cmin` folds them into the minimized corpus, and the ratchet below
measures the gain.

## Coverage ratchet: `coverage-gate.py` + `coverage-baseline.toml`

Per target: `cmin`, `cargo fuzz coverage`, `llvm-cov export` totals.
The gate fails on any REPRODUCIBLE covered-regions decrease vs the
baseline. Coverage wobbles a few regions run to run (observed: −15 on
bytecompiler-implied, −2 on cli-exec, each `= baseline` on the next run),
so a first-below-baseline reading re-measures once before failing: noise
passes on the second reading, real regressions breach twice.

- `./pipeline/coverage-gate.py` — check mode (what CI runs).
- `./pipeline/coverage-gate.py --update-baseline` — ratchet up after new
  seeds cover more, or down when code removal legitimately drops totals
  (reason in the commit message). Missing baseline sections SKIP with
  instructions (fail-open on first run by design).
- `./pipeline/coverage-gate.py --targets vm-implied,cli-exec` — subset.

## Gotchas learned the hard way

- `cmin` hash-renames: after any `cmin`, corpus files are content hashes,
  not their intake names. The `repro-*.input` names from `sync-seeds.sh`
  survive only until the first minimization — content persists (when it
  covers), names don't. Never track corpus files by name across `cmin`.
- Bank-true baselines: unmerged fuzz finds in `corpus/` inflate coverage
  (observed: +15k regions on vm-implied from one hunt's finds), and the
  baseline must reproduce from the committed bank on a clean checkout.
  Before `--update-baseline`, restore bank-only corpora
  (`rm corpus/<t>/*; ./sync-seeds.sh <t>`) — preserving any unmerged
  finds aside first so the merge-back queue isn't lost — then measure.
- Check mode is not read-only: every run re-runs `cmin` in place, so the
  working corpus shrinks/mutates under the gate. That is by design
  (always measure the minimized bank); don't be surprised.
- `cargo fuzz coverage` never clears `coverage/<target>/raw/`: the merge
  unions every profraw batch ever written there, so back-to-back measures
  only climb (observed: +13k phantom parser regions). The gate deletes
  the target's coverage dir before each measure; never compare numbers
  across runs that skipped the clear.

## Gotcha: one cargo-fuzz command at a time

All cargo-fuzz subcommands share `tests/fuzz/target/` and mutate
`corpus/` in place. Concurrent invocations (a fuzz run + cmin, two
different sanitizer builds, …) corrupt each other: observed once as
emptied corpora with a misleading error. Serialize locally; CI already
isolates jobs on separate runners.

## Dedupe rules: `known-signatures.txt`

`<sig-id> :: <match-substring> :: <status> :: <regression-id>`. Rules must
be tight substrings (never bare `exit=N`). Stack overflows share one
message, so the parser rule doubles as the class rule — new overflows get
skeletons flagged for human splitting.
