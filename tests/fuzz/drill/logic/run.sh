#!/usr/bin/env bash
# Logic drill runner (P4 validation gate): reintroduce three landed logic
# bugs, prove the semantic oracles flag all three, restore byte-identical.
#
# Usage: ./run.sh
#
# Flow: sanity (probes agree on the fixed tree) -> apply breaking patches ->
# rebuild boa -> probes must DIVERGE (Boa vs oracle) -> restore patches ->
# verify the tree snapshot is identical -> rebuild (leave green).
#
# Safety: refuses to start unless every patch applies cleanly (`--check`);
# an EXIT trap always restores + rebuilds, so interrupts cannot strand a
# broken tree. Never commits. Fails closed on probe rot (sanity disagreement
# stops the drill before any mutation).
set -euo pipefail

cd "$(dirname "$0")"
LOGIC_DIR="$PWD"
REPO_ROOT="$(cd ../../../.. && pwd)"
SNAPSHOT="$(mktemp)"
APPLIED=0

tree_hash() {
    { git status --porcelain | sort; git diff; } | sha256sum | cut -d' ' -f1
}

cleanup() {
    cd "$REPO_ROOT"
    for patch in tests/fuzz/drill/logic/patches/*.break.patch; do
        git apply -R --check "$patch" >/dev/null 2>&1 && git apply -R "$patch" || true
    done
    if [ "$(tree_hash)" != "$(cat "$SNAPSHOT")" ]; then
        echo "drill-logic RESTORE MISMATCH: tree differs from snapshot!" >&2
        git status --porcelain >&2
        exit 1
    fi
    if [ "$APPLIED" = "1" ]; then
        echo "drill-logic: tree restored byte-identical; rebuilding..."
        cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p boa_cli 2>&1 | tail -1
    fi
    rm -f "$SNAPSHOT"
}
trap cleanup EXIT

tree_hash > "$SNAPSHOT"

echo "drill-logic: pre-checking patches apply cleanly..."
for patch in patches/*.break.patch; do
    git -C "$REPO_ROOT" apply --check "$LOGIC_DIR/$patch" || {
        echo "drill-logic: patch does not apply (tree drifted?): $patch" >&2
        exit 1
    }
done

BOA="$REPO_ROOT/target/debug/boa"
JSSHELL="$REPO_ROOT/target/oracle/js"
[ -x "$BOA" ] || { echo "drill-logic: build target/debug/boa first" >&2; exit 1; }
[ -x "$JSSHELL" ] || { echo "drill-logic: oracle missing at $JSSHELL" >&2; exit 1; }

# Compare normalized outcomes: exit-zero-ness + stdout + first error class.
compare() {
    python3 - "$BOA" "$JSSHELL" "$1" <<'EOF'
import re, subprocess, sys
boa, jsshell, probe = sys.argv[1], sys.argv[2], sys.argv[3]

def run(path, args):
    try:
        proc = subprocess.run([path, *args], capture_output=True, text=True, timeout=30)
        out = (proc.stdout or "") + (proc.stderr or "")
    except subprocess.TimeoutExpired:
        return ("timeout", "", "")
    cls = next(
        (m.group(1) for m in re.finditer(r"(Syntax|Type|Reference|Range|URI|Uri|Eval|Aggregate)Error", out)),
        "",
    )
    stdout = "\n".join((proc.stdout or "").splitlines())
    return ("exit0" if proc.returncode == 0 else "threw", stdout, cls)

a = run(boa, [probe])
b = run(jsshell, ["-f", probe])
print(f"boa={a[0]}:{a[1][:60]}:{a[2]} oracle={b[0]}:{b[1][:60]}:{b[2]}")
sys.exit(0 if a == b else 10)
EOF
}

echo "drill-logic: sanity — probes must AGREE on the fixed tree..."
for probe in probes/*.js; do
    if ! compare "$LOGIC_DIR/$probe" > /tmp/drill-sanity.txt 2>&1; then
        echo "drill-logic SANITY FAIL (probe rot?): $probe" >&2
        cat /tmp/drill-sanity.txt >&2
        exit 1
    fi
    echo "  agree: $probe ($(cat /tmp/drill-sanity.txt))"
done

echo "drill-logic: applying breaking patches..."
for patch in patches/*.break.patch; do
    git -C "$REPO_ROOT" apply "$LOGIC_DIR/$patch"
done
APPLIED=1

echo "drill-logic: rebuilding broken boa..."
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p boa_cli 2>&1 | tail -1

echo "drill-logic: probes must DIVERGE on the broken tree..."
failed=0
for probe in probes/*.js; do
    if compare "$LOGIC_DIR/$probe" > /tmp/drill-broken.txt 2>&1; then
        echo "  MISS (no divergence!): $probe" >&2
        cat /tmp/drill-broken.txt >&2
        failed=1
    else
        echo "  FLAGGED: $probe ($(cat /tmp/drill-broken.txt))"
    fi
done

cleanup
trap - EXIT
if [ "$failed" != "0" ]; then
    echo "drill-logic: INCOMPLETE — a reverted bug escaped the oracles" >&2
    exit 1
fi
echo "drill-logic: 3/3 reverted logic bugs flagged; tree restored."
