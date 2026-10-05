#!/usr/bin/env bash
# P7.2: automated Boa-specific mutant set (see docs/boa-mutants.md).
# Applies each mutant as an exact source transform, runs its killer (which
# MUST go red), then surgically reverts and verifies the file's diff matches
# its pre-run baseline (the tree carries uncommitted session work, so residue
# is defined as diff-drift, not diff-presence). Any survivor or any residue
# fails the run naming the mutant.
#
# Usage: scripts/run-boa-mutants.sh [--list] [BM-... ...] [--without-kani]
#   no args = run all 9. --without-kani skips BM-NAN-1 loudly (its killer is
#   Kani; native suites are proven-green against that mutant by the P6 drill).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! rustc --version 2>/dev/null | grep -q '1\.94\.0'; then
    echo "error: Boa mutants require the P0-pinned 1.94.0 toolchain" >&2
    exit 1
fi

WITHOUT_KANI=0
ONLY=()
for a in "$@"; do
    case "$a" in
        --list)
            echo "BM-CMP-1 BM-IC-1 BM-IC-2 BM-PROTO-1 BM-REG-1 BM-ERR-1 BM-GC-1 BM-NAN-1 BM-FETCH-1"
            exit 0
            ;;
        --without-kani) WITHOUT_KANI=1 ;;
        BM-*) ONLY+=("$a") ;;
        *) echo "error: unknown arg $a" >&2; exit 1 ;;
    esac
done

PATCHER="$ROOT/target/boa-mutant-patch.py"
cat > "$PATCHER" <<'PYEOF'
import sys
# argv: file find_b64 replace_b64  (base64 avoids all shell-quoting pitfalls)
import base64
path, find, repl = sys.argv[1], base64.b64decode(sys.argv[2]).decode(), base64.b64decode(sys.argv[3]).decode()
text = open(path).read()
if text.count(find) != 1:
    sys.exit(f"patch anchor not unique/found in {path}: count={text.count(find)}")
open(path, "w").write(text.replace(find, repl))
print(f"patched {path}")
PYEOF

apply_patch() { # $1=file $2=find_b64 $3=repl_b64
    python3 "$PATCHER" "$ROOT/$1" "$2" "$3"
}

run_one() { # $1=id $2=file $3=find_b64 $4=repl_b64 $5=killer...
    local id="$1" file="$2" find="$3" repl="$4"; shift 4
    if [ "${#ONLY[@]}" -gt 0 ]; then
        local want=0
        for o in "${ONLY[@]}"; do [ "$o" = "$id" ] && want=1; done
        [ "$want" = 1 ] || return 0
    fi
    echo "===== $id ($file) ====="
    local baseline
    baseline="$(git diff -- "$file")"
    apply_patch "$file" "$find" "$repl"
    set +e
    timeout 900 "$@" > "$ROOT/target/boa-mutant-$id.log" 2>&1
    local killer_exit=$?
    set -e
    # Revert FIRST (surgical reverse-patch), then judge.
    apply_patch "$file" "$repl" "$find"
    if [ "$(git diff -- "$file")" != "$baseline" ]; then
        echo "error: $id left residue in $file" >&2
        exit 1
    fi
    if [ "$killer_exit" -eq 0 ]; then
        echo "error: SURVIVOR $id (killer green; log target/boa-mutant-$id.log)" >&2
        exit 1
    fi
    echo "killed $id (killer exit $killer_exit)"
}

B64() { printf '%s' "$1" | base64 -w0; }

# --- BM-CMP-1: lt_fast means <= ---
run_one BM-CMP-1 core/engine/src/value/operations.rs \
    "$(B64 '    pub(crate) fn lt_fast(&self, other: &Self) -> Option<bool> {
        if let (Some(x), Some(y)) = (self.0.as_integer32(), other.0.as_integer32()) {
            return Some(x < y);
        }
        let x = self.as_number_cheap()?;
        let y = other.as_number_cheap()?;
        Some(x < y)
    }')" \
    "$(B64 '    pub(crate) fn lt_fast(&self, other: &Self) -> Option<bool> {
        if let (Some(x), Some(y)) = (self.0.as_integer32(), other.0.as_integer32()) {
            return Some(x <= y);
        }
        let x = self.as_number_cheap()?;
        let y = other.as_number_cheap()?;
        Some(x <= y)
    }')" \
    cargo test -p boa_engine --lib control_flow::loops

# --- BM-IC-1: born megamorphic ---
run_one BM-IC-1 core/engine/src/vm/inline_cache/mod.rs \
    "$(B64 '            entries: GcRefCell::new(ArrayVec::new()),
            megamorphic: Cell::new(false),')" \
    "$(B64 '            entries: GcRefCell::new(ArrayVec::new()),
            megamorphic: Cell::new(true),')" \
    cargo test -p boa_engine --features vm-coverage --lib ic_transitions

# --- BM-IC-2: shape guard inverted ---
run_one BM-IC-2 core/engine/src/vm/inline_cache/mod.rs \
    "$(B64 '                if upgraded.to_addr_usize() == shape_addr {')" \
    "$(B64 '                if upgraded.to_addr_usize() != shape_addr {')" \
    cargo test -p boa_engine --lib inline_cache

# --- BM-PROTO-1: parent walk dropped ---
run_one BM-PROTO-1 core/engine/src/object/internal_methods/mod.rs \
    "$(B64 '        None => {
            // a. Let parent be ? O.[[GetPrototypeOf]]().
            if let Some(parent) = obj.__get_prototype_of__(context)? {
                context.slot().set_not_cacheable_if_already_prototype();
                context.slot().attributes |= SlotAttributes::PROTOTYPE;

                // c. Return ? parent.[[Get]](P, Receiver).
                parent.__get__(key, receiver, context)
            }
            // b. If parent is null, return undefined.
            else {
                Ok(JsValue::undefined())
            }
        }')" \
    "$(B64 '        None => {
            Ok(JsValue::undefined())
        }')" \
    cargo test -p boa_engine --lib object

# --- BM-REG-1: register indices shifted ---
run_one BM-REG-1 core/engine/src/vm/opcode/mod.rs \
    "$(B64 'impl RegisterOperand {
    /// Create a new [`RegisterOperand`] from a u32 value.
    pub(crate) fn new(value: u32) -> Self {
        Self(value)
    }
}')" \
    "$(B64 'impl RegisterOperand {
    /// Create a new [`RegisterOperand`] from a u32 value.
    pub(crate) fn new(value: u32) -> Self {
        Self(value.wrapping_add(1))
    }
}')" \
    cargo test -p boa_engine --lib value::tests::abstract_equality_comparison

# --- BM-ERR-1: TypeError swapped for RangeError ---
run_one BM-ERR-1 core/engine/src/builtins/string/mod.rs \
    "$(B64 '            return Err(JsNativeError::typ().with_message(
                "First argument to String.prototype.startsWith must not be a regular expression",
            ).into());')" \
    "$(B64 '            return Err(JsNativeError::range().with_message(
                "First argument to String.prototype.startsWith must not be a regular expression",
            ).into());')" \
    cargo test -p boa_engine --lib builtins::string::tests::starts_with_with_regex_arg

# --- BM-GC-1: sweep skips root-count reset ---
run_one BM-GC-1 core/gc/src/lib.rs \
    "$(B64 '            if node_ref.is_marked() {
                node_ref.header.unmark();
                node_ref.reset_non_root_count();

                true
            } else {')" \
    "$(B64 '            if node_ref.is_marked() {
                node_ref.header.unmark();

                true
            } else {')" \
    cargo test -p boa_gc weak

# --- BM-NAN-1: NaN not canonicalized (Kani killer) ---
if [ "$WITHOUT_KANI" = 1 ]; then
    echo "===== BM-NAN-1 SKIPPED (--without-kani; native suites proven-green by P6 drill) ====="
else
    if ! cargo kani --version >/dev/null 2>&1; then
        echo "error: BM-NAN-1 needs cargo-kani (or pass --without-kani to skip loudly)" >&2
        exit 1
    fi
    run_one BM-NAN-1 core/engine/src/value/inner/nan_boxed.rs \
        "$(B64 '    pub(super) const fn tag_f64(value: f64) -> u64 {
        if value.is_nan() {
            // Reduce any NAN to a canonical NAN representation.
            f64::NAN.to_bits()
        } else {
            value.to_bits()
        }
    }')" \
        "$(B64 '    pub(super) const fn tag_f64(value: f64) -> u64 {
        if value.is_nan() {
            value.to_bits()
        } else {
            value.to_bits()
        }
    }')" \
        cargo kani -p boa_engine --harness kani_codec_f64_canonical
fi

# --- BM-FETCH-1: TestFetcher drops mappings ---
run_one BM-FETCH-1 core/runtime/src/fetch/tests/mod.rs \
    "$(B64 '    pub fn add_response(&mut self, url: Uri, response: Response<Vec<u8>>) {
        self.request_mapper.insert(url, response);
    }')" \
    "$(B64 '    pub fn add_response(&mut self, url: Uri, response: Response<Vec<u8>>) {
        let _ = (url, response);
    }')" \
    cargo test -p boa_runtime fetch::tests

echo "boa mutants: all ran, all killed, zero residue"
