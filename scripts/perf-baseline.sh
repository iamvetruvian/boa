#!/usr/bin/env bash
# Runs the full criterion suite and rolls the per-bench statistics up into
# `perf/baseline.json`, the P0 performance baseline.
#
# Usage:
#   scripts/perf-baseline.sh              # run benches, then collect
#   scripts/perf-baseline.sh --collect-only  # collect from target/criterion
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ "${1:-}" != "--collect-only" ]; then
  bash scripts/verify-baseline.sh
  echo "Running full criterion suite (pinned toolchain)..."
  start=$(date +%s)
  cargo bench -p boa_benches
  echo "bench wall time: $(( $(date +%s) - start ))s"
fi

mkdir -p perf
python3 - "$ROOT" <<'EOF'
import json, sys
from pathlib import Path

root = Path(sys.argv[1])
crit = root / "target" / "criterion"
benches = {}
for est in sorted(crit.rglob("Execution/new/estimates.json")):
    # Display name comes from benchmark.json: criterion sanitizes group
    # separators in directory names ("basic/call-loop" -> "basic_call-loop"),
    # so the path cannot be inverted reliably.
    meta = json.loads((est.parent / "benchmark.json").read_text())
    name = meta["group_id"]
    data = json.loads(est.read_text())
    benches[name] = {
        "mean_ns": data["mean"]["point_estimate"],
        "stddev_ns": data["std_dev"]["point_estimate"],
        "median_ns": data["median"]["point_estimate"],
    }

import subprocess, datetime
def git(*args):
    return subprocess.run(["git", *args], cwd=root, capture_output=True,
                          text=True, check=True).stdout.strip()

baseline = {
    "schema_version": 1,
    "generated_by": "scripts/perf-baseline.sh",
    "pins_ref": "docs/baseline.md",
    "date_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "boa_commit": git("rev-parse", "HEAD"),
    "toolchain": subprocess.run(["rustc", "--version"], capture_output=True,
                                text=True, check=True).stdout.strip(),
    "features": "boa_benches default (engine with intl_bundled + float16,xsum,temporal)",
    "comparison_procedure": (
        "cargo bench -p boa_benches on the pinned toolchain and same features; "
        "compare each bench mean_ns against this file; criterion also compares "
        "against its own previous run automatically"
    ),
    "regression_threshold_pct": 5.0,
    "smoke_set": {
        "command": "cargo bench -p boa_benches -- 'basic|closures'",
        "benches": sorted(n for n in benches if n.startswith("basic/") or n.startswith("closures/")),
    },
    "test262_full_run_wall_time_s": {"run-1": 122, "run-2": 108, "run-3": 98},
    "benches": benches,
}
out = root / "perf" / "baseline.json"
out.write_text(json.dumps(baseline, indent=2) + "\n")
print(f"wrote {out} with {len(benches)} benches")
EOF
