#!/usr/bin/env bash
# Crash gate runner (P4 validation gate): reintroduce five historical crash
# bugs, prove the fleet rediscovers all five with distinct signatures and
# minimizes each, restore byte-identical.
#
# Usage: ./crash-gate.sh
#
# Flow: pre-check patches -> snapshot -> apply breaking patches -> rebuild
# boa -> run.sh inventory-gate.toml (fleet binaries rebuild incrementally) ->
# restore patches -> verify the tree snapshot is identical -> rebuild (green).
#
# Safety mirrors logic/run.sh: refuses to start unless every patch applies
# cleanly; an EXIT trap always restores + rebuilds. Never commits.
set -euo pipefail

cd "$(dirname "$0")"
DRILL_DIR="$PWD"
REPO_ROOT="$(cd ../../.. && pwd)"
SNAPSHOT="$(mktemp)"
LOCK_SNAP="$(mktemp)"
APPLIED=0

# Full transcript (forensics for exactly this kind of gate incident).
exec > >(tee /tmp/crash-gate.log) 2>&1

tree_hash() {
    { git status --porcelain | sort; git diff; } | sha256sum | cut -d' ' -f1
}

cleanup() {
    cd "$REPO_ROOT"
    # Reverse order of application (same-file patches unwind safely), and
    # say which patch skips (a silent skip strands a broken tree).
    for patch in $(ls tests/fuzz/drill/crash-patches/*.break.patch | sort -r); do
        if git apply -R --check "$patch" >/dev/null 2>&1; then
            git apply -R "$patch"
        else
            echo "crash-gate restore: SKIP $patch (reverse does not apply)"
        fi
    done
    # The parser break-patch transiently removes a dependency; cargo rewrites
    # Cargo.lock on the next build and the rewrite-back is NOT byte-exact
    # (observed: the re-added edge drops a line), so restore lock bytes.
    cp "$LOCK_SNAP" "$REPO_ROOT/Cargo.lock"
    if [ "$(tree_hash)" != "$(cat "$SNAPSHOT")" ]; then
        echo "crash-gate RESTORE MISMATCH: tree differs from snapshot!" >&2
        git status --porcelain >&2
        exit 1
    fi
    if [ "$APPLIED" = "1" ]; then
        echo "crash-gate: tree restored byte-identical; rebuilding..."
        cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p boa_cli 2>&1 | tail -1
    fi
    rm -f "$SNAPSHOT" "$LOCK_SNAP"
}
trap cleanup EXIT

tree_hash > "$SNAPSHOT"
cp "$REPO_ROOT/Cargo.lock" "$LOCK_SNAP"

echo "crash-gate: pre-checking patches apply cleanly..."
for patch in crash-patches/*.break.patch; do
    git -C "$REPO_ROOT" apply --check "$DRILL_DIR/$patch" || {
        echo "crash-gate: patch does not apply (tree drifted?): $patch" >&2
        exit 1
    }
done

echo "crash-gate: applying breaking patches..."
for patch in crash-patches/*.break.patch; do
    git -C "$REPO_ROOT" apply "$DRILL_DIR/$patch"
done
APPLIED=1

echo "crash-gate: rebuilding broken boa (boa-eval entries)..."
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p boa_cli 2>&1 | tail -1

echo "crash-gate: running seeded rediscovery (fleet binaries rebuild as needed)..."
gate_status=0
./run.sh inventory-gate.toml || gate_status=$?

cleanup
trap - EXIT
if [ "$gate_status" != "0" ]; then
    echo "crash-gate: INCOMPLETE — see drill output above" >&2
    exit 1
fi
echo "crash-gate: 5/5 historical crashers rediscovered, distinct, minimized."
