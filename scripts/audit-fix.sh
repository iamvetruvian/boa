#!/usr/bin/env bash
# Audits the four P2 fix-loop artifacts for one regression-DB entry:
# reproducer, responsible-layer unit test, Test262-style test (or written
# justification), and fuzz seed. Exits nonzero unless all four exist and
# the whole regression database is green (which proves the reproducer).
#
# Usage:
#   scripts/audit-fix.sh <entry-id>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ $# -ne 1 ]; then
  echo "usage: scripts/audit-fix.sh <entry-id>" >&2
  exit 2
fi
ID="$1"

python3 - "$ID" <<'EOF'
import os
import sys
import tomllib

entry_id = sys.argv[1]
with open("tests/regression/regressions.toml", "rb") as handle:
    manifest = tomllib.load(handle)
entries = [e for e in manifest["entries"] if e["id"] == entry_id]
if not entries:
    print(f"FAIL: no entry `{entry_id}` in regressions.toml")
    sys.exit(1)
entry = entries[0]
failures = 0

def check(label, ok, detail):
    global failures
    if ok:
        print(f"ok: {label}: {detail}")
    else:
        print(f"FAIL: {label}: {detail}")
        failures += 1

repro = os.path.join("tests/regression", entry["reproducer"])
check("reproducer", os.path.isfile(repro), repro)

unit_file = entry["unit_test"].split("::")[0]
check("unit test", os.path.isfile(unit_file), entry["unit_test"])

link = (entry.get("test262_style") or "").strip()
reason = (entry.get("no_test262_reason") or "").strip()
if link:
    target = os.path.join("test262", link)
    # The checkout may be absent locally; the DB test enforces the link
    # when it exists, so here presence of the field suffices unless the
    # checkout is there to check against.
    ok = (not os.path.isdir("test262")) or os.path.exists(target)
    check("test262-style test", ok, link)
elif reason:
    check("test262 justification", True, reason[:80])
else:
    check("test262-style test or justification", False, "neither field set")

seed = (entry.get("fuzz_seed") or "").strip()
check("fuzz seed", bool(seed) and os.path.isfile(seed), seed or "(missing)")

sys.exit(1 if failures else 0)
EOF

echo "--- regression database ---"
out="$(mktemp)"
if cargo test -q -p boa_regression >"$out" 2>&1; then
  grep -E "test result" "$out" || true
  rm -f "$out"
  echo "audit-fix.sh: entry \`$ID\` has all four artifacts and the DB is green"
else
  cat "$out"
  rm -f "$out"
  exit 1
fi
