# Test262 Conformance Baseline (P0.2)

Immutable snapshot of the full pinned Test262 corpus on the P0 baseline pins
(see [`docs/baseline.md`](../docs/baseline.md)). Every later conformance
measurement is compared against `run-1/latest.json` via
`boa_tester compare` (strict gating arrives in P1; until then, compare
manually and never land on red).

## Provenance

| Fact | Value |
|---|---|
| Boa commit | `39cd11214ecb366200157dab96b3919e61e9a391` |
| Toolchain | `1.94.0` (`rustc 1.94.0 (4a4ef493e 2026-03-02)`) |
| Test262 commit | `d86b2294eb0a17eaa281ff12c73c473ec864c72f` (recorded in each `latest.json` as `u`) |
| Tester features | default (`intl_bundled`, `experimental`, `annex-b`) |
| Command | `./target/release/boa_tester run -v -o <run-dir>` (built from the pinned toolchain) |
| run-1 | 2026-10-01T15:08:08Z, wall 122 s, exit 0 |
| run-2 | 2026-10-01T15:10:35Z, wall 108 s, exit 0 |
| run-3 (tiebreak, see below) | 2026-10-01T15:48:20Z approx, wall 98 s, exit 0 |

To reproduce: `bash scripts/conformance-snapshot.sh` (writes fresh `run-1` /
`run-2` directories and checks determinism itself).

## Results (identical in all three runs)

| Metric | Count |
|---|---|
| Total | 53578 |
| Passed | 51437 |
| Ignored | 1648 |
| Failed | 493 |
| Panics | 0 |
| Conformance | 96.00% |

Per-edition breakdown (from `latest.json`, key `r.av`):

| Edition | Total | Passed | Ignored | Panics |
|---|---|---|---|---|
| es5 | 8165 | 8124 | 23 | 0 |
| es6 | 27680 | 27455 | 36 | 0 |
| es7 | 27810 | 27584 | 36 | 0 |
| es8 | 28859 | 28617 | 36 | 0 |
| es9 | 33717 | 33471 | 37 | 0 |
| es10 | 33854 | 33608 | 37 | 0 |
| es11 | 36031 | 35646 | 115 | 0 |
| es12 | 36948 | 36481 | 185 | 0 |
| es13 | 42475 | 41936 | 245 | 0 |
| es14 | 42882 | 42257 | 290 | 0 |
| es15 | 43722 | 43097 | 290 | 0 |
| es16 | 45010 | 44255 | 412 | 0 |
| es17 | 45323 | 44559 | 420 | 0 |

## Determinism verdict: PASS (with one allow-listed artifact)

- `run-1/latest.json` and `run-3/latest.json` are **byte-identical**
  (sha256 `236433c0…4a6dc52`, 3866559 bytes each).
- `run-1/results.json` and `run-2/results.json` are byte-identical
  (sha256 `284ca247…5ccaa064`, 651 bytes each); run-3's matches too.
- `run-1` and `run-2` `latest.json` files are **semantically identical**
  (parsed JSON verdict trees compare equal, including per-test outcomes and
  the recorded Test262 commit), but `run-2/latest.json` carries **one extra
  trailing `0x0a` byte** (3866560 bytes, sha256 `ba483e41…79ecab4`), as does
  `run-2/features.json` (4106 vs 4105 bytes; same 198-feature set).
- Filesystem forensics: run-2's `results.json` mtime/ctime match the run end
  (20:42:23), while `latest.json` (20:42:59) and `features.json` (20:43:31)
  were each extended ~30 s apart *after* the tester process had exited
  (exit 0, wall 108 s). A repeat subset experiment (`-s test/built-ins/Array`
  twice into `/tmp`) produced three byte-identical files with no trailing
  byte, and the full run-3 tiebreak reproduced run-1 byte-for-byte — so the
  tester writer itself is deterministic and the run-2 trailing bytes are a
  post-write artifact from an unidentified environment process (a concurrent
  workspace session was asked and ruled out; no watcher processes were found).
- Disposition: **allow-listed with this justification**. The verdict files
  that gates consume (`latest.json`) agree semantically across all three runs
  and byte-for-byte across run-1/run-3. No source or configuration change was
  made in response; P1 (tester hardening) owns writer canonicalization if the
  anomaly ever recurs. The run-2 files are kept unmodified as evidence.

## File inventory

| File | sha256 | Notes |
|---|---|---|
| `run-1/latest.json` | `236433c0…4a6dc52` | full per-test verdicts; the comparison base |
| `run-1/results.json` | `284ca247…5ccaa064` | single-entry reduced history |
| `run-1/features.json` | `e00e79c5…45e526921a` | 198-feature set, informational |
| `run-1.console.log` | `63d7286a…26beaa541f1` | console transcript of run-1 |
| `run-2/latest.json` | `ba483e41…715ec79ecab4` | semantically identical to run-1 + trailing `0x0a` (see above) |
| `run-2/results.json` | `284ca247…5ccaa064` | identical to run-1 |
| `run-2/features.json` | `a180637a…49ed792eafe8` | same 198-feature set + trailing `0x0a` (see above) |
| `run-2.console.log` | `b62df1b7…f06a80b982` | console transcript of run-2 |
| `run-3/latest.json` | `236433c0…4a6dc52` | tiebreak: byte-identical to run-1 |
| `run-3/results.json` | `284ca247…5ccaa064` | tiebreak: identical to run-1 |
| `run-3/features.json` | `e00e79c5…45e526921a` | tiebreak: identical to run-1, 4105 bytes, no trailing byte |
| `run-3.console.log` | (transcript) | console transcript of run-3 |

(Full hashes: see `sha256sum` output recorded at snapshot time; abbreviated
here for readability.)
