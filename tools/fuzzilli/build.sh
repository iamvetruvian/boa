#!/usr/bin/env bash
# Builds the REPRL adapter with SanitizerCoverage edge instrumentation.
#
# Usage: ./build.sh   (from tools/fuzzilli; needs pinned nightly)
#
# Recipe (validated on nightly-2026-10-01):
# - `-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=3
#   -Cllvm-args=-sanitizer-coverage-trace-pc-guard`: edge guards calling the
#   stub in src/cov.rs. The LEVEL flag is load-bearing (without it LLVM
#   silently emits no guards); level 3 stays clear of trace-pc-indir, which
#   would need a second runtime symbol (level 4 pulls it in).
# - Flags travel via `--config target.*.rustflags` with
#   `-Ztarget-applies-to-host --config target-applies-to-host=false`, so
#   build scripts and proc macros (host artifacts) stay uninstrumented:
#   they cannot link the stub and would otherwise fail the build.
# - `debug-assertions` + `overflow-checks` on top of release: Fuzzilli's
#   "debug parameters" (assertions stay live in the fuzzed build).
# - Separate target dir (`target/sancov`): sancov fingerprints would
#   otherwise fight normal release builds over `target/release`.
# - `profile.release.debug=1`: line info for triaging found crashes.
set -euo pipefail

cd "$(dirname "$0")/../.."

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly-2026-10-01}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/sancov}"

cargo build --release -p boa_fuzzilli \
    -Ztarget-applies-to-host \
    --config 'target-applies-to-host=false' \
    --config 'profile.release.debug=1' \
    --config 'target.x86_64-unknown-linux-gnu.rustflags=["-Cpasses=sancov-module", "-Cllvm-args=-sanitizer-coverage-level=3", "-Cllvm-args=-sanitizer-coverage-trace-pc-guard", "-Cllvm-args=-sanitizer-coverage-prune-blocks=0", "-Cdebug-assertions=on", "-Coverflow-checks=on"]'

BIN="$CARGO_TARGET_DIR/release/boa-reprl"
# Behavioral proof the flags bit: instrumented modules call the stub's init
# at startup, which logs each guard range. No [COV] lines, no instrumentation.
# Capture-then-grep (never `| grep -q` under pipefail: grep's early exit
# SIGPIPEs the binary, whose println! then panics with exit 101 — a false
# alarm that always triggers since [COV] precedes the completed line).
probe_out="$("$BIN" --script /dev/null 2>&1)"
if ! grep -q "\[COV\] registered guard range" <<<"$probe_out"; then
    echo "build.sh: no coverage init observed from $BIN (flags silently dropped?)" >&2
    echo "--- probe output ---" >&2
    echo "$probe_out" >&2
    exit 1
fi
echo "build.sh: ok: $BIN (instrumentation verified live)"
