#!/usr/bin/env python3
"""P7.1c: semantic-feature set gate (suite-drift detector).

Compares the feature-name set from a fresh `boa_tester` run
(`features.json`, last entry) against the committed baseline. The set is
corpus-derived (union over the discovered suite, outcome-independent), so
this gate detects suite/pin/config drift — accidental subsetting, pin
moves, ignore-file changes that drop tests — not engine regressions
(those are caught by `compare --fail-on` and the panic trend).

Fails on any lost feature or any suite/test262-commit mismatch; gained
features are reported for ratchet-up. Baseline:
`docs/semantic-features-baseline.json`, adopted verbatim from
`test-results-baseline/run-1/features.json` (P0.2 full pinned runs x3,
identical sets).

Usage: scripts/check-semantic-features.py <fresh-features.json> [baseline.json]
"""
import json
import sys


def load_last(path):
    with open(path) as f:
        entries = json.load(f)
    if not entries:
        raise SystemExit(f"error: {path} has no entries")
    return entries[-1]


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    baseline_path = argv[2] if len(argv) == 3 else "docs/semantic-features-baseline.json"
    fresh = load_last(argv[1])
    base = load_last(baseline_path)
    failed = False
    for key, label in (("n", "suite"), ("u", "test262 commit")):
        if fresh.get(key) != base.get(key):
            print(f"error: {label} drifted: {base.get(key)!r} -> {fresh.get(key)!r}")
            failed = True
    base_f, fresh_f = set(base.get("f", [])), set(fresh.get("f", []))
    lost = sorted(base_f - fresh_f)
    gained = sorted(fresh_f - base_f)
    print(f"features: baseline {len(base_f)}, fresh {len(fresh_f)}")
    if lost:
        print(f"error: {len(lost)} lost features: {', '.join(lost[:10])}"
              + (" ..." if len(lost) > 10 else ""))
        failed = True
    if gained:
        print(f"note: {len(gained)} gained features (ratchet up): "
              f"{', '.join(gained[:10])}" + (" ..." if len(gained) > 10 else ""))
    if failed:
        print("semantic gate: DRIFT", file=sys.stderr)
        return 1
    print("semantic gate: feature set stable")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
