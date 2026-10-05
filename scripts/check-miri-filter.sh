#!/usr/bin/env bash
# P6.2: guard the Miri filter against silent coverage loss.
#
# 1. The Miri CI command selects tests by the substring `miri`, so renaming a
#    `mod miri` drops coverage silently (plan gotcha 1). The allowlist below
#    fails the gate when any expected mod disappears.
# 2. Workspace feature unification must not enable fuzzer features in the
#    Miri build: `boa_engine/fuzz` activates the `instructions_remaining`
#    budget (default 0), which fails every JS-running test with
#    `NoInstructionsRemainError`. Every member that enables `"fuzz"` must be
#    `--exclude`d in the CI Miri command.
# 3. Both Miri commands must throttle test threads (`--test-threads=1`):
#    default parallelism OOM-kills the runner.
# 4. No `mod miri` may build a context via `Context::default()` or a bare
#    `ContextBuilder`: both hit module-root canonicalization, which Miri
#    rejects as an unsupported operation (isolation). Use `test_context().
# 5. `#[cfg_attr(miri, ignore)]` silently drops coverage like a rename, so
#    the ignored set must exactly equal the allowlist below (each entry
#    needs a `MIRI-IGNORE:` justification at the test).
# 6. In-file `mod miri` blocks (in non-test files) must carry `#[cfg(test)]`,
#    or test code leaks into normal builds (unused-import warnings).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail=0

# --- 1. mod miri allowlist (path -> required mod name) ---
check_mod() {
    if ! grep -qE "^[[:space:]]*mod $2 \\{" "$ROOT/$1"; then
        echo "error: expected 'mod $2' missing in $1" >&2
        fail=1
    fi
}

check_mod core/gc/src/test/allocation.rs miri
check_mod core/gc/src/test/cell.rs miri
check_mod core/gc/src/test/erased.rs miri
check_mod core/gc/src/test/weak.rs miri
check_mod core/gc/src/test/weak_map.rs miri
check_mod core/gc/src/test/stress.rs miri
check_mod core/gc/src/test/std_types.rs miri
check_mod core/gc/src/internals/gc_header.rs miri
check_mod core/engine/src/builtins/array_buffer/utils.rs tests_miri
check_mod core/engine/src/builtins/finalization_registry/tests.rs miri
check_mod core/engine/src/value/inner/nan_boxed.rs miri
check_mod core/string/src/tests.rs miri
check_mod core/engine/src/builtins/typed_array/element/mod.rs miri
check_mod core/engine/src/vm/inline_cache/tests.rs miri
check_mod core/engine/src/object/tests.rs miri
check_mod core/engine/src/property/attribute/tests.rs miri

# --- 2. fuzz-enabling members excluded from the Miri command ---
miri_cmd="$(grep -m1 'cargo miri test --workspace' "$ROOT/.github/workflows/rust.yml")"
while IFS= read -r manifest; do
    pkg="$(grep -m1 '^name = ' "$manifest" | sed 's/^name = "\(.*\)"$/\1/')"
    if ! grep -q -- "--exclude $pkg" <<< "$miri_cmd"; then
        echo "error: '$pkg' enables a fuzz feature but is not --exclude'd from the Miri command" >&2
        fail=1
    fi
done < <(grep -rln --include=Cargo.toml '"fuzz"' "$ROOT/core" "$ROOT/tests" "$ROOT/tools" "$ROOT/examples" "$ROOT/ffi" 2>/dev/null)

# --- 3. test-thread throttle present (Miri RSS OOM mitigation) ---
if ! grep -q -- "--test-threads=1" <<< "$miri_cmd"; then
    echo "error: Miri command lacks '--test-threads=1' (proven OOM at default parallelism)" >&2
    fail=1
fi
nightly_cmd="$(grep 'cargo miri test' "$ROOT/.github/workflows/nightly_build.yml")"
if ! grep -q -- "--test-threads=1" <<< "$nightly_cmd"; then
    echo "error: nightly miri_seeds command lacks '--test-threads=1'" >&2
    fail=1
fi

# --- 4. miri mods use the sandboxed context constructor ---
if ! grep -q 'fn test_context()' "$ROOT/core/engine/src/lib.rs"; then
    echo "error: 'fn test_context()' helper missing in core/engine/src/lib.rs" >&2
    fail=1
fi
while IFS= read -r mfile; do
    if grep -nE 'Context::default\(\)|ContextBuilder' "$ROOT/$mfile" | grep -qv 'test_context'; then
        echo "error: sandbox-escaping context constructor in $mfile (use test_context())" >&2
        fail=1
    fi
done <<'EOF'
core/engine/src/builtins/array_buffer/utils.rs
core/engine/src/builtins/finalization_registry/tests.rs
core/engine/src/value/inner/nan_boxed.rs
core/engine/src/builtins/typed_array/element/mod.rs
core/engine/src/vm/inline_cache/tests.rs
core/engine/src/object/tests.rs
core/engine/src/property/attribute/tests.rs
EOF

# --- 5. miri-ignored tests equal the allowlist exactly ---
ignore_files="$(grep -rl --include='*.rs' 'cfg_attr(miri, ignore)' "$ROOT/core" \
    | sed "s|^$ROOT/||" | sort -u | paste -sd' ' -)"
expected_ignores="core/engine/src/object/tests.rs core/gc/src/test/allocation.rs core/gc/src/test/std_types.rs"
if [ "$ignore_files" != "$expected_ignores" ]; then
    echo "error: miri-ignored set drifted: got [$ignore_files], want [$expected_ignores]" >&2
    fail=1
fi
for f in $expected_ignores; do
    if ! grep -q 'MIRI-IGNORE:' "$ROOT/$f"; then
        echo "error: ignored test in $f lacks a MIRI-IGNORE: justification" >&2
        fail=1
    fi
done

# --- 6. in-file miri mods are test-gated ---
while IFS= read -r mfile; do
    if ! grep -B1 -E '^(    )?mod (tests_)?miri \{' "$ROOT/$mfile" | grep -q '#\[cfg(test)\]'; then
        echo "error: in-file mod miri in $mfile lacks #[cfg(test)]" >&2
        fail=1
    fi
done <<'EOF'
core/gc/src/internals/gc_header.rs
core/engine/src/builtins/array_buffer/utils.rs
core/engine/src/value/inner/nan_boxed.rs
core/engine/src/builtins/typed_array/element/mod.rs
EOF

if [ "$fail" -ne 0 ]; then
    exit 1
fi
echo "miri filter gate: 16 mods present, fuzz members excluded, threads throttled"
