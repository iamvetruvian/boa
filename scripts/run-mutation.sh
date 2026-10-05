#!/usr/bin/env bash
# P7.2: durable cargo-mutants wrapper. Enforces the methodology the pilot
# proved (2026-10-04):
#
# - P0-pinned toolchain guard (mutant diffs drift across toolchains).
# - Scratch + thread throttle (OOM-kill proven at -j4/default threads on a
#   15GB box; -j2 + RUST_TEST_THREADS=2 peaks ~4GB).
# - Two-stage verdicts: stage 1 runs the mutated package's own suite (cheap);
#   stage 2 retests stage-1 survivors against wider suites (leaf-crate
#   behavior is mostly tested upstream — e.g. boa_string via boa_engine).
# - Kill-threshold gate with a machine-checked survivor file.
#
# Usage:
#   scripts/run-mutation.sh [-p <pkg>]... [-o <dir>] [--jobs N]
#       [--test-package <pkg>]... [--filter <regex>] [--threshold R]
#       [--stage2-from <missed.txt>] [--no-gate] [--merge <out.json> <dir>...]
#
#   --stage2-from builds an exact-match filter from a prior missed.txt and
#   retests exactly those mutants (no over-approximation: same-span mutants
#   with mixed outcomes exist, so file:line prefixes are insufficient).
#   --merge SUPERSEDES batch/stage outcomes.json files into one verdict keyed
#   by mutant name (later dirs win): merging stage-1 + stage-2 yields the true
#   verdict (retested mutants take their stage-2 outcome, no double count).
#   --no-gate runs without the threshold gate (for stage-1 legs whose verdict
#   is only meaningful after the merge).
#   Per-PR sampling: generate the diff (`git diff origin/main...HEAD -- core/
#   > /tmp/pr.diff`) and pass MUTANTS_EXTRA_ARGS="--in-diff /tmp/pr.diff"
#   (--in-diff takes a diff FILE, not a ref; sampled runs gate with
#   --threshold 0, i.e. full accounting without a rate bar on tiny samples).
#   Rotation shards: MUTANTS_EXTRA_ARGS="--shard S/K" (deterministic slices).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT=""; JOBS=2; THRESHOLD="0.95"; STAGE2_FROM=""; MERGE_OUT=""; NO_GATE=0
PKGS=(); TEST_PKGS=(); FILTER=""

while [ $# -gt 0 ]; do
    case "$1" in
        -p) PKGS+=("$2"); shift 2 ;;
        -o) OUT="$2"; shift 2 ;;
        --jobs) JOBS="$2"; shift 2 ;;
        --threshold) THRESHOLD="$2"; shift 2 ;;
        --test-package) TEST_PKGS+=("$2"); shift 2 ;;
        --filter) FILTER="$2"; shift 2 ;;
        --stage2-from) STAGE2_FROM="$2"; shift 2 ;;
        --no-gate) NO_GATE=1; shift ;;
        --merge) MERGE_OUT="$2"; shift 2; break ;; # dirs follow; put other flags before --merge
        *) echo "error: unknown arg $1" >&2; exit 1 ;;
    esac
done

# --- merge mode: concat batch outcomes into one outcomes.json-shaped verdict,
# then fall through to the same threshold gate below ---
GATE_INPUT=""
if [ -n "$MERGE_OUT" ]; then
    python3 - "$MERGE_OUT" "$@" <<'EOF'
import json, sys
out_path, dirs = sys.argv[1], sys.argv[2:]
by_name, baselines = {}, 0
for d in dirs:
    o = json.load(open(f"{d}/mutants.out/outcomes.json"))
    for r in o["outcomes"]:
        if r.get("scenario") == "Baseline":
            baselines += 1
            continue
        # Supersede by exact mutant name: later dirs (stage-2, reruns) win.
        by_name[r["scenario"]["Mutant"]["name"]] = r
merged = list(by_name.values())
counts = {"missed": 0, "caught": 0, "timeout": 0, "unviable": 0}
summaries = {"MissedMutant": "missed", "CaughtMutant": "caught",
             "Timeout": "timeout", "Unviable": "unviable"}
for r in merged:
    key = summaries.get(r.get("summary", ""))
    if key:
        counts[key] += 1
json.dump({"outcomes": merged, "total_mutants": len(merged), **counts},
          open(out_path, "w"))
print(f"merged: {len(merged)} tested, {counts} ({baselines} baselines skipped)")
EOF
    GATE_INPUT="$MERGE_OUT"
fi

if [ -z "$GATE_INPUT" ]; then
# -p optional (repeatable): without it the whole workspace is in scope and
# MUTANTS_EXTRA_ARGS (--in-diff/--shard) does the scoping (per-PR mode).
[ -n "$OUT" ] || OUT="$ROOT/target/mutants-run"

# --- 1. pinned toolchain guard ---
if ! rustc --version 2>/dev/null | grep -q '1\.94\.0'; then
    echo "error: mutation runs require the P0-pinned 1.94.0 toolchain (got: $(rustc --version))" >&2
    exit 1
fi

# --- 2. resource throttle (measured; see header) ---
export TMPDIR="${TMPDIR:-$ROOT/target/mutants-tmp}"
mkdir -p "$TMPDIR"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-2}"

ARGS=(-j "$JOBS" --no-times -o "$OUT")
for p in "${PKGS[@]:-}"; do
    [ -n "$p" ] && ARGS+=(-p "$p")
done
for t in "${TEST_PKGS[@]:-}"; do
    [ -n "$t" ] && ARGS+=(--test-package "$t")
done

# --- 3. stage-2 exact filter from a prior missed.txt ---
if [ -n "$STAGE2_FROM" ] && [[ "${MUTANTS_EXTRA_ARGS:-}" == *"--shard"* ]]; then
    echo "error: --stage2-from must not combine with --shard (the shard would slice the survivor set)" >&2
    exit 1
fi
if [ -n "$STAGE2_FROM" ]; then
    FILTER="$(python3 - "$STAGE2_FROM" <<'EOF'
import re, sys
lines = [l.rstrip("\n") for l in open(sys.argv[1]) if l.strip()]
print("^(" + "|".join(re.escape(l) for l in lines) + ")$")
EOF
)"
    echo "stage-2 filter: $(grep -c . "$STAGE2_FROM") survivors (exact match)"
fi
[ -n "$FILTER" ] && ARGS+=(-F "$FILTER")

# shellcheck disable=SC2086
(cd "$ROOT" && cargo mutants "${ARGS[@]}" ${MUTANTS_EXTRA_ARGS:-})
GATE_INPUT="$OUT/mutants.out/outcomes.json"
fi # end run mode (merge mode set GATE_INPUT above)

if [ "$NO_GATE" = 1 ]; then
    echo "(--no-gate: verdict deferred to the merge)"
    exit 0
fi

echo "--- threshold gate (kill_rate >= $THRESHOLD, zero unaccounted miss/timeout) ---"
python3 - "$GATE_INPUT" "$THRESHOLD" "$ROOT/docs/mutation-survivors.md" <<'EOF'
import json, sys
outcomes_path, threshold, survivors_path = sys.argv[1], float(sys.argv[2]), sys.argv[3]
o = json.load(open(outcomes_path))
# Recompute from records (robust to merges): skip baselines, classify by
# the cargo-mutants 27.1.0 summary strings (verified against real output).
tested = caught = missed = timeout = unviable = killed_timeout = 0
unaccounted = []
try:
    survivors = open(survivors_path).read().splitlines()
except FileNotFoundError:
    survivors = []
for r in o["outcomes"]:
    if r.get("scenario") == "Baseline":
        continue
    tested += 1
    summary = r.get("summary", "")
    name = r.get("scenario", {}).get("Mutant", {}).get("name", "")
    if summary == "CaughtMutant":
        caught += 1
    elif summary == "Unviable":
        unviable += 1
    elif summary in ("MissedMutant", "Timeout"):
        # Machine-checked justification: the exact mutant name must appear as
        # a `JUSTIFIED: <name>` line in docs/mutation-survivors.md.
        justified = f"JUSTIFIED: {name}" in survivors
        if summary == "MissedMutant":
            missed += 1
        else:
            timeout += 1
            # A triaged deterministic-hang timeout is a kill (the suite did
            # not pass); an untriaged one counts as unkilled. See
            # docs/mutation-survivors.md for the triage bar.
            if justified:
                killed_timeout += 1
        if not justified:
            unaccounted.append((summary, name))
    else:
        print(f"warning: unknown summary {summary!r} for {name}")
viable = tested - unviable
rate = (caught + killed_timeout) / viable if viable else 0.0
print(f"tested={tested} caught={caught} missed={missed} "
      f"timeout={timeout} (triaged-killed={killed_timeout}) "
      f"unviable={unviable} kill_rate={rate:.4f}")
for outcome, name in unaccounted:
    print(f"UNACCOUNTED {outcome}: {name}")
fails = []
if rate < threshold:
    fails.append(f"kill_rate {rate:.4f} < {threshold}")
if unaccounted:
    fails.append(f"{len(unaccounted)} unaccounted miss/timeout (justify in {survivors_path})")
if fails:
    print("THRESHOLD FAIL: " + "; ".join(fails))
    sys.exit(1)
print("THRESHOLD PASS")
EOF
