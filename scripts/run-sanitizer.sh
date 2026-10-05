#!/usr/bin/env bash
# P6.3: run test suites under AddressSanitizer (+ nightly UB checks).
#
# Recipe notes (all proven on nightly-2026-10-01, 2026-10-04):
# - `-Zsanitizer=undefined` (UBSan) does NOT exist on the pinned nightly;
#   rustc rejects it. ASan carries the allocator/OOB/UAF load, while Miri
#   (6.2, 104 tests green) and `-Zub-checks=yes` cover alignment/validity
#   UB. CFI was considered and rejected (wrong property for Rust code).
# - Sanitizer flags must NOT reach proc-macro (host) crates: rustc refuses
#   to build them with sanitizers on. Hence `-Zhost-config` /
#   `-Ztarget-applies-to-host` with an empty host rustflags and the
#   sanitizer flags scoped to the target triple. A `RUSTFLAGS` env var
#   would leak to host builds, so it is refused (fail closed).
# - Results land in `target/sanitizer/` (override with CARGO_TARGET_DIR)
#   to keep instrumented artifacts out of the normal build cache.
#
# Usage: scripts/run-sanitizer.sh [cargo test args...]
#   e.g. scripts/run-sanitizer.sh -p boa_gc -p boa_string -p boa_engine --lib miri
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ -n "${RUSTFLAGS:-}" ]; then
    echo "error: RUSTFLAGS must be unset (it would leak sanitizer flags to proc-macro builds)" >&2
    exit 1
fi

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly-2026-10-01}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/sanitizer}"

exec cargo test \
    -Zhost-config \
    -Ztarget-applies-to-host \
    --config 'target-applies-to-host=false' \
    --config 'host.rustflags=[]' \
    --config 'target.x86_64-unknown-linux-gnu.rustflags=["-Zsanitizer=address","-Zub-checks=yes"]' \
    "$@"
