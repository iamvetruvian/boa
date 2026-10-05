#!/usr/bin/env bash
# Prints the Test262 `-s` prefixes affected by the working tree changes.
#
# Usage:
#   scripts/affected-subsets.sh [<base-ref>]
#
# Compares `<base-ref>` (default: `origin/main`) against HEAD and maps every
# changed path to the Test262 subtree that covers it. Prints one `-s`
# argument per line, or `test` (the full corpus) when an S0-wide path
# changed, the mapping misses, or the change is too broad to subset.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BASE="${1:-origin/main}"

files="$({
  git diff --name-only "$BASE"...HEAD 2>/dev/null || true
  # Union with uncommitted work so local runs on a dirty tree are correct
  # too (CI checkouts are clean, so these add nothing there).
  git diff --name-only 2>/dev/null || true
  git diff --name-only --cached 2>/dev/null || true
  git ls-files --others --exclude-standard 2>/dev/null || true
} | sort -u)"
if [ -z "$files" ]; then
  # Unknown base (shallow clone, missing ref) or empty diff: fall back to a
  # smoke subset for empty diffs and to the full corpus when the base itself
  # cannot be resolved.
  if git rev-parse --verify --quiet "$BASE" >/dev/null; then
    echo "test/built-ins/Array"
  else
    echo "test"
  fi
  exit 0
fi

count="$(printf '%s\n' "$files" | wc -l)"
if [ "$count" -gt 40 ]; then
  echo "test"
  exit 0
fi

# $1 = changed path; prints subset prefixes (possibly `test` for full).
map_path() {
  case "$1" in
    test262_config.toml | test262_quarantine.toml)
      echo "test"
      ;;
    core/gc/* | core/string/* | core/interner/*)
      echo "test"
      ;;
    core/engine/src/value/* | core/engine/src/object/* | core/engine/src/environments/* | \
      core/engine/src/property/* | core/engine/src/context/* | core/engine/src/job.rs | \
      core/engine/src/realm.rs)
      echo "test"
      ;;
    core/engine/src/builtins/intl/* | core/engine/src/builtins/temporal/* | core/icu_provider/*)
      echo "test/intl402"
      ;;
    core/engine/src/builtins/*)
      echo "test/built-ins"
      ;;
    core/engine/src/bytecompiler/* | core/engine/src/vm/* | core/engine/src/optimizer/* | \
      core/engine/src/module/* | core/engine/src/script.rs)
      echo "test/language"
      ;;
    core/parser/* | core/ast/* | core/macros/*)
      echo "test/language"
      echo "test/staging"
      ;;
    core/runtime/* | core/wintertc/*)
      echo "test/built-ins"
      ;;
    Cargo.toml | Cargo.lock | rust-toolchain.toml | .cargo/* | core/*/Cargo.toml)
      echo "test"
      ;;
    tests/tester/* | tests/regression/* | cli/* | examples/* | benches/* | docs/* | .github/* | \
      scripts/* | tools/* | utils/* | ffi/* | tests/insta-bytecode/* | tests/wpt/* | tests/fuzz/*)
      echo "test/built-ins/Array"
      ;;
    *)
      echo "test"
      ;;
  esac
}

subsets="$(while IFS= read -r file; do
  [ -n "$file" ] && map_path "$file"
done <<<"$files" | sort -u)"

if printf '%s\n' "$subsets" | grep -qx "test"; then
  echo "test"
else
  printf '%s\n' "$subsets"
fi
