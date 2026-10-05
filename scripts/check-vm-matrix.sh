#!/usr/bin/env bash
# P7.1b: VM opcode/operand/exception-path matrix gate.
#
# Runs the deterministic matrix feeds (10k battery programs plain +
# optimized, seeds, probes, Test262 static scan) and `--check`s them
# against the committed ratchet (`tests/fuzz/matrix/ratchet.json`):
# floors met, no new/stale cells, zero cells justified, deep cells
# present-but-absent-from-fresh. See `tests/fuzz/matrix/README.md`
# for the canonical commands this wraps (~2 min + one release build).
#
# Usage: scripts/check-vm-matrix.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/target/matrix-gate"
mkdir -p "$OUT"
cd "$ROOT/tests/fuzz"

run_matrix() {
    cargo run -q --release --example matrix -- "$@"
}

echo "matrix gate: deterministic battery (plain)..."
run_matrix "$OUT/det-battery.json" --programs 10000 --seeds --probes

echo "matrix gate: deterministic battery (optimized)..."
run_matrix "$OUT/det-battery-opt.json" --programs 10000 --seeds --probes --optimize

echo "matrix gate: Test262 static scan..."
find "$ROOT/test262/test" -name '*.js' | sort > "$OUT/t262-files.txt"
run_matrix "$OUT/det-static.json" --programs 0 --files-from "$OUT/t262-files.txt" --static-only

echo "matrix gate: --check against ratchet..."
run_matrix --check \
    --ratchet matrix/ratchet.json --justifications matrix/justifications.toml \
    --deep matrix/test262-dynamic.json \
    "$OUT/det-battery.json" "$OUT/det-battery-opt.json" "$OUT/det-static.json"

echo "vm matrix gate: floors met, no drift"
