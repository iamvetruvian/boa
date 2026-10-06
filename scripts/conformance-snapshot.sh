#!/usr/bin/env bash
# Reproduces the P0 conformance snapshot: builds the pinned tester, runs the
# full pinned Test262 corpus twice into fresh directories, and checks that the
# two verdict files are byte-identical.
#
# Usage:
#   scripts/conformance-snapshot.sh [output-dir]
# Default output-dir: ./test-results-baseline
set -euo pipefail

OUT="${1:-./test-results-baseline}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bash scripts/verify-baseline.sh --test262-only

echo "Building pinned boa_tester (release)..."
# NOTE: `-p` is load-bearing, not style. A bare `--bin` from the workspace
# root unifies features across default members; since tools/fuzzilli joined
# the workspace that silently enables boa_engine/fuzz+verify-bytecode and the
# resulting tester fails every test with NoInstructionsRemain (budget 0).
# Always scope bin builds/runs with `-p` (see step-0 notes).
cargo build --release -p boa_tester --bin boa_tester

for run in run-1 run-2; do
  rm -rf "$OUT/$run"
  mkdir -p "$OUT/$run"
  echo "Starting full Test262 run ($run) at $(date -u +%FT%TZ)..."
  start=$(date +%s)
  ./target/release/boa_tester run -v -o "$OUT/$run" 2>&1 | tee "$OUT/$run.console.log"
  echo "$run wall time: $(( $(date +%s) - start ))s"
done

echo "Checking determinism..."
if cmp -s "$OUT/run-1/latest.json" "$OUT/run-2/latest.json"; then
  echo "DETERMINISTIC: run-1/latest.json and run-2/latest.json are byte-identical"
else
  echo "DIVERGENT: verdict files differ (see cmp/diff output):" >&2
  cmp "$OUT/run-1/latest.json" "$OUT/run-2/latest.json" || true
  exit 1
fi
if cmp -s "$OUT/run-1/results.json" "$OUT/run-2/results.json"; then
  echo "DETERMINISTIC: results.json files are byte-identical"
else
  echo "NOTE: results.json files differ (informational history file)" >&2
fi
if cmp -s "$OUT/run-1/features.json" "$OUT/run-2/features.json"; then
  echo "DETERMINISTIC: features.json files are byte-identical"
else
  echo "NOTE: features.json files differ (informational feature-set file)" >&2
fi
echo "Snapshot complete in $OUT"
