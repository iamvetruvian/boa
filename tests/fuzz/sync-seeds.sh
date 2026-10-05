#!/usr/bin/env bash
# Sync the committed seed bank into the working corpora.
#
# Usage: ./sync-seeds.sh [target...]   (default: all eight targets)
#
# Copies `seeds/*.js` (regression reproducers, plan P4.1/P4.6) into
# `corpus/<target>/`. Encoding per target kind:
# - Byte-level targets (`source-bytes`, `cli-exec`) consume each seed
#   literally as a program: directed seeding of exact reproducers.
# - Structured targets take `Arbitrary` bytes, so a `.js` seed contributes
#   raw byte-diversity (deserializing to whatever its bytes encode, or
#   ignored when they don't decode) rather than its literal program.
# `seeds/` stays the committed bank; `corpus/` is gitignored working state.
set -euo pipefail

cd "$(dirname "$0")"

targets=("$@")
if [ "${#targets[@]}" -eq 0 ]; then
    targets=(parser-idempotency vm-implied bytecompiler-implied source-bytes module-specifier url-parse fetch-response cli-exec)
fi

shopt -s nullglob
seeds=(seeds/*.js)
if [ "${#seeds[@]}" -eq 0 ]; then
    echo "sync-seeds: no seeds/*.js found" >&2
    exit 1
fi

for target in "${targets[@]}"; do
    mkdir -p "corpus/$target"
    count=0
    for seed in "${seeds[@]}"; do
        cp "$seed" "corpus/$target/repro-$(basename "$seed").input"
        count=$((count + 1))
    done
    echo "sync-seeds: $count seeds -> corpus/$target/"
done
