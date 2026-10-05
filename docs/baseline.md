# P0 Baseline Manifest

This file is the single source of truth for every pinned input the assurance
battery compares against. It was created by step-0 phase P0
(`Implementation_Plan.md`, P0.1) and is enforced by
[`scripts/verify-baseline.sh`](../scripts/verify-baseline.sh), which runs in CI
and fails the workflow on any drift.

Rule: **no measurement is comparable unless it was taken on these pins.** Any
change to a pin is a reviewed event that re-baselines the affected artifacts
(conformance snapshots, perf numbers, coverage) — never a drive-by edit. The
re-pin procedure for each input is at the bottom of this file.

## Pins

| Input | Pinned value | Verified how |
|---|---|---|
| Boa source (P0 baseline commit) | `39cd11214ecb366200157dab96b3919e61e9a391` | `git rev-parse HEAD` at snapshot time |
| Stable toolchain | `1.94.0` (`rustc 1.94.0 (4a4ef493e 2026-03-02)`, LLVM 21.1.8) | [`rust-toolchain.toml`](../rust-toolchain.toml) + `verify-baseline.sh`; channel manifest `channel-rust-1.94.0.toml` fetched 2026-10-01 |
| Nightly toolchain (Miri, fuzz runs, sanitizers) | `nightly-2026-10-01` (`1.101.0-nightly (21b707e3f 2026-09-30)`, ships `miri-preview` + `rust-src`) | Dated manifest `dist/2026-10-01/channel-rust-nightly.toml` fetched 2026-10-01; consumed via `RUSTUP_TOOLCHAIN` in CI (see below) |
| MSRV | `1.91.0` (from `rust-version` in `Cargo.toml`) | MSRV CI job reads `Cargo.toml` dynamically |
| Test262 commit | `d86b2294eb0a17eaa281ff12c73c473ec864c72f` (2026-08-24, "Escape test paths and engine names as well in CI table") | [`test262_config.toml`](../test262_config.toml) + `git -C test262 rev-parse HEAD` in CI after every run |
| WPT rev | `82a84e1842d583f1f197d64950453222b9f52c67` | [`test_wpt_config.toml`](../test_wpt_config.toml) |
| Dependency lockfile | `Cargo.lock`, format version 4, sha256 `85a7c93b…2e2da8a` (full hash in git history at the P0 commit) | `cargo metadata --locked` must pass; `git diff --exit-code -- Cargo.lock` must be clean in CI |
| `MIRIFLAGS` | `-Zmiri-tree-borrows` | [`.cargo/config.toml`](../.cargo/config.toml) |
| Sanitizer flags (P4/P6 runs) | `RUSTFLAGS="-Zsanitizer=address,undefined"` on the pinned nightly | Pinned string; jobs that use it assert the nightly pin first |
| Differential oracle engine (P3.1 v1) | SpiderMonkey jsshell `JavaScript-C128.14.0` (Firefox 128.14.0esr release, linux-x86_64) | Pin block below; `scripts/fetch-oracle.sh` fetches + verifies; `verify-baseline.sh` enforces version + binary hash |

### Why `RUSTUP_TOOLCHAIN` appears in CI

`rust-toolchain.toml` pins the stable toolchain for every command run inside the
repo directory — including jobs that must *not* run on stable. The two
exceptions override the file with the `RUSTUP_TOOLCHAIN` environment variable,
which takes precedence over toolchain files:

- the Miri job sets `RUSTUP_TOOLCHAIN: nightly-2026-10-01`,
- the MSRV job sets `RUSTUP_TOOLCHAIN: <rust-version from Cargo.toml>`.

`verify-baseline.sh` asserts these overrides exist in `.github/workflows/rust.yml`
so a refactor cannot silently run Miri or the MSRV check on the wrong toolchain.

## Feature matrix (what the baselines were measured with)

Different binaries build the engine with different features. A baseline number
is only comparable to a new number taken with the same row:

| Binary / suite | Feature set |
|---|---|
| `boa_tester` (Test262 baseline) | default: `boa_engine/intl_bundled`, `boa_engine/experimental`, `annex-b` (see `tests/tester/Cargo.toml`) |
| `boa` CLI | default incl. `native-backtrace`, `fast-allocator`, `fetch`, plus `deser`, `flowgraph`, `trace` (see `cli/Cargo.toml`) |
| `boa_benches` (perf baseline) | engine with `intl_bundled` on top of engine defaults `float16,xsum,temporal` (see `benches/Cargo.toml`) |
| Engine defaults | `float16,xsum,temporal` (see `core/engine/Cargo.toml`) |
| Fuzz targets | engine with `fuzz` (`boa_ast/arbitrary`, `boa_interner/arbitrary`) |

## Baseline artifacts produced on these pins

| Artifact | Location | Headline result |
|---|---|---|
| Test262 conformance snapshot (two full runs) | `test-results-baseline/run-1`, `run-2` (+ `MANIFEST.md`) | 53578 total, 51437 passed, 1648 ignored, 493 failed, 0 panics, 96.00% — both `latest.json` files byte-identical |
| Test262 full-run wall time | `test-results-baseline/MANIFEST.md`, `perf/baseline.json` | 122 s / 108 s / 98 s (bounds full-gate frequency) |
| Performance baseline | `perf/baseline.json` | Criterion means per `benches/scripts/**` on the pinned toolchain |
| Architecture inventory | `docs/architecture-inventory.md` | every crate + major module classified |
| Unsafe inventory | `docs/unsafe-inventory.json` (machine-readable) + `docs/unsafe-inventory.md` | every `unsafe` block/fn/impl under `core/*`, status `unjustified` by default (P6 audits) |
| Concurrency inventory | `docs/concurrency-inventory.md` | threads, atomics, locks, channels, `Send`/`Sync`; decides Loom scope in P6 |
| Host-boundary map | `docs/host-boundary.md` | `boa_runtime` vs `boa_wintertc`, CLI injections, untrusted-input surfaces |

## Oracle-engine pin (P3.1 v1)

Selected in P3.1 after head-to-head evaluation (see
`tools/differential/README.md` § "Oracle selection"): SpiderMonkey's `jsshell`
— fastest measured startup (~10 ms vs ~30 ms Node, ~20–60 ms Deno), cleanest
normalizable output (one error line + short stack, exit 3 on throw/syntax),
and a zero-build pin (official Mozilla release zip + published SHA256SUMS,
verified byte-identical at pin time). Node v26.10.0 is reserved as the P3.5
second oracle; Deno was dropped (variable startup, largest host surface).

```text
oracle.name          = "jsshell"   # SpiderMonkey headless shell
oracle.version       = "JavaScript-C128.14.0"
oracle.url           = "https://archive.mozilla.org/pub/firefox/releases/128.14.0esr/jsshell/jsshell-linux-x86_64.zip"
oracle.zip_sha256    = "dfe276d0966fd5597c11a1a283245e18a10a90f4becd93e61fe19f13fe0d269e"  # == published SHA256SUMS
oracle.binary_sha256 = "9cdfbda6940d10e213b7c045e78005ec4aecbb0390ba400bb257de649dc6da4b"  # extracted `js`
oracle.build_flags   = "official Mozilla release build: release_or_beta=true, debug=false, no sanitizers, intl-api=true, x64 (via getBuildConfiguration)"
oracle.source        = "mozilla-esr128 @ df0b4a4887880a1b267ca2b4902afee84e80f6e9 (FIREFOX_128_14_0esr_RELEASE)"
oracle.runner        = "tools/differential: run_oracle(binary, program, timeout)"
```

`scripts/fetch-oracle.sh` is the single source of fetch truth for these values;
`verify-baseline.sh` sources it and enforces version + binary hash on the
fetched copy before any differential run. Differential jobs must never run
against an unpinned engine: with no fetched oracle, verification reports
"oracle not fetched" and differential CI stays disabled.

## Re-pin procedures

### Stable toolchain

1. Pick the new version; verify its channel manifest exists:
   `curl -sI https://static.rust-lang.org/dist/channel-rust-<V>.toml`.
2. Update `channel` in `rust-toolchain.toml`, the `STABLE_PIN` in
   `scripts/verify-baseline.sh`, every `toolchain: <V>` in
   `.github/workflows/rust.yml` / `test262.yml` / `test262_full.yml` /
   `nightly_build.yml`, and this file.
3. Re-run the full P0 battery (conformance double-run + perf baseline) and
   commit the refreshed artifacts with the pin change.

### Nightly toolchain

1. Pick the new date; verify the dated manifest exists **and** lists
   `miri-preview` and `rust-src`:
   `curl -s https://static.rust-lang.org/dist/<date>/channel-rust-nightly.toml`.
2. Update `NIGHTLY_PIN` in `scripts/verify-baseline.sh`, the Miri job in
   `.github/workflows/rust.yml`, and this file.
3. Re-run Miri + one sanitizer smoke job before landing.

### Test262 commit

1. Update `commit` in `test262_config.toml` and `TEST262_PIN` in
   `scripts/verify-baseline.sh` and this file.
2. Delete `./test262` (or let the tester re-reset it), run
   `scripts/conformance-snapshot.sh`, and triage the `compare` delta against
   the previous baseline: every newly failing test needs a P2 work item or an
   audited ignore entry — a re-pin never silently moves the denominator.
3. Commit the refreshed `test-results-baseline/` with the pin change.

### WPT rev

Same shape as Test262: update `test_wpt_config.toml` + `WPT_PIN` + this file,
re-run `cargo test -p boa_wpt`, triage deltas, commit together.

### `Cargo.lock`

Dependency updates land via the normal Dependabot/review flow. The pin here is
not the hash (which legitimately changes) but the **discipline**: the lockfile
is committed, `cargo metadata --locked` passes, and routine test/bench runs
never modify it — `verify-baseline.sh` enforces all three.

### Differential oracle

1. Pick the new release; download the `jsshell-linux-x86_64.zip` from
   `archive.mozilla.org/pub/firefox/releases/<V>/jsshell/` and confirm its
   sha256 appears in that release's published `SHA256SUMS`.
2. Update the pin vars in `scripts/fetch-oracle.sh`, the pin block above,
   and the oracle record in `tools/differential/README.md` together.
3. Delete the fetched copy, re-run `scripts/fetch-oracle.sh`, then run the
   P3 oracle-upgrade drill: a full differential run where the only new
   mismatches are re-triaged oracle-quirks or newly-exposed Boa bugs —
   land the drill's re-triage with the pin change.
