# Differential testing (`boa_differential`)

P3 pipeline: run the same programs on Boa and one pinned independent oracle
engine, normalize observable behavior, and triage every mismatch against the
spec. This crate owns the Boa side (in-process `boa_engine`), the oracle side
(subprocess), the normalizer, the seed corpora manifests, and the triage queue.

## Oracle selection (P3.1)

v1 oracle: **SpiderMonkey `jsshell`, `JavaScript-C128.14.0`** (Firefox
128.14.0esr release, linux-x86_64). Pin facts live in
[`docs/baseline.md`](../../docs/baseline.md) § "Oracle-engine pin"; fetch with
`scripts/fetch-oracle.sh` (verifies zip + binary hashes, idempotent).

Head-to-head evaluation (all commands run headless, single trivial program,
3 repetitions, October 2026):

| Candidate | Version | Startup | Pin cost | Output normalizability | Verdict |
|---|---|---|---|---|---|
| `jsshell` (SpiderMonkey) | JavaScript-C128.14.0 | ~10 ms | zero-build: official Mozilla release zip + published SHA256SUMS (verified byte-identical) | best: one error line + short stack on stderr, exit 3 on throw/syntax, silent on success | **v1 oracle** |
| `node` (V8) | v26.10.0 | ~30 ms | low: nodejs.org tarballs + SHASUMS | noisier: internal loader frames + version footer on stderr, exit 1 | reserved for P3.5 second oracle |
| `deno` (V8) | 2.9.7 | ~20–60 ms, variable | medium: release zips, no versioned checksum chain as direct | largest host surface (permissions, TS) | dropped |
| `d8` / `jsc` | — | — | build-from-source | — | not evaluated further once jsshell won on all three axes |

Observed oracle behavior the normalizer relies on (all verified against the
pinned binary):

- success: exit 0, completion value NOT printed, `print()` lines on stdout.
- thrown error: exit 3, `<file>:<line>:<col> <ErrorName>: <message>` on
  stderr followed by a `Stack:` frame list.
- syntax error: exit 3, `SyntaxError: ...` + source caret lines on stderr.
- `getBuildConfiguration()`: release build, no sanitizers, `intl-api: true`.

## Runner contract (adding oracles is mechanical)

Every oracle adapter implements exactly this surface (see `src/oracle.rs`).
Adding a second oracle (P3.5) means adding one adapter file + one row in the
oracle table below — the runner, normalizer, corpora, and queue are shared.

```text
run_oracle(binary, program, timeout) -> OracleOutcome { exit_code, stdout, stderr }
```

- `binary`: absolute path to the pinned headless shell.
- `program`: UTF-8 source text; the adapter chooses file vs `-e` (jsshell:
  temp file, so caret diagnostics carry a stable filename).
- `timeout`: per-case wall-clock budget in seconds, identical to the Boa
  side; enforced by the shared subprocess supervisor (P1.2 mechanics:
  concurrent pipe drains, kill on expiry, signal-death vs nonzero-exit
  classification). The adapter never invents its own timeout path.
- `OracleOutcome`: raw `exit_code: Option<i32>` (`None` = killed), bounded
  `stdout`/`stderr` (64 KiB each, matching the tester). Parsing raw output
  into verdicts is the normalizer's job, not the adapter's.

An adapter additionally declares:

```text
oracle.name           # "jsshell" | "node" | ...
oracle.version_args   # ["--version"] (checked against the pin at startup)
oracle.success_exit   # [0]
oracle.error_class    # exit_code -> {threw, syntax} hint (normalizer confirms from stderr)
```

| Oracle | Status | Adapter | Pin |
|---|---|---|---|
| jsshell 128.14.0esr | v1, gated in CI | `src/oracle.rs::Jsshell` | `docs/baseline.md` |
| node 26.10.0 | P3.5 recipe demo (nightly-only) | `src/oracle.rs::Node` | `docs/baseline.md` (recorded at demo time) |

## Pipeline layout

```text
corpora/ci.toml        # fast CI corpus (one Test262 slice + regression + adversarial)
corpora/full.toml      # full green-slice corpus (all suites, weekly frequency)
corpora/adversarial/   # curated adversarial set (P0 choke points + P2 seams)
corpora/verdicts.toml  # committed verdicts pre-applied to identical signatures
src/boa_side.rs        # Boa driver behind the supervised run-case child
src/oracle.rs          # subprocess adapters implementing run_oracle
src/compose.rs         # identical-both-sides driver composition
src/capture.rs         # marker parsing into per-side observations
src/normalize.rs       # explicit normalization table (every rule has fixtures)
src/supervise.rs       # P1.2-mechanics subprocess supervisor (both sides)
src/corpus.rs          # manifest loading + exclusion accounting
src/reduce.rs          # line-based mismatch minimizer (P4 tmin replaces the hook)
src/queue.rs           # triage-queue store (P1.1 conventions) + verdicts
src/main.rs            # `run` / `run-case` / `triage` subcommands
```

- `boa_differential run --corpus corpora/ci.toml --oracle <js>
  --expected-version <pin> --verdicts corpora/verdicts.toml --output <dir>`
  executes every case on both sides and writes `diff-results.json` (P1.1
  store conventions) plus `triage-queue.json` for mismatches, carrying
  verdicts forward across identical diffs and pre-applying committed ones.
- `boa_differential triage --queue <file> --check` breaches (exit 1) on any
  mismatch without a verdict — the zero-untriaged gate.
- `triage --queue <file> --set-verdict <kind> --area/--pattern/--fields …`
  verdicts entries in bulk (`--fields` matches the exact field-diff set, so
  mismatch clusters triage precisely); `--export corpora/verdicts.toml`
  commits them. When a run reports stale committed verdicts (no longer
  mismatching), prune them from `verdicts.toml` via a fresh `--export`.
- `#strict` corpus variants compare completion/kind/console only: strict eval
  code binds `var`/functions in a fresh declarative environment, so strict
  state dumps are always empty on both sides (verified symmetric).

## Normalization rules (summary; fixtures in `src/normalize.rs` tests)

Compared after normalization: completion flag | thrown kind | value rendering
| console lines (ordered) | state bindings (sorted) + enumeration order |
stderr. Erased by explicit rule only:

- R1/R1b: error *message* text (kinds compare; thrown native errors
  canonicalize to `throw:<Kind>`).
- R2: stack traces, caret lines, `file:line:col` prefixes (first stderr line
  still compares — stderr is never silently ignored).
- R3: bare 13-digit timestamp lines (full-string matches only).
- R5: function source (names compare).
- R7/R8: console order significant; state bindings sorted at capture.
- R10: `globalThis`-identical values canonicalize (host-defined `toStringTag`).

Every rule ships fixtures proving what it erases and what it can never mask.
Deliberately absent: kind aliasing (exact match) and `undefined`-completion
unification (the driver always emits markers, so both sides are symmetric).

## Triage verdicts (P3.4)

Every mismatch closes with exactly one spec-anchored verdict — majority vote
is never a verdict:

- `boa-bug <spec-section + regression-id>` → regression-DB entry + fix loop.
- `oracle-quirk <pin + note>` → documented, re-checked on oracle upgrades
  (`triage --reset-oracle-quirks` re-opens them for the upgrade drill).
- `spec-ambiguity <spec-section + filing + expiry>` → upstream filing where
  appropriate, audited exception with expiry.
