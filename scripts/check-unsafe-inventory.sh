#!/usr/bin/env bash
# P6.1: fail if the committed unsafe inventory is stale or incompletely audited.
#
# The inventory is fully derived: `generate-unsafe-inventory.py` produces the
# site shape, `apply-unsafe-audit.py` attaches the reviewed justifications
# (and exits nonzero unless coverage is 100%). Any edit that adds, removes, or
# moves an `unsafe` site (or a Trace-bypass attribute) must regenerate the
# committed file:
#
#   python3 scripts/generate-unsafe-inventory.py core > /tmp/fresh.json
#   python3 scripts/apply-unsafe-audit.py /tmp/fresh.json docs/unsafe-inventory.json
#
# (Manual edits to docs/unsafe-inventory.json are forbidden; it must always be
# the output of the pipeline above.)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 "$ROOT/scripts/generate-unsafe-inventory.py" core > "$TMP/fresh.json"
python3 "$ROOT/scripts/apply-unsafe-audit.py" "$TMP/fresh.json" "$TMP/audited.json"

if ! diff -q "$TMP/audited.json" "$ROOT/docs/unsafe-inventory.json" > /dev/null; then
    echo "error: docs/unsafe-inventory.json is stale; regenerate it with the pipeline above" >&2
    diff "$ROOT/docs/unsafe-inventory.json" "$TMP/audited.json" | head -20 >&2 || true
    exit 1
fi

echo "unsafe inventory current and fully audited"
