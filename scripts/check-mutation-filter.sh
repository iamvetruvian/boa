#!/usr/bin/env bash
# P7.2: guard the mutation gate against silent coverage loss.
#
# cargo-mutants discovers sources by walking `mod` statements; it cannot see
# files included only through `cfg_if!` (proven: `value/inner/nan_boxed.rs`
# is default-active production code yet undiscovered) and it skips `#[cfg(test)]`
# files (correct) and unreferenced orphans (correct, but must stay orphans).
# This gate fails when the discovered set drifts from the true product set
# outside the documented allowlist below.
#
# 1. Config scope must still cover the whole product (`core/*/src/**`).
# 2. `gitignore=true` must stay set (without it mutants copies ignored build
#    trees into scratch and dies ENOSPC).
# 3. Every true product file must be discovered, except:
#    - files whose `mod` chain is `#[cfg(test)]`-gated at some level
#      (mutating tests is circular; checked by textual ascent, not patterns), and
#    - the explicit per-file allowlist, each entry machine-checked:
#      ORPHAN (file must stay unreferenced) or COMPENSATED (a Boa-specific
#      mutant in `docs/boa-mutants.md` covers the blind file) or NON-DEFAULT
#      (compiled only under a non-default feature set).
# 4. The Boa-specific set must exist (it carries the compensated files).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail=0
CFG="$ROOT/.cargo/mutants.toml"
BOA_DOC="$ROOT/docs/boa-mutants.md"

# --- 1. config scope covers the whole product ---
if ! grep -q 'core/\*/src/\*\*/\*\.rs' "$CFG"; then
    echo "error: .cargo/mutants.toml lost the core/*/src/** examine scope" >&2
    fail=1
fi

# --- 2. gitignore copy filter stays on ---
if ! grep -q '^gitignore = true' "$CFG"; then
    echo "error: .cargo/mutants.toml lost gitignore=true (proven ENOSPC without it)" >&2
    fail=1
fi

# --- 4 (early; check 3 needs it). Boa-specific set exists ---
if [ ! -f "$BOA_DOC" ]; then
    echo "error: docs/boa-mutants.md missing (carries cfg_if-blind compensation)" >&2
    fail=1
fi

# --- 3. discovery parity ---
discovered="$(mktemp)"
true_set="$(mktemp)"
trap 'rm -f "$discovered" "$true_set"' EXIT
cargo mutants --list-files 2>/dev/null | sort -u > "$discovered"
(cd "$ROOT" && find core -name '*.rs' -path '*/src/*' | sort -u) > "$true_set"

# A missed file is justified iff its `mod` declaration chain is `#[cfg(test)]`
# gated at some level (tests.rs siblings, tests/ dirs, gc's test/ module,
# P5.2's vm/direct.rs, ...). Ascent is textual: stem -> parent module file
# (`dir.rs`, `dir/mod.rs`, or crate-root `lib.rs`/`main.rs`), looking for the
# attribute directly above the `mod` line (rustfmt-canonical, -B1 is exact;
# matches `cfg(test)` and `cfg(all(test, ...))` alike).
is_test_gated() {
    local cur="$1" stem parentdir parent candidate
    cur="$ROOT/$cur"
    while true; do
        if [ "$(basename "$cur")" = "mod.rs" ]; then
            stem="$(basename "$(dirname "$cur")")"
            parentdir="$(dirname "$(dirname "$cur")")"
        else
            stem="$(basename "$cur" .rs)"
            parentdir="$(dirname "$cur")"
        fi
        parent=""
        if [ "$(basename "$parentdir")" = "src" ]; then
            for candidate in "$parentdir/lib.rs" "$parentdir/main.rs"; do
                if [ -f "$candidate" ] && grep -qE "^(pub(\\(crate\\))? )?mod $stem;" "$candidate"; then
                    parent="$candidate"
                    break
                fi
            done
        else
            for candidate in "$parentdir.rs" "$parentdir/mod.rs"; do
                if [ -f "$candidate" ] && grep -qE "^(pub(\\(crate\\))? )?mod $stem;" "$candidate"; then
                    parent="$candidate"
                    break
                fi
            done
        fi
        # No plain `mod` declaration (cfg_if!-hidden or orphan) -> not gated.
        [ -z "$parent" ] && return 1
        if grep -B1 -E "^(pub(\\(crate\\))? )?mod $stem;" "$parent" | grep -qE '#\[cfg\([^)]*\btest\b'; then
            return 0
        fi
        # Not gated at this level: ascend (crate root without a gate ends it).
        if [ "$(basename "$parent")" = "lib.rs" ] || [ "$(basename "$parent")" = "main.rs" ]; then
            return 1
        fi
        cur="$parent"
    done
}

missed_non_test=""
while IFS= read -r m; do
    [ -z "$m" ] && continue
    if ! is_test_gated "$m"; then
        missed_non_test="$missed_non_test
$m"
    fi
done < <(comm -23 "$true_set" "$discovered")
missed="$(grep -v '^$' <<< "$missed_non_test" || true)"

# Explicit per-file allowlist: path|kind|justification. Kinds:
#   ORPHAN  never compiled (must stay unreferenced)
#   COMPENSATED  default-active but cfg_if-hidden (must be covered in boa-mutants.md)
#   VACUOUS  generates zero mutants even if seen (re-proven via --Zmutate-file)
#   NON-DEFAULT  compiled only under a non-default feature (documents the gap)
# Any other kind fails closed (extend the case below deliberately).
allowlist="$(cat <<'EOF'
core/engine/src/object/builtins/jsfinalization_registry.rs|ORPHAN|never mod-included anywhere; 108 lines of dead code (stale copy-paste: says TypedArray)
core/engine/src/value/inner/nan_boxed.rs|COMPENSATED|cfg_if-hidden but default-active value codec; Boa-specific boundary mutants
core/engine/src/value/inner/legacy.rs|NON-DEFAULT|cfg_if-hidden; compiled only with --features jsvalue-enum
core/engine/src/sys/fallback/mod.rs|VACUOUS|3-line std::time reexport; generates zero mutants (re-proven below)
core/engine/src/sys/js/mod.rs|NON-DEFAULT|cfg_if-hidden; compiled only with --features js
core/runtime/src/fetch/tests/mod.rs|COMPENSATED|named `tests` so mutants skips it by convention, but TestFetcher compiles into the product (fetch is default); Boa-specific fake-breaking mutant
EOF
)"

while IFS= read -r m; do
    [ -z "$m" ] && continue
    entry="$(grep "^$m|" <<< "$allowlist" || true)"
    if [ -z "$entry" ]; then
        echo "error: product file invisible to mutants and not allowlisted: $m" >&2
        fail=1
        continue
    fi
    kind="$(cut -d'|' -f2 <<< "$entry")"
    case "$kind" in
        ORPHAN)
            base="$(basename "$m" .rs)"
            if grep -rn --include='*.rs' -E "(mod $base|path.*$base|$base::)" "$ROOT/core" | grep -v "$m" | grep -q .; then
                echo "error: allowlisted ORPHAN is now referenced (revived?): $m" >&2
                fail=1
            fi
            ;;
        COMPENSATED)
            if [ -f "$BOA_DOC" ] && ! grep -qF "$m" "$BOA_DOC"; then
                echo "error: COMPENSATED file lost its Boa-specific coverage: $m" >&2
                fail=1
            fi
            ;;
        VACUOUS)
            if [ -n "$(cargo mutants --no-config --Zmutate-file "$ROOT/$m" 2>/dev/null)" ]; then
                echo "error: VACUOUS file now generates mutants (reclassify): $m" >&2
                fail=1
            fi
            ;;
        NON-DEFAULT)
            # Gap stays documented; nothing to machine-check beyond presence.
            ;;
        *)
            echo "error: unknown allowlist kind '$kind' for $m (extend the gate deliberately)" >&2
            fail=1
            ;;
    esac
done <<< "$missed"

# Stale allowlist entries (file now discovered or deleted) fail: the list must
# shrink, never rot.
while IFS='|' read -r path _ rest; do
    [ -z "$path" ] && continue
    if [ ! -f "$ROOT/$path" ]; then
        echo "error: stale mutation allowlist entry (file gone): $path" >&2
        fail=1
    elif grep -qxF "$path" "$discovered"; then
        echo "error: stale mutation allowlist entry (now discovered): $path" >&2
        fail=1
    fi
done <<< "$allowlist"

if [ "$fail" -ne 0 ]; then
    exit 1
fi
echo "mutation filter gate: discovery parity holds (6 allowlisted blind files)"
