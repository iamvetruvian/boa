# Test262 Conformance Baseline (P0.2, re-snapshotted at step-0)

Immutable snapshot of the full pinned Test262 corpus on the baseline pins
(see [`docs/baseline.md`](../docs/baseline.md)). Every later conformance
measurement is compared against `run-1/latest.json` via
`boa_tester compare --fail-on=regression` (plus the quarantine file); never
land on red.

Re-snapshotted 2026-10-06 after rebasing the harness onto upstream main
(`ef40c632`, +3 commits: Iterator take/drop limits, Promise.try, clippy
lints): 9 newly fixed tests, 0 regressions vs the P0 snapshot (strict
compare exit 0).

## Provenance

| Fact                              | Value                                                                                                                                                       |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Boa commit                        | `fcf9d9b4d778069fdc4b16d0bcd86fc1796bc972` (step-0 harness; this snapshot committed on top)                                                                 |
| Toolchain                         | `1.94.0` (`rustc 1.94.0 (4a4ef493e 2026-03-02)`)                                                                                                            |
| Test262 commit                    | `d86b2294eb0a17eaa281ff12c73c473ec864c72f` (recorded in each `latest.json` as `u`)                                                                          |
| Tester features                   | default (`intl_bundled`, `experimental`, `annex-b`)                                                                                                         |
| Command                           | `./target/release/boa_tester run -v -o <run-dir>` (built via `cargo build --release -p boa_tester --bin boa_tester` — `-p` is load-bearing, see note below) |
| run-1                             | 2026-10-06T09:14:47Z, wall 84 s, exit 0                                                                                                                     |
| run-2                             | 2026-10-06T09:16:11Z, wall 84 s, exit 0                                                                                                                     |
| run-3 (P0-era tiebreak, retained) | 2026-10-01T15:48:20Z approx, wall 98 s, exit 0 (51437 passed; kept for history, not a comparison base)                                                      |

To reproduce: `bash scripts/conformance-snapshot.sh` (builds the pinned
tester, writes fresh `run-1` / `run-2` directories, checks determinism).

NOTE (step-0): bin builds/runs must be `-p` scoped. A bare
`cargo --bin X` from the workspace root unifies features across default
members; since tools/fuzzilli joined the workspace that silently enables
`boa_engine/fuzz+verify-bytecode` and the tester fails every test with
`NoInstructionsRemain`. `scripts/verify-baseline.sh` guards this.

## Results (identical in run-1 and run-2)

| Metric      | Count  |
| ----------- | ------ |
| Total       | 53578  |
| Passed      | 51446  |
| Ignored     | 1648   |
| Failed      | 484    |
| Panics      | 0      |
| Conformance | 96.02% |

Per-edition breakdown (from `latest.json`, key `r.av`):

| Edition | Total | Passed | Ignored | Panics |
| ------- | ----- | ------ | ------- | ------ |
| es5     | 8165  | 8124   | 23      | 0      |
| es6     | 27680 | 27456  | 36      | 0      |
| es7     | 27810 | 27585  | 36      | 0      |
| es8     | 28859 | 28618  | 36      | 0      |
| es9     | 33717 | 33472  | 37      | 0      |
| es10    | 33854 | 33609  | 37      | 0      |
| es11    | 36031 | 35647  | 115     | 0      |
| es12    | 36948 | 36482  | 185     | 0      |
| es13    | 42475 | 41937  | 245     | 0      |
| es14    | 42882 | 42258  | 290     | 0      |
| es15    | 43722 | 43098  | 290     | 0      |
| es16    | 45010 | 44264  | 412     | 0      |
| es17    | 45323 | 44568  | 420     | 0      |

## Determinism verdict: PASS

- `run-1/latest.json` and `run-2/latest.json` are **byte-identical**
  (sha256 `6727513204863596…`, 4378077 bytes each).
- `run-1/results.json` and `run-2/results.json` are byte-identical
  (sha256 `485ec7105dfedda2…`, 917 bytes each).
- `run-1/features.json` and `run-2/features.json` are byte-identical
  (sha256 `e00e79c57c4b1c8d…`, 4105 bytes each; same 198-feature set as P0).
- No trailing-byte anomaly this time (cf. the P0 run-2 allow-list below).
- `run-*/crashes.json` (untracked, gitignored) carry per-run timestamps and
  are excluded from byte-determinism by design; both runs record zero crashes.

P0 note (retained for history): the original 2026-10-01 snapshot's run-2
`latest.json`/`features.json` carried one extra trailing `0x0a` byte each
(post-write artifact from an unidentified environment process; writer proven
deterministic by subset repeats and the run-3 tiebreak). Allow-listed with
justification; superseded files remain in git history.

## File inventory

| File                  | sha256                 | Notes                                               |
| --------------------- | ---------------------- | --------------------------------------------------- |
| `run-1/latest.json`   | `6727513204863596…`    | full per-test verdicts; the comparison base         |
| `run-1/results.json`  | `485ec7105dfedda2…`    | single-entry reduced history                        |
| `run-1/features.json` | `e00e79c57c4b1c8d…`    | 198-feature set, informational (unchanged since P0) |
| `run-1.console.log`   | (untracked transcript) | console transcript of run-1                         |
| `run-2/latest.json`   | `6727513204863596…`    | byte-identical to run-1                             |
| `run-2/results.json`  | `485ec7105dfedda2…`    | identical to run-1                                  |
| `run-2/features.json` | `e00e79c57c4b1c8d…`    | identical to run-1                                  |
| `run-2.console.log`   | (untracked transcript) | console transcript of run-2                         |
| `run-3/*`             | (P0 snapshot)          | retained P0-era tiebreak (51437 passed)             |

(Full hashes: see `sha256sum` output recorded at snapshot time; abbreviated
here for readability.)
