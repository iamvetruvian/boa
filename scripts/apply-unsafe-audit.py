#!/usr/bin/env python3
"""Apply the P6 unsafe audit to a fresh inventory (see docs/unsafe-audit.md).

The committed docs/unsafe-inventory.json is ALWAYS fully derived: fresh
regen + these rules. Manual JSON edits are forbidden (they are silently
overwritten); the rules below are the reviewed audit content. Rule order
matters: first match wins, so specific rules precede family patterns.

Usage:
  python3 scripts/generate-unsafe-inventory.py core > /tmp/fresh.json
  python3 scripts/apply-unsafe-audit.py /tmp/fresh.json docs/unsafe-inventory.json

Exits nonzero with the unmatched-site list unless every site matches a
rule (the audit is complete only at 100% rule coverage).

Rule fields: id (family tag), file (regex, full match against the
inventory path), kind (optional exact), code (optional regex search),
line/lines (optional exact line number(s), only for code-identical
blocks with differing bodies; fail-closed on drift), invariant (the
SAFETY argument), covering_test (real test path or Miri/sanitizer/Kani
coverage note), owner (optional; default is CODEOWNERS: intl/temporal
specialists, else @boa-dev/maintainers).
"""

import json
import re
import sys

DEFAULT_OWNER = "@boa-dev/maintainers"
INTL_TEMPORAL_OWNER = "@jedel1043 @nekevss"


def default_owner(path):
    if "/builtins/intl" in path or "/builtins/temporal" in path:
        return INTL_TEMPORAL_OWNER
    return DEFAULT_OWNER


NAN_BOXED = "core/engine/src/value/inner/nan_boxed\\.rs"
NAN_BOXED_TESTS = (
    "core/engine/src/value/inner/nan_boxed.rs tests (9) + value/tests.rs; "
    "Miri mod miri (P6.2); Kani codec harness (P6.4)"
)

GC_TRACE = "core/gc/src/trace\\.rs"
GC_LIB = "core/gc/src/lib\\.rs"
GC_GC = "core/gc/src/pointers/gc\\.rs"
GC_EPH_BOX = "core/gc/src/internals/ephemeron_box\\.rs"
GC_CELL = "core/gc/src/cell\\.rs"
GC_VTABLE = "core/gc/src/internals/vtable\\.rs"
GC_EPHEMERON = "core/gc/src/pointers/ephemeron\\.rs"
GC_WMAP_BOX = "core/gc/src/internals/weak_map_box\\.rs"
GC_WMAP = "core/gc/src/pointers/weak_map\\.rs"
GC_TEST_ERASED = "core/gc/src/test/erased\\.rs"
GC_TEST_STD = "core/gc/src/test/std_types\\.rs"
GC_TEST_WEAK = "core/gc/src/test/weak\\.rs"

STR_BUILDER = "core/string/src/builder\\.rs"
STR_LIB = "core/string/src/lib\\.rs"
STR_STR = "core/string/src/str\\.rs"
STR_SEQ = "core/string/src/vtable/sequence\\.rs"
STR_SLICE = "core/string/src/vtable/slice\\.rs"
STR_STATIC = "core/string/src/vtable/static\\.rs"
STR_DISPLAY = "core/string/src/display\\.rs"
STR_TESTS_FILE = "core/string/src/tests\\.rs"

AB_UTILS = "core/engine/src/builtins/array_buffer/utils\\.rs"
TA_ELEMENT = "core/engine/src/builtins/typed_array/element/mod\\.rs"
AT_FUTEX = "core/engine/src/builtins/atomics/futex\\.rs"
AT_MOD = "core/engine/src/builtins/atomics/mod\\.rs"
AB_SHARED = "core/engine/src/builtins/array_buffer/shared\\.rs"
DV_MOD = "core/engine/src/builtins/dataview/mod\\.rs"
TA_BUILTIN = "core/engine/src/builtins/typed_array/builtin\\.rs"
TA_OBJECT = "core/engine/src/builtins/typed_array/object\\.rs"

ANY_RS = "core/.*\\.rs"
VM_ARGS = "core/engine/src/vm/opcode/args\\.rs"
VM_TESTS = (
    "engine unit tests + test262 suite; vm verify tests (27); "
    "Miri (P6.2)"
)
ENG_SYM = "core/engine/src/symbol\\.rs"
ENG_TESTS = "engine unit tests + test262 suite; Miri (P6.2)"
ENG_INTEROP = "core/engine/src/interop/into_js_function_impls\\.rs"
ENG_NATFN = "core/engine/src/native_function/mod\\.rs"
ENG_JSOBJ = "core/engine/src/object/jsobject\\.rs"
MACROS_LIB = "core/macros/src/lib\\.rs"
ENG_SYN = "core/engine/src/module/synthetic\\.rs"
ENG_OBJMOD = "core/engine/src/object/mod\\.rs"
ENG_DATATYPES = "core/engine/src/object/datatypes\\.rs"
ENG_PROP = "core/engine/src/property/mod\\.rs"
ENG_VM = "core/engine/src/vm/mod\\.rs"
INT_INTERNED = "core/interner/src/interned_str\\.rs"
INT_RAW = "core/interner/src/raw\\.rs"
RT_TEST262 = "core/runtime/src/test262\\.rs"
INT_TESTS = "boa_interner unit tests; Miri (P6.2)"
RT_TESTS = "boa_runtime tests + test262 suite"
ENG_FUNCST = "core/engine/src/builtins/function/mod\\.rs"
ENG_GEN = "core/engine/src/builtins/generator/mod\\.rs"
ENG_COLL = "core/engine/src/builtins/intl/collator/mod\\.rs"
ENG_NUMFMT = "core/engine/src/builtins/intl/number_format/mod\\.rs"
ENG_ORDMAP = "core/engine/src/builtins/map/ordered_map\\.rs"
ENG_PROM = "core/engine/src/builtins/promise/mod\\.rs"
ENG_ORDSET = "core/engine/src/builtins/set/ordered_set\\.rs"
ENG_THISB = "core/engine/src/environments/runtime/declarative/function\\.rs"
ENG_PRIVENV = "core/engine/src/environments/runtime/private\\.rs"
ENG_ERR = "core/engine/src/error/mod\\.rs"
ENG_HOSTDEF = "core/engine/src/host_defined\\.rs"
ENG_SHAPE = "core/engine/src/object/shape/shared_shape/mod\\.rs"
ENG_LEGACY = "core/engine/src/value/inner/legacy\\.rs"
ENG_CODEBLK = "core/engine/src/vm/code_block\\.rs"
ENG_COMPL = "core/engine/src/vm/completion_record\\.rs"
ENG_BIGINT = "core/engine/src/bigint\\.rs"
ENG_DATEU = "core/engine/src/builtins/date/utils\\.rs"
ENG_CONV = "core/engine/src/builtins/number/conversions\\.rs"
ENG_STRMOD = "core/engine/src/builtins/string/mod\\.rs"
ENG_URI = "core/engine/src/builtins/uri/mod\\.rs"
ENG_INTEROPMOD = "core/engine/src/interop/mod\\.rs"
ENG_MOD = "core/engine/src/module/mod\\.rs"
ENG_CONT = "core/engine/src/native_function/continuation\\.rs"
ENG_NONMAX = "core/engine/src/property/nonmaxu32\\.rs"
ENG_TADISP = "core/engine/src/value/display/typed_array\\.rs"
ENG_PROC = "core/runtime/src/process/mod\\.rs"
INT_FIXED = "core/interner/src/fixed_string\\.rs"
INT_LIB = "core/interner/src/lib\\.rs"
INT_SYM = "core/interner/src/sym\\.rs"
PAR_NUM = "core/parser/src/lexer/number\\.rs"
PAR_REGEX = "core/parser/src/lexer/regex\\.rs"
PAR_TESTS = "boa_parser lexer tests"
WTC_CONSOLE = "core/wintertc/src/console/mod\\.rs"
WTC_FROM = "core/wintertc/src/store/from\\.rs"
WTC_STORE = "core/wintertc/src/store/mod\\.rs"
WTC_TESTS = "boa_wintertc tests"
GC_TESTS_E = (
    "GC cycle tests (core/gc/src/test/*) + engine test262 suite "
    "(no leaks/UAF observed); Miri (P6.2)"
)
BUF_TESTS = (
    "array_buffer/utils.rs tests_miri (5) + typed_array/dataview/atomic "
    "API tests; Miri (P6.2); sanitizers (P6.3)"
)
STR_TESTS = (
    "core/string/src/tests.rs (25) + builder doc tests; Miri (P6.2)"
)
GC_TESTS = (
    "core/gc/src/test/* (allocation/cell/erased/weak/weak_map/std_types); "
    "Miri (P6.2); Kani GC-header harness (P6.4b, partial)"
)

RULES = [
    # Family A: NaN-boxed JsValue codec. Provenance: pointer payloads are
    # built by from_object_like (with_addr on the ORIGINAL pointer, keeping
    # its provenance) and restored by untag+with_addr (blessed round-trip);
    # non-pointer payloads use without_provenance and are never derefed.
    # Every as_*_unchecked call site is guarded by the matching tag check
    # (is_* or MASK_KIND dispatch). Clone/Drop own exactly one refcount
    # unit each (clone+forget / into_inner). Trace marks Object only, sound
    # because String (boa_string has no boa_gc dep), Symbol (Arc Tagged),
    # BigInt (Rc) payloads are never GC-managed.
    {
        "id": "A-trace",
        "file": NAN_BOXED,
        "kind": "unsafe_impl",
        "invariant": (
            "Trace marks Object payloads only. Sound: String/Symbol/BigInt "
            "payloads are Rc/Arc/raw-alloc (boa_string has no boa_gc dep; "
            "JsSymbol is Arc Tagged; JsBigInt is Rc), so no Gc-owned data "
            "hides behind non-Object tags. Re-verify on any repr change."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        "id": "A-clone",
        "file": NAN_BOXED,
        "code": r"MASK_(OBJECT|STRING|SYMBOL|BIGINT) => unsafe",
        "invariant": (
            "Clone arm reconstructs the tag-matching payload, clones it "
            "(+1 ref), and mem::forgets the temporary, so the new value "
            "owns exactly one refcount unit. Tag dispatch guarantees the "
            "payload type; Drop's into_inner balances."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        # Before A-accessor: variant arms also contain as_*_unchecked().clone().
        "id": "A-variant",
        "file": NAN_BOXED,
        "code": r"JsVariant::",
        "invariant": (
            "as_variant arm clones the tag-matching payload into an owned "
            "JsVariant (+1 ref, caller's to drop). Tag dispatch guarantees "
            "the type."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        "id": "A-accessor",
        "file": NAN_BOXED,
        "code": r"as_(bigint|object|symbol|string)_unchecked\(\).*clone\(\)",
        "invariant": (
            "Guarded as_* accessor: the is_* check above guarantees the tag, "
            "so the unchecked reconstruction yields a valid payload; clone "
            "hands the caller an owned (+1) handle."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        "id": "A-unchecked-fn",
        "file": NAN_BOXED,
        "kind": "unsafe_fn",
        "invariant": (
            "Caller contract (documented # Safety): inner value must be a "
            "valid payload of that type. All in-tree callers uphold it via "
            "tag dispatch/guards (verified call-site by call-site in audit)."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        "id": "A-unchecked-body",
        "file": NAN_BOXED,
        "code": r"^unsafe \{$",
        "invariant": (
            "Reconstruction body: untag_pointer + with_addr on self.ptr "
            "restores the original address with preserved provenance "
            "(from_object_like); from_raw wraps the live allocation; "
            "NonNull::new_unchecked is non-null (into_raw never yields "
            "null); ManuallyDrop prevents refcount disturbance."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        "id": "A-toboolean",
        "file": NAN_BOXED,
        "code": r"as_(object|string|bigint)_unchecked\(\)\.(is|is_empty|is_zero)",
        "invariant": (
            "to_boolean borrows the tag-matching payload via ManuallyDrop "
            "(no clone, no refcount touch; temporary dies un-dropped). "
            "Shared borrows only; tag dispatch guarantees the type."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    {
        "id": "A-drop",
        "file": NAN_BOXED,
        "code": r"ManuallyDrop::into_inner",
        "invariant": (
            "Drop arm reconstructs the tag-matching payload and drops the "
            "handle (-1 ref), balancing Clone's +1. Each value drops exactly "
            "once; ManuallyDrop prevents double-drop of temporaries."
        ),
        "covering_test": NAN_BOXED_TESTS,
    },
    # Family B1: Tracer core + Trace trait machinery. trace_until_empty
    # upholds queue-validity (caller contract): as_ref on queued erased
    # pointers, is_marked guard against re-trace loops, trace_fn taken
    # from the node's OWN vtable (type-appropriate). Container impls
    # iterate every element (keys AND values for maps); Cell takes/marks/
    # restores with Default (re-entrant traces see default: terminating
    # and complete); leaves hold no Gc by construction or structure.
    {
        "id": "B-tracer-fn",
        "file": GC_TRACE,
        "code": r"unsafe fn trace_until_empty",
        "invariant": (
            "Caller contract (documented # Safety): every queued pointer "
            "must be valid. Upheld: only live Gc handles are enqueued "
            "(Gc::trace), and collection never runs during sweep."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-tracer-asref",
        "file": GC_TRACE,
        "code": r"node\.as_ref\(\)",
        "invariant": (
            "Derefs a queued erased pointer; valid per the trace_until_empty "
            "caller contract. is_marked check below prevents re-trace loops."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-tracer-dispatch",
        "file": GC_TRACE,
        "code": r"trace_fn\(node",
        "invariant": (
            "Calls the trace fn from the node's own vtable (type-appropriate "
            "by construction) on the valid node pointer."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-trait",
        "file": GC_TRACE,
        "kind": "unsafe_trait",
        "invariant": (
            "Contract declaration (no code): implementors must trace every "
            "contained Gc exactly per the obligation; misuse is implementor "
            "UB, documented on the trait."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-trait-decl",
        "file": GC_TRACE,
        "kind": "unsafe_fn",
        "code": r"unsafe fn trace(_non_roots)?\(&self.*;\s*$",
        "invariant": (
            "Trait method declaration (no body): inherits the Trace contract."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-empty-macro",
        "file": GC_TRACE,
        "code": r"(\$crate::Tracer\) \{\}|trace_non_roots\(&self\) \{\})",
        "invariant": (
            "empty_trace! expansion: no-op trace bodies for leaf types. Sound "
            "wherever instantiated only if the type holds no Gc (each use "
            "audited at its impl: primitives, fn pointers, PhantomData, ICU "
            "string-likes, JsString)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-delegate-fn",
        "file": GC_TRACE,
        "kind": "unsafe_fn",
        "code": r"unsafe fn trace(_non_roots)?\(&self.*\{\s*$",
        "invariant": (
            "Delegation body (custom_trace! macro arms, Box): forwards to the "
            "contained value's Trace impl. Sound iff the callee upholds its "
            "contract (structural: same obligation, smaller value)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-delegate-body",
        "file": GC_TRACE,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "Delegation block (custom_trace! arms, Box arms): calls the "
            "element's trace/trace_non_roots. Sound per the callee contract."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-static-ref",
        "file": GC_TRACE,
        "code": r"Trace for &'static T",
        "invariant": (
            "'static borrows live forever, so contained Gc handles are "
            "permanently reachable without tracing (skipping is correct, not "
            "a leak: collection can never free them)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-leaf-macro",
        "file": GC_TRACE,
        "code": r"Trace for \$T \{ empty_trace",
        "invariant": (
            "Primitive/std-leaf impls (bool/ints/floats/char/TypeId/String/"
            "str/Rc<str>/Path/Instant/NonZero/atomics/File/sockets): none "
            "can contain Gc (no Gc type parameter possible)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-fn-ptr",
        "file": GC_TRACE,
        "code": r"Trace for \$ty \{ empty_trace",
        "invariant": "Function pointers carry no data; nothing to trace.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-extern-decl",
        "file": GC_TRACE,
        "kind": "unsafe_extern",
        "invariant": (
            "Fn-type declarations inside the trace-impl macro (lowest "
            "priority): types only, no code, no runtime effect."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-tuple-impl",
        "file": GC_TRACE,
        "code": r"Trace for \(\$\(",
        "invariant": (
            "Tuple impl: destructures and marks every element (verified in "
            "macro body)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-tuple-body",
        "file": GC_TRACE,
        "code": r"unsafe \{ \$\(mark",
        "invariant": "Tuple macro body: marks every destructured element.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-iter-impl",
        "file": GC_TRACE,
        "code": r"Trace for ([\w:]*::)?(Box|Vec|ThinVec|ArrayVec|Option|Result|BinaryHeap|BTreeMap|BTreeSet|HashMap|HashSet|LinkedList|VecDeque|Either|\[T; N\])",
        "invariant": (
            "Container impl: iterates and marks every element (keys AND "
            "values for maps; active arm for Option/Result/Either). Bodies "
            "verified one by one in audit."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-impl",
        "file": GC_TRACE,
        "code": r"Trace for Cell<T>",
        "invariant": (
            "Cell take/mark/set with Default: re-entrant traces observe the "
            "default (terminating, no infinite regress) while the outer mark "
            "traces the taken value (complete). Requires T: Default (bound "
            "present)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-oncecell-impl",
        "file": GC_TRACE,
        "code": r"Trace for OnceCell<T>",
        "invariant": (
            "Marks the inner value iff present (get borrows; no mutation, "
            "no re-entrancy hazard)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cow-impl",
        "file": GC_TRACE,
        "code": r"Trace for Cow<'static",
        "invariant": (
            "Owned variant traced; Borrowed variant skipped (the borrow is "
            "'static: permanently reachable, same argument as B-static-ref)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-leaf-impl",
        "file": GC_TRACE,
        "code": r"Trace for (PhantomData|LanguageIdentifier|Locale|boa_string::JsString)",
        "invariant": (
            "Leaf impl: PhantomData has no data; ICU LanguageIdentifier/"
            "Locale are external string-likes (icu crates cannot name Gc); "
            "JsString structurally cannot hold Gc (boa_string has no boa_gc "
            "dependency)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Last: the split-line hashbrown impl (line holds `Trace` but the
        # type name sits on the next line). No other bare `unsafe impl`
        # remains in this file after the specific rules above.
        "id": "B-hashbrown-split",
        "file": GC_TRACE,
        "kind": "unsafe_impl",
        "invariant": (
            "hashbrown HashMap impl (split across two lines): iterates and "
            "marks every key and value (body verified in audit)."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B2: allocator + collector. Allocator invariant: every heap
    # pointer comes from Box::new (never null), is registered before it
    # escapes, and is freed exactly once via the paired from_raw/drop_fn
    # (retain-false/mem::take removal). manage_state collects BEFORE
    # into_raw, so the new node is invisible to that collection. Mark
    # phases never deallocate (all heap derefs valid); vtable fns come
    # from the node's own table; DropGuard forbids deref during sweep.
    {
        "id": "B-alloc-new",
        "file": GC_LIB,
        "code": r"NonNull::new_unchecked\(Box::into_raw\(Box::new\(",
        "invariant": (
            "Box::new/into_raw never yields null; the node is pushed to its "
            "heap list before the pointer escapes (a push panic leaks, never "
            "double-frees); collection runs before into_raw, so the new node "
            "cannot be swept prematurely."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-finalize-call",
        "file": GC_LIB,
        "code": r"Self::finalize\(unreachables\)",
        "invariant": (
            "collect passes mark_heap output (live heap-list pointers, no "
            "deallocation before sweep), upholding finalize's caller "
            "contract; the second mark_heap catches finalizer resurrection."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-phase-body",
        "file": GC_LIB,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "Collector phase bodies: the sweep call upholds sweep's contract "
            "(valid Box-allocated heap lists); vtable calls "
            "(trace_non_roots_fn/run_finalizer_fn/drop_fn) use the node's own "
            "table (type-appropriate); trace_until_empty calls run with valid "
            "queues. Mark phases never deallocate."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-heap-asref",
        "file": GC_LIB,
        "code": r"\.as_ref\(\)",
        "invariant": (
            "Derefs a heap-list pointer: valid by the allocator invariant "
            "(registered Box allocations); mark phases perform no drops, and "
            "sweep/drop paths free only via the paired from_raw with "
            "exactly-once list removal."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-trace",
        "file": GC_LIB,
        "code": r"eph_ref\.trace\(tracer\)",
        "invariant": (
            "Dynamic ephemeron key-liveness trace on a valid heap ref "
            "(allocator invariant; mark phase performs no drops)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-weakmap-trace",
        "file": GC_LIB,
        "code": r"node_ref\.trace\(tracer\)",
        "invariant": (
            "Weak-map trace on a valid heap ref (allocator invariant; mark "
            "phase performs no drops)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-fromraw-pair",
        "file": GC_LIB,
        "code": r"Box::from_raw\(",
        "invariant": (
            "Paired with the Box::new allocation (allocator invariant); the "
            "pointer is removed from its heap list exactly once "
            "(retain-false/mem::take), so no double-free and no leak."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-collector-fn",
        "file": GC_LIB,
        "kind": "unsafe_fn",
        "invariant": (
            "Caller contract (documented # Safety): valid pointers "
            "(finalize) / valid Box-allocated lists (sweep). Upheld by "
            "collect/dump, the only callers, via the allocator invariant."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B3: Gc handle. Rooting protocol: a live handle keeps refcount
    # above the non-root count (rooted => enqueued+marked => survives), so
    # inner_ptr derefs are always valid. Clone inc-refs/Drops dec-ref
    # (balanced); casts move ownership without count changes (forget).
    # NonTraceable bodies are unreachable (uninhabited type).
    {
        "id": "B-gc-nontraceable",
        "file": GC_GC,
        "code": r"(Trace for NonTraceable|trace\(&self, _tracer: &mut Tracer\) \{)",
        "invariant": (
            "NonTraceable is uninhabited by construction; all bodies are "
            "unreachable! (panic on misuse, never UB)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by two code-identical lines: NonTraceable's unreachable
        # body and Gc<T>'s inc_non_root_count (rooting protocol). Both
        # verified; the text names each.
        "id": "B-gc-tracenonroots",
        "file": GC_GC,
        "code": r"unsafe fn trace_non_roots\(&self\) \{",
        "invariant": (
            "NonTraceable arm: unreachable (uninhabited). Gc<T> arm: incs "
            "the heap non-root count (rooting protocol: heap-only handles "
            "must be visited or the node is wrongly rooted)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-downcast-fn",
        "file": GC_GC,
        "code": r"pub unsafe fn downcast(_ref)?_unchecked",
        "invariant": (
            "Caller contract (documented # Safety): the cast must be valid. "
            "Delegates to cast_unchecked/cast_ref_unchecked."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Before B-gc-downcast-body: the checked call shares the prefix.
        "id": "B-gc-downcast-checked",
        "file": GC_GC,
        "code": r"Gc::cast_unchecked::<U>\(this\)",
        "invariant": (
            "Checked downcast: TypeId equality verified above, so the cast "
            "is valid."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-downcast-body",
        "file": GC_GC,
        "code": r"Gc::cast(_ref)?_unchecked::",
        "invariant": (
            "Delegates to the cast primitive under the caller's validity "
            "contract."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-erased-trace",
        "file": GC_GC,
        "code": r"Trace for GcErased",
        "invariant": (
            "Transparent single-field wrapper: marks the inner Gc "
            "(complete: no other fields exist)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-cyclic-open",
        "file": GC_GC,
        "code": r"let weak = unsafe \{$",
        "invariant": (
            "new_cyclic block start: from_raw on the fresh alloc_ephemeron "
            "pointer (sole owner, valid); see B-gc-cyclic."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-cyclic",
        "file": GC_GC,
        "code": r"(Ephemeron::from_raw\(Allocator::alloc_ephemeron|\.set\(&gc, \(\)\))",
        "invariant": (
            "new_cyclic: the fresh ephemeron has a sole owner (cannot be "
            "collected while the live local exists); set runs on the valid, "
            "live, unescapable allocation."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-fromraw",
        "file": GC_GC,
        "code": r"pub const unsafe fn from_raw",
        "invariant": (
            "Caller contract (documented # Safety): pointer from into_raw "
            "with same size/alignment. Callers: Clone (live ptr + inc_ref), "
            "cast_unchecked (ownership move), collectors."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-cast-fn",
        "file": GC_GC,
        "code": r"pub unsafe fn cast(_ref)?_unchecked",
        "invariant": (
            "Caller contract (documented # Safety): the cast must be valid. "
            "Layout-safe (Gc<T> is a single NonNull for all T); vtable "
            "dispatch still uses T's table; accessing as U when actually T "
            "is caller UB."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-castref-body",
        "file": GC_GC,
        "code": r"\(&raw const \*this\)\.cast::<Gc<U>>",
        "invariant": (
            "Reborrows &Gc<T> as &Gc<U> without moving (borrow preserved); "
            "valid under the caller's type-validity contract."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-inner-deref",
        "file": GC_GC,
        "code": r"self\.inner_ptr(\(\))?\.as_ref\(\)",
        "invariant": (
            "Derefs the handle's node: valid at all times (a live handle "
            "keeps refcount above non-root count, so the node is rooted or "
            "marked and never swept)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by two bare blocks: Finalize::finalize (dec_ref on the
        # live pre-sweep node) and Clone::clone (inc_ref + from_raw,
        # balancing Drop). Both verified; the text names each.
        "id": "B-gc-bare-body",
        "file": GC_GC,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "Finalize body: dec-refs the inner node, alive because owners "
            "finalize pre-sweep. Clone body: inc-refs then from_raw on the "
            "live pointer (balances Drop's dec-ref)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-trace-impl",
        "file": GC_GC,
        "code": r"Trace for Gc<T>",
        "invariant": (
            "Rooting protocol: trace enqueues the erased pointer; "
            "trace_non_roots incs the heap non-root count."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-gc-trace-fn",
        "file": GC_GC,
        "code": r"unsafe fn trace\(&self, tracer: &mut Tracer\) \{",
        "invariant": "Enqueues the handle's erased pointer for marking.",
        "covering_test": GC_TESTS,
    },
    # Family B4.1: EphemeronBox. Phase discipline on data (UnsafeCell):
    # writes happen only in set (caller contract: no live refs; called on
    # fresh/unescaped allocations) and finalize_and_clear (collector
    # guarantees no remaining refs); reads happen in the mark phase when
    # no mutation occurs, on the single-threaded collector. The key
    # pointer stays derefable at trace time: sweep runs after marking, and
    # a key dead in a prior cycle implies data=None (taken by
    # finalize_and_clear), which returns before the key deref.
    {
        "id": "B-eph-read-fn",
        "file": GC_EPH_BOX,
        "code": r"pub\(crate\) unsafe fn (value|key_ptr|key)\(&self\)",
        "invariant": (
            "Caller contract (documented # Safety): no live mutable refs. "
            "Callers are GC internals during the mark phase, when the "
            "collector performs no ephemeron mutation."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-cell-read",
        "file": GC_EPH_BOX,
        "code": r"&\*self\.data\.get\(\)",
        "invariant": (
            "Shared read of the UnsafeCell: exclusive access holds by the "
            "phase discipline (no mutation between set/finalize and mark-"
            "phase reads; single-threaded collector)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-key-body",
        "file": GC_EPH_BOX,
        "code": r"self\.key_ptr\(\)\.map\(\|data\| data\.as_ref\(\)\)",
        "invariant": (
            "Delegates to key_ptr under the same caller contract; as_ref on "
            "the valid key pointer (see B-eph-key-deref)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-mark-fn",
        "file": GC_EPH_BOX,
        "code": r"pub\(crate\) unsafe fn mark\(&self\)",
        "invariant": (
            "Body performs no unsafe ops (header mark only); the unsafe "
            "marker is a phase contract (mark phase only), upheld by the "
            "collector, the only caller."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-set-fn",
        "file": GC_EPH_BOX,
        "code": r"pub\(crate\) unsafe fn set\(&self",
        "invariant": (
            "Caller contract (documented # Safety): no live refs. Callers: "
            "Gc::new_cyclic (fresh allocation) and Ephemeron::set paths."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by the three bare blocks (all verified; the text names
        # each): key_ptr read, set write, trace_non_roots delegation.
        "id": "B-eph-bare-body",
        "file": GC_EPH_BOX,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "key_ptr body: shared cell read under the no-live-mut-refs "
            "contract. set body: cell write under the no-live-refs "
            "contract. trace_non_roots body: calls value() (contract holds "
            "in the pre-mark root phase) then the safe "
            "trace_non_roots on the value."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-trait-decl",
        "file": GC_EPH_BOX,
        "code": r"unsafe fn trace\(&self, tracer: &mut Tracer\) -> bool;$",
        "invariant": (
            "Trait declaration: implementors must uphold the mark-phase "
            "trace protocol (trace only when marked; see B-eph-trace-fn)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-trace-fn",
        "file": GC_EPH_BOX,
        "code": r"unsafe fn trace\(&self, tracer: &mut Tracer\) -> bool \{$",
        "invariant": (
            "Mark-phase protocol: returns false unmarked; None data "
            "returns true; else traces the value only when the key is also "
            "marked (ephemeron semantics)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-key-deref",
        "file": GC_EPH_BOX,
        "code": r"let key = unsafe \{ data\.key\.as_ref\(\) \};",
        "invariant": (
            "Key deref valid: sweep runs after marking (nothing freed yet "
            "this cycle), and a key dead in a prior cycle implies "
            "data=None, which returns before this deref."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-value-trace",
        "file": GC_EPH_BOX,
        "code": r"unsafe \{ data\.value\.trace\(tracer\) \}",
        "invariant": (
            "Traces the value only when both ephemeron and key are marked; "
            "trace protocol upheld by V: Trace."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-eph-finalize",
        "file": GC_EPH_BOX,
        "code": r"\(\*self\.data\.get\(\)\)\.take\(\)",
        "invariant": (
            "Exclusive cell mutation (take to None); the collector runs "
            "finalize_and_clear only when no refs to the inner data remain."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B4.2: GcRefCell (RefCell-style dynamic borrows). try_borrow
    # rejects Writing + add_reading (panics on overflow); try_borrow_mut
    # requires Unused + set_writing; guards (Drop/Clone) keep the count
    # exact. Trace impls skip while Writing: sound by PAIRED conservatism
    # (trace_non_roots skips too, so pointees under-count heap handles,
    # look rooted, and survive; at worst floating garbage). run_finalizer
    # skips the inner finalizer while Writing (deferred/dropped finalizer
    # semantic, not UB). GcRef/GcRefMut derefs hold the guard with 'a tied
    # to the live cell; NonNull (not &mut) avoids noalias issues.
    {
        # Shared by try_borrow/try_borrow_mut bodies (both verified).
        "id": "B-cell-borrow-body",
        "file": GC_CELL,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "NonNull::new_unchecked(self.cell.get()): UnsafeCell::get never "
            "yields null (valid live T). try_borrow checked non-Writing + "
            "add_reading; try_borrow_mut checked Unused + set_writing; the "
            "returned guard owns the counted borrow."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-trace-impl",
        "file": GC_CELL,
        "code": r"unsafe impl<T: Trace \+ \?Sized> Trace for GcRefCell<T>",
        "invariant": (
            "Trace skips while Writing; sound by paired conservatism with "
            "trace_non_roots (see family note): pointees survive as "
            "apparent roots."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-trace-fn",
        "file": GC_CELL,
        "code": r"unsafe fn trace\(&self, tracer: &mut Tracer\) \{",
        "invariant": "Skips while Writing (paired-conservative, see above).",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-trace-body",
        "file": GC_CELL,
        "code": r"\(\*self\.cell\.get\(\)\)\.trace\(tracer\)",
        "invariant": (
            "Derefs the live cell (shared, non-Writing) and delegates to "
            "T's trace protocol."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-nonroots-fn",
        "file": GC_CELL,
        "code": r"unsafe fn trace_non_roots\(&self\) \{",
        "invariant": (
            "Skips while Writing: under-counts heap handles (conservative; "
            "pairs with the trace skip)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-nonroots-body",
        "file": GC_CELL,
        "code": r"\(\*self\.cell\.get\(\)\)\.trace_non_roots\(\)",
        "invariant": "Delegates to T's trace_non_roots on the live cell.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-finalize-body",
        "file": GC_CELL,
        "code": r"\(\*self\.cell\.get\(\)\)\.run_finalizer\(\)",
        "invariant": (
            "Runs T's finalizer on the live cell; skipped while Writing "
            "(dropped-finalizer semantic, not UB)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by GcRef::cast and GcRefMut::cast (both verified).
        "id": "B-cell-cast-fn",
        "file": GC_CELL,
        "code": r"pub unsafe fn cast",
        "invariant": (
            "Caller contract (documented # Safety): T safely castable to "
            "the target. Moves the borrow guard, preserving accounting."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by GcRef::deref and GcRefMut::deref (both verified).
        "id": "B-cell-deref",
        "file": GC_CELL,
        "code": r"self\.value\.as_ref\(\)",
        "invariant": (
            "Guard held (read or write borrow counted) and 'a ties the "
            "cell alive; shared read is valid under either."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-derefmut",
        "file": GC_CELL,
        "code": r"self\.value\.as_mut\(\)",
        "invariant": (
            "Exclusive borrow held + &mut self (unique); NonNull storage "
            "avoids noalias violations per the NB comment."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-cell-send",
        "file": GC_CELL,
        "code": r"unsafe impl<T: \?Sized \+ Send> Send for GcRefCell<T>",
        "invariant": (
            "Send iff T: Send (same bound as std RefCell); !Sync keeps the "
            "borrow protocol thread-confined."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B4.3: custom const vtables. Each GcBox carries vtable_of::<T>
    # for its own allocation type T (allocator invariant, B2), so the
    # erased-pointer casts to GcBox<Self> are correct by construction. The
    # collector dispatches trace/trace_non_roots/run_finalizer only on
    # live nodes in mark/finalize phases (pre-sweep), and drop_fn exactly
    # once per dead box in sweep (exactly-once free, B2).
    {
        # Shared by the four dispatch fns (all verified; text names each).
        "id": "B-vt-fn",
        "file": GC_VTABLE,
        "code": r"unsafe fn (trace_fn|trace_non_roots_fn|run_finalizer_fn|drop_fn)\(this: GcErasedPointer",
        "invariant": (
            "Caller contract (documented # Safety): the erased pointer is "
            "a live GcBox<Self> (drop_fn: additionally not yet dropped). "
            "Upheld by collector dispatch on the node's own vtable: mark/"
            "finalize phases pre-sweep; sweep drops each dead box once."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by the three cast sites (trace, non-roots, finalizer).
        "id": "B-vt-cast",
        "file": GC_VTABLE,
        "code": r"this\.cast::<GcBox<Self>>\(\)\.as_ref\(\)\.value\(\)",
        "invariant": (
            "Cast correct by vtable-type correspondence (box carries "
            "vtable_of::<T> for its own T); deref valid (live node, "
            "pre-sweep); value() is a safe accessor."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by the two dispatch blocks (both verified; text names
        # each): trace_fn delegates to T's trace; trace_non_roots_fn calls
        # the safe Self::trace_non_roots (block is belt-and-braces).
        "id": "B-vt-dispatch-body",
        "file": GC_VTABLE,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "trace_fn body: Trace::trace under the implementor contract "
            "(T: Trace upholds the trace protocol). trace_non_roots_fn "
            "body: calls safe Self::trace_non_roots (trivially sound)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-vt-drop-body",
        "file": GC_VTABLE,
        "code": r"Box::from_raw\(this\.as_ptr\(\)\)",
        "invariant": (
            "Reconstitutes the Box from the live leaked allocation and "
            "drops it (value + allocation); exactly-once by sweep "
            "discipline (each dead box visited once, B2)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by the four unsafe-fn-pointer type aliases.
        "id": "B-vt-fntype",
        "file": GC_VTABLE,
        "code": r"pub\(crate\) type \w+ = unsafe fn\(this: GcErasedPointer",
        "invariant": (
            "Declares the dispatch contract (erased ptr is GcBox<Self>); "
            "sound by the vtable-type correspondence, see B-vt-fn."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B4.4: Ephemeron handle. Mirrors the Gc rooting protocol
    # (Clone inc-refs, Drop/Finalize dec-refs; a live handle keeps the box
    # rooted-or-marked, so inner_ptr derefs are always valid). Collector
    # ordering (lib.rs): finalize clears dead-key ephemerons BEFORE sweep
    # frees any box, so Some(data) always implies a live key in every
    # phase (mark: nothing freed; finalize: keys freed only in the later
    # sweep; sweep: dead-key entries already cleared, live keys marked).
    # EphemeronValueRef holds a Gc<K>, keeping the key (and hence the
    # entry) alive for the value ref's lifetime.
    {
        "id": "B-ephandle-key-body",
        "file": GC_EPHEMERON,
        "code": r"self\.inner_ptr\.as_ref\(\)\.key_ptr\(\)",
        "invariant": (
            "Box valid by the live-handle protocol; key_ptr contract "
            "(no live mut refs) holds: box data mutates only via "
            "set-on-fresh and pre-sweep finalize_and_clear."
        ),
        "covering_test": GC_TESTS,
    },
    {
        # Shared by the three bare blocks (all verified; text names each).
        "id": "B-ephandle-bare-body",
        "file": GC_EPHEMERON,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "key() body: inc-refs the valid key (Some implies the key "
            "survived the last collection; clearing precedes freeing). "
            "Finalize body: dec-refs the live pre-sweep box. trace body: "
            "marks the live box during the mark phase."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-key-fromraw",
        "file": GC_EPHEMERON,
        "code": r"Gc::from_raw\(key_ptr\)",
        "invariant": (
            "Refcount inc'd above on the valid key; the new handle owns "
            "one unit (balances its Drop)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-value-body",
        "file": GC_EPHEMERON,
        "code": r"self\.inner_ptr\.as_ref\(\)\.value\(\)\?",
        "invariant": (
            "Reads the live box's value; the returned EphemeronValueRef "
            "holds a Gc<K>, keeping the key (hence the entry) alive for "
            "the ref's lifetime."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-hasvalue-body",
        "file": GC_EPHEMERON,
        "code": r"self\.inner_ptr\.as_ref\(\)\.value\(\)\.is_some\(\)",
        "invariant": "Pure read of the live box's value presence.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-inner-body",
        "file": GC_EPHEMERON,
        "code": r"self\.inner_ptr\(\)\.as_ref\(\)",
        "invariant": (
            "inner_ptr() asserts finalizer_safe (no deref during sweep) "
            "and returns the valid live-handle pointer."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-fromraw-fn",
        "file": GC_EPHEMERON,
        "code": r"pub\(crate\) const unsafe fn from_raw",
        "invariant": (
            "Caller contract (documented # Safety): valid pointer, no "
            "double ownership. Callers: Clone (live ptr + inc_ref) and "
            "Gc::new_cyclic (fresh allocation)."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-trace-impl",
        "file": GC_EPHEMERON,
        "code": r"unsafe impl<K: Trace \+ \?Sized, V: Trace> Trace for Ephemeron<K, V>",
        "invariant": (
            "Complete (inner_ptr is the only field); marks the box only, "
            "holding key/value weakly by ephemeron design."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-trace-fn",
        "file": GC_EPHEMERON,
        "code": r"unsafe fn trace\(&self, _tracer: &mut Tracer\) \{",
        "invariant": "Marks the live inner box; key/value held weakly.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-nonroots-fn",
        "file": GC_EPHEMERON,
        "code": r"unsafe fn trace_non_roots\(&self\) \{",
        "invariant": "Incs the live box's non-root count (safe callees).",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-ephandle-clone-body",
        "file": GC_EPHEMERON,
        "code": r"Self::from_raw\(ptr\)",
        "invariant": (
            "Refcount inc'd above on the live pointer; the new handle "
            "owns one unit (balances its Drop)."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B4.5: weak maps. WeakMapBox::trace delegates to the WeakGc
    # only when live (upgrade checked); WeakMap::Trace marks its sole
    # field; RawWeakMap::Trace marks every ephemeron while the hasher S
    # stays untraced by the codebase-wide hasher convention (B1 HashMap/
    # HashSet impls do the same; RawWeakMap is pub(crate) and only ever
    # instantiated with DefaultHashBuilder). Hash/eq helpers call
    # inner()/key() on live ephemerons in user code (finalizer_safe holds;
    # hashing allocates nothing, so no collection mid-probe).
    {
        "id": "B-wmapbox-decl",
        "file": GC_WMAP_BOX,
        "code": r"unsafe fn trace\(&self, tracer: &mut Tracer\);",
        "invariant": "Trait decl: trace the weak ref only when live.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-wmapbox-impl",
        "file": GC_WMAP_BOX,
        "code": r"unsafe fn trace\(&self, tracer: &mut Tracer\) \{",
        "invariant": "Upgrades first; traces only when the map is live.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-wmapbox-body",
        "file": GC_WMAP_BOX,
        "code": r"self\.map\.trace\(tracer\)",
        "invariant": (
            "Delegates to WeakGc's trace protocol on the live map."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-wmap-trace-impl",
        "file": GC_WMAP,
        "code": r"unsafe impl<K: Trace \+ \?Sized \+ 'static, V: Trace \+ 'static> Trace for WeakMap<K, V>",
        "invariant": "Complete: marks inner, the only field.",
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-wmap-raw-impl",
        "file": GC_WMAP,
        "code": r"unsafe impl<K, V, S> Trace for RawWeakMap<K, V, S>",
        "invariant": (
            "Marks every ephemeron; S untraced by the hasher convention "
            "(B1: HashMap/HashSet impls identical); only instantiated "
            "with DefaultHashBuilder."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-wmap-hash-body",
        "file": GC_WMAP,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "make_hash_from_eph: inner()/key() on a live ephemeron in "
            "user code (finalizer_safe holds); hashing allocates nothing."
        ),
        "covering_test": GC_TESTS,
    },
    {
        "id": "B-wmap-equiv-body",
        "file": GC_WMAP,
        "kind": "unsafe_block",
        "code": r"move \|eph\| unsafe \{$",
        "invariant": (
            "equivalent_key closure: inner().key() pointer comparison on "
            "a live ephemeron; allocates nothing."
        ),
        "covering_test": GC_TESTS,
    },
    # Family B4.6: gc crate tests (test-only sites).
    {
        "id": "B-gctest-erased",
        "file": GC_TEST_ERASED,
        "code": r"Gc::cast_unchecked::<Base>\(derived\.clone\(\)\)",
        "invariant": (
            "Test-only: #[repr(C)] prefix cast (Base is Derived's first "
            "field), valid by representation."
        ),
        "covering_test": "core/gc/src/test/erased.rs miri::cast test",
    },
    {
        # Shared by the two leaf-trace test blocks (both verified).
        "id": "B-gctest-trace-body",
        "file": GC_TEST_STD,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "Test-only: traces leaf values (Instant/PathBuf/File) with a "
            "fresh tracer; asserts nothing is enqueued."
        ),
        "covering_test": "core/gc/src/test/std_types.rs",
    },
    {
        # Shared by the two test bypasses (both verified).
        "id": "B-gctest-bypass",
        "file": GC_TEST_WEAK,
        "kind": "trace_bypass",
        "code": r"unsafe_ignore_trace",
        "invariant": (
            "Test-only: Rc<Cell<u8>> fields hold no GC pointers; "
            "correctly untraced."
        ),
        "covering_test": "core/gc/src/test/weak.rs eph_finalizer tests",
    },
    # Family C1: JsStringBuilder (Vec-like raw allocator). Dangling iff
    # unallocated (cap 0); every data()/current_layout() caller guards on
    # is_allocated() or a positive len/cap. Layout round-trip is EXACT:
    # the trait is sealed to Byte in {u8, u16} (size == align), so the
    # capacity recovered from the padded layout recomputes the identical
    # layout (expression now shared with new_layout). P6.1 fixes:
    # current_layout uses Layout::new (never forms & to the uninit
    # header); extend_from_slice_unchecked documents non-overlap.
    {
        "id": "C-bld-setlen-fn",
        "file": STR_BUILDER,
        "code": r"pub const unsafe fn set_len",
        "invariant": (
            "Caller contract (documented # Safety): new_len <= cap, "
            "0..new_len initialized. Callers: push/extend paths "
            "(in-bounds writes), clone_from (checked + copied)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by with_capacity/allocate_inner allocs (both verified).
        "id": "C-bld-alloc",
        "file": STR_BUILDER,
        "code": r"unsafe \{ alloc\((layout|new_layout)\) \}",
        "invariant": (
            "Layout always nonzero (header stores len + refcount); null "
            "routes to handle_alloc_error via the NonNull check below."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-layout-fn",
        "file": STR_BUILDER,
        "code": r"unsafe fn current_layout",
        "invariant": (
            "Caller contract (allocated); reconstruction is exact (sealed "
            "Byte, size == align: capacity round-trips losslessly); "
            "unwrap_unchecked cannot overflow (layout previously "
            "allocated). Callers guard on is_allocated/allocation."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the seven bare blocks (all verified; text names each).
        "id": "C-bld-bare-body",
        "file": STR_BUILDER,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "current_layout body: exact layout math (see C-bld-layout-fn). "
            "push body: push_unchecked after growth to len+1. "
            "extend_from_slice_unchecked body: copy per the cap + "
            "non-overlap contract. extend_from_slice body: capacity "
            "ensured above. push_unchecked body: in-bounds write per "
            "contract. build_inner body: header write to the valid "
            "allocated inner (len > 0). Drop body: dealloc with the "
            "exact layout, guarded, exactly-once (build_inner forgets)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-data-fn",
        "file": STR_BUILDER,
        "code": r"const unsafe fn data",
        "invariant": (
            "Caller contract (allocated); DATA_OFFSET < size (in-bounds) "
            "and Byte-aligned by construction."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-data-body",
        "file": STR_BUILDER,
        "code": r"seq_ptr\.byte_add\(D::DATA_OFFSET\)",
        "invariant": (
            "Raw-pointer arithmetic only (no reference formed); offset "
            "in-bounds and aligned (see C-bld-data-fn)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the allocate_inner/Drop calls (both guarded).
        "id": "C-bld-layout-call",
        "file": STR_BUILDER,
        "code": r"unsafe \{ self\.current_layout\(\) \}",
        "invariant": "Guarded by is_allocated (allocate_inner/Drop).",
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-realloc",
        "file": STR_BUILDER,
        "code": r"realloc\(old_ptr\.cast\(\), old_layout, new_layout\.size\(\)\)",
        "invariant": (
            "Valid allocated ptr (checked), exact old layout "
            "(C-bld-layout-fn), nonzero new size (header); null checked "
            "below."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-extend-fn",
        "file": STR_BUILDER,
        "code": r"pub const unsafe fn extend_from_slice_unchecked",
        "invariant": (
            "Caller contract (documented # Safety, P6.1: cap + "
            "non-overlap): dst in-bounds, aligned, disjoint; len "
            "cannot overflow (bounded by cap)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-push-fn",
        "file": STR_BUILDER,
        "code": r"pub const unsafe fn push_unchecked",
        "invariant": (
            "Caller contract (cap): in-bounds aligned write; len+1 "
            "cannot overflow (cap fits). Callers: push (grown), "
            "build_as_latin1 (grown via push/extend)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-asslice",
        "file": STR_BUILDER,
        "code": r"std::slice::from_raw_parts\(self\.data\(\), self\.len\(\)\)",
        "invariant": (
            "Allocated (checked); 0..len initialized by construction; "
            "aligned; shared borrow (&self) excludes mutation."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-asmut-fn",
        "file": STR_BUILDER,
        "code": r"pub unsafe fn as_mut_slice",
        "invariant": (
            "Memory-sound (&mut self: exclusive, disjoint); unsafe for "
            "the ENCODING contract (documented # Safety: content must be "
            "valid encoding before the borrow ends). No in-tree callers."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-asmut-body",
        "file": STR_BUILDER,
        "code": r"std::slice::from_raw_parts_mut\(self\.data\(\), self\.len\(\)\)",
        "invariant": (
            "Allocated (checked); exclusive via &mut self; encoding "
            "validity is the caller's contract (C-bld-asmut-fn)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by From/Clone (both grow-then-copy into fresh builders).
        "id": "C-bld-extend-call",
        "file": STR_BUILDER,
        "code": r"extend_from_slice_unchecked\((value|self\.as_slice\(\))\)",
        "invariant": (
            "Capacity ensured (with_capacity); disjoint (fresh builder "
            "vs borrowed source)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-setlen0",
        "file": STR_BUILDER,
        "code": r"self\.set_len\(0\)",
        "invariant": "Shrinking to 0: always within cap, no init needed.",
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the clone_from data calls (both verified; text names
        # each): self is allocated (cap > 0 on this path), source is
        # allocated (len > 0 on this path).
        "id": "C-bld-clone-data",
        "file": STR_BUILDER,
        "code": r"unsafe \{ (self|source)\.data\(\) \}",
        "invariant": (
            "self.data(): allocated (grown, or cap >= source_len > 0). "
            "source.data(): allocated (source_len > 0 implies writes "
            "happened)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-clone-copy",
        "file": STR_BUILDER,
        "code": r"ptr::copy_nonoverlapping\(source_data, self_data, source_len\)",
        "invariant": (
            "Both ranges valid (source init'd, self grown/checked), "
            "aligned, and disjoint (&mut self vs &source; borrowck "
            "rejects self-clone_from)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-bld-clone-setlen",
        "file": STR_BUILDER,
        "code": r"self\.set_len\(source_len\)",
        "invariant": "Within cap (checked); initialized (just copied).",
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the Latin1/Common builder fns (both verified).
        "id": "C-bld-latin1-fn",
        "file": STR_BUILDER,
        "code": r"pub unsafe fn build_as_latin1",
        "invariant": (
            "Caller contract (documented # Safety): content is Latin1 "
            "text (interpretation contract for raw u8). Latin1-builder "
            "callers: checked (L881) + tests. Common-builder body "
            "verifies each segment (as_latin1/unreachable + u8 pushes)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the two checked calls (both verified; text names each).
        "id": "C-bld-latin1-checked",
        "file": STR_BUILDER,
        "code": r"unsafe \{ (self|builder)\.build_as_latin1\(\) \}",
        "invariant": (
            "Common::build call: guarded by can_be_latin1 (every "
            "segment checked). build_as_latin1 body call: builder holds "
            "only verified-Latin1 content."
        ),
        "covering_test": STR_TESTS,
    },
    # Family C2a: JsString (refcounted vtable-embedded allocations).
    # Every live JsString owns a refcount unit (clone incs, drop decs,
    # from_raw/into_raw move it), so &JsString implies a live allocation
    # whose first field is always the vtable. slice_unchecked callers all
    # establish start <= end <= len (position/rposition bounds, explicit
    # clamps + checks, macro checks).
    {
        # Shared by the five checked slice calls (all verified).
        "id": "C-str-slice-call",
        "file": STR_LIB,
        "code": r"slice_unchecked\((self|str), ",
        "invariant": (
            "trims: position/rposition bounds (all-trimmable early-"
            "returns; first <= last; end+1 <= len). slice(): p2 "
            "clamped, p1 < p2 checked. macro: end <= len, start <= "
            "end checked."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-fromraw-fn",
        "file": STR_LIB,
        "code": r"pub const unsafe fn from_raw",
        "invariant": (
            "Caller contract (documented # Safety): ptr from into_raw "
            "(ownership move, no Drop on the source). No in-tree "
            "callers (pub API)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-fromptr-fn",
        "file": STR_LIB,
        "code": r"pub\(crate\) const unsafe fn from_ptr",
        "invariant": (
            "Caller contract (documented # Safety): valid vtable ptr "
            "with an owned refcount unit. Callers: vtable Clone impls "
            "(sequence/slice/static), which inc first (C3)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-vtable-body",
        "file": STR_LIB,
        "code": r"unsafe \{ self\.ptr\.as_ref\(\) \}",
        "invariant": (
            "Vtable is the first field of every variant (embedded); "
            "live by refcount ownership (&JsString holds a unit; "
            "statics are 'static)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-slice-fn",
        "file": STR_LIB,
        "code": r"pub unsafe fn slice_unchecked\(data",
        "invariant": (
            "Caller contract (documented # Safety): start <= end <= "
            "len. All callers establish it (C-str-slice-call)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-slice-body",
        "file": STR_LIB,
        "code": r"SliceString::new\(data, start, end\)",
        "invariant": (
            "Under the fn contract; Box::leak transfers ownership to "
            "the new JsString (dropped via the Slice vtable, C3)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-asinner-fn",
        "file": STR_LIB,
        "code": r"pub\(crate\) unsafe fn as_inner",
        "invariant": (
            "Caller contract (documented # Safety): kind-validated. "
            "Sole caller display.rs (kind-matched Slice arm)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-asinner-body",
        "file": STR_LIB,
        "code": r"self\.ptr\.cast::<T>\(\)\.as_ref\(\)",
        "invariant": "Cast+deref under the kind-validation contract.",
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-str-concat-head",
        "file": STR_LIB,
        "code": r"let mut data = unsafe \{",
        "invariant": (
            "SequenceString::allocate guarantees a valid seq pointer "
            "with a full_count data region (C3); offset in-bounds."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the concat loop + from_slice_skip_interning bodies.
        "id": "C-str-copy-body",
        "file": STR_LIB,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "concat: per-string counts sum to full_count (checked_add); "
            "each copy/widening step in-bounds, aligned (allocate), "
            "disjoint (fresh alloc). interning-skip: count = len, "
            "fresh alloc(count); raw field projection (no & to uninit)."
        ),
        "covering_test": STR_TESTS,
    },
    # Family C2b: JsStr (borrowed slice enum). Sync+Send: holds only
    # &'a [u8]/[u16] (immutable Sync+Send data). as_static/get_unchecked
    # are caller contracts; the only in-tree callers are SliceString::new
    # (bounds under its contract; ownership keeps the referent alive).
    {
        "id": "C-jssstr-sync",
        "file": STR_STR,
        "code": r"unsafe impl Sync for JsStr",
        "invariant": "Holds only shared refs to u8/u16 (Sync); read-only.",
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-jssstr-send",
        "file": STR_STR,
        "code": r"unsafe impl Send for JsStr",
        "invariant": "Read-only shared refs; no mutation, no data race.",
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-jssstr-static-fn",
        "file": STR_STR,
        "code": r"pub unsafe fn as_static",
        "invariant": (
            "Caller contract (documented # Safety): caller ensures the "
            "lifetime. Sole caller SliceString::new, which owns the "
            "referent (C3)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the Latin1/Utf16 arms (both verified).
        "id": "C-jssstr-static-body",
        "file": STR_STR,
        "code": r"std::slice::from_raw_parts\(v\.as_ptr\(\), v\.len\(\)\)",
        "invariant": (
            "Same ptr+len as the live source slice; only the lifetime "
            "is extended (caller's contract)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-jssstr-get-fn",
        "file": STR_STR,
        "code": r"pub unsafe fn get_unchecked<I>",
        "invariant": (
            "Caller contract (documented # Safety): index in bounds. "
            "Sole caller SliceString::new (under its contract, C3)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-jssstr-get-body",
        "file": STR_STR,
        "code": r"JsSliceIndex::get_unchecked\(self, index\)",
        "invariant": "Delegates under the caller's bounds contract.",
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-jssstr-trait-decl",
        "file": STR_STR,
        "code": r"unsafe fn get_unchecked\(value: JsStr<'a>, index: Self\) -> Self::Value;",
        "invariant": "Trait decl: implementors require in-bounds index.",
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the five index-impl fns (all verified).
        "id": "C-jssstr-idx-fn",
        "file": STR_STR,
        "code": r"unsafe fn get_unchecked\(value: JsStr<'a>, index: Self\) -> Self::Value \{",
        "invariant": (
            "Caller contract (documented # Safety): index in bounds; "
            "delegates to slice::get_unchecked per variant."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the five index-impl bodies (all verified).
        "id": "C-jssstr-idx-body",
        "file": STR_STR,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "usize/Range/RangeInclusive/RangeFrom/RangeTo bodies: "
            "slice::get_unchecked on the live variant slice under the "
            "caller's bounds contract."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-jssstr-full-fn",
        "file": STR_STR,
        "code": r"unsafe fn get_unchecked\(value: JsStr<'a>, _index: Self\)",
        "invariant": "Trivially sound: returns value unchanged (no deref).",
        "covering_test": STR_TESTS,
    },
    # Family C3a: SequenceString vtable. Dispatch-correct by construction
    # (allocate/build_inner write seq_*::<T> for the allocation's own T;
    # repr(C) vtable-first; header fully initialized at construction).
    # Refcount: checked inc/dec (abort on overflow/underflow), dealloc
    # only at zero with the exact layout (same sealed-Byte proof as C1;
    # all three layout expressions identical). Data fully initialized by
    # the only allocate callers (concat, interning-skip) before exposure.
    # The vtable 'static is contained (shortened at both call sites).
    {
        "id": "C-seq-alloc-body",
        "file": STR_SEQ,
        "code": r"alloc\(layout\)\.cast::<Self>\(\)",
        "invariant": (
            "Layout nonzero (header); null checked below (Err -> "
            "handle_alloc_error); overflow checked (Layout::array/"
            "extend Err -> alloc_overflow)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the three bare blocks (all verified; text names each).
        "id": "C-seq-bare-body",
        "file": STR_SEQ,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "allocate header write: valid NonNull, aligned; writes the "
            "fully-initialized header (vtable + refcount 1). debug "
            "offset assert: reads the written header; offset in-bounds. "
            "seq_drop dealloc: refcount zero (last unit), exact layout, "
            "ptr from alloc."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the four dispatch casts (all verified).
        "id": "C-seq-cast",
        "file": STR_SEQ,
        "code": r"let this: &SequenceString<T> = unsafe \{ vtable\.cast\(\)\.as_ref\(\) \};",
        "invariant": (
            "Dispatch-correct by construction (seq_*::<T> stored only "
            "in SequenceString<T> headers); live (refcount held at "
            "clone/drop/as_str/refcount)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-seq-clone-body",
        "file": STR_SEQ,
        "code": r"unsafe \{ JsString::from_ptr\(vtable\) \}",
        "invariant": (
            "Refcount inc'd above (checked, abort on overflow); the new "
            "JsString owns one unit."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-seq-layout-body",
        "file": STR_SEQ,
        "code": r"let layout = unsafe \{",
        "invariant": (
            "Reconstructs from the valid header ref (for_value on "
            "initialized data) + vtable.len (== allocated cap); exact "
            "by the sealed-Byte proof (C1); unwrap_unchecked cannot "
            "overflow (layout previously allocated)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-seq-str-body",
        "file": STR_SEQ,
        "code": r"std::slice::from_raw_parts\(data_ptr, len\)",
        "invariant": (
            "data_ptr in-bounds + Byte-aligned (offset by construction); "
            "len == allocated cap; data initialized before exposure "
            "(only callers write fully); 'static shortened by callers."
        ),
        "covering_test": STR_TESTS,
    },
    # Family C3b: SliceString vtable. new() slices under its bounds
    # contract, clones owned BEFORE extending the lifetime (field order +
    # the & borrow), so the 'static referent stays alive via owned.
    # Dispatch-correct by construction; Box::from_raw round-trips the
    # Box::new+leak from slice_unchecked, exactly once (refcount gate).
    {
        "id": "C-slice-new-fn",
        "file": STR_SLICE,
        "code": r"pub\(crate\) unsafe fn new\(owned",
        "invariant": (
            "Caller contract (documented # Safety): start <= end <= "
            "len. Sole caller slice_unchecked (contract, C2a)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-slice-new-get",
        "file": STR_SLICE,
        "code": r"owned\.as_str\(\)\.get_unchecked\(start\.\.end\)",
        "invariant": "Range in-bounds under the fn contract.",
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-slice-new-static",
        "file": STR_SLICE,
        "code": r"inner\.as_static\(\)",
        "invariant": (
            "Lifetime extension valid: owned.clone() stored above keeps "
            "the referent alive for the SliceString's lifetime."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the four dispatch casts (all verified).
        "id": "C-slice-cast",
        "file": STR_SLICE,
        "code": r"let this: &SliceString = unsafe \{ vtable\.cast\(\)\.as_ref\(\) \};",
        "invariant": (
            "Dispatch-correct by construction (slice_* fns stored only "
            "in SliceString headers); live (refcount held)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-slice-clone-body",
        "file": STR_SLICE,
        "code": r"unsafe \{ JsString::from_ptr\(vtable\) \}",
        "invariant": (
            "Refcount inc'd above (checked, abort on overflow); the new "
            "JsString owns one unit."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-slice-drop-body",
        "file": STR_SLICE,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "Box::from_raw round-trips the Box::new+leak (same "
            "allocator); exactly-once (refcount-zero gate); drop runs "
            "owned's Drop + frees."
        ),
        "covering_test": STR_TESTS,
    },
    # Family C3c: static strings + the display caller.
    {
        "id": "C-static-clone-body",
        "file": STR_STATIC,
        "code": r"unsafe \{ JsString::from_ptr\(this\) \}",
        "invariant": (
            "Statics are 'static (never freed); copying the pointer "
            "needs no refcount unit."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-static-cast",
        "file": STR_STATIC,
        "code": r"let this: &StaticString = unsafe \{ this\.cast\(\)\.as_ref\(\) \};",
        "invariant": (
            "Dispatch-correct (static_* fns only in StaticString "
            "vtables); 'static referent always live."
        ),
        "covering_test": STR_TESTS,
    },
    {
        "id": "C-disp-slice",
        "file": STR_DISPLAY,
        "code": r"self\.inner\.as_inner\(\)",
        "invariant": (
            "Kind-validated: inside the JsStringKind::Slice match arm "
            "(sole as_inner caller)."
        ),
        "covering_test": STR_TESTS,
    },
    {
        # Shared by the three test builds (all verified).
        "id": "C-test-latin1",
        "file": STR_TESTS_FILE,
        "code": r"let s_builder = unsafe",
        "invariant": (
            "Test-only: build_as_latin1 on the s_latin1_literal "
            "fixture (latin1 by construction); contract holds."
        ),
        "covering_test": "core/string/src/tests.rs latin1 tests",
    },
    # Family D1: ArrayBuffer utils (shared/plain buffer primitives).
    # Batch-copy proof (uniform over the 20 phase blocks): head/chunks/
    # tail partition count exactly (head <= count; head+8*chunks+tail ==
    # count); same-misalignment check + head bytes align BOTH pointers
    # (debug_asserted); chunks==0 falls back to byte loops (regression
    # a6101fe1); AtomicU8 is repr(transparent) u8 so aligned u64/atomic
    # reinterpretation is valid; all indices stay in [0, count). Forward
    # order is overlap-correct iff src >= dest (P6.1 doc fix); backward
    # iff src <= dest; memmove picks by comparison. get/set_value bounds
    # + alignment are caller contracts, verified per call site in the
    # typed_array/atomics/dataview families.
    {
        # Shared by the const/mut ptr-bump fns (both verified).
        "id": "D-add-fn",
        "file": AB_UTILS,
        "code": r"pub\(crate\) unsafe fn add\(self, count: usize\)",
        "invariant": (
            "Caller contract (documented # Safety): in-bounds offset "
            "staying in the allocation. Callers add validated byte "
            "indices (typed_array/dataview families)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # The two add bodies (lines-anchored: code-identical blocks).
        "id": "D-add-body",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [23, 42],
        "invariant": "Pointer add under the caller's in-bounds contract.",
        "covering_test": BUF_TESTS,
    },
    {
        # to_vec + clone memcpy bodies (lines-anchored).
        "id": "D-memcpy-body",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [91, 226],
        "invariant": (
            "to_vec: atomic slice -> fresh Vec (cap == count), then "
            "set_len(count) over initialized bytes; disjoint. clone: "
            "same-length fresh buffer; disjoint."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-getvalue-fn",
        "file": AB_UTILS,
        "code": r"pub\(crate\) unsafe fn get_value\(",
        "invariant": (
            "Caller contract (documented # Safety): enough bytes + "
            "element alignment (debug-asserted). Kind->T mapping "
            "matches TypedArrayKind semantics."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-readelem-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn read_elem<T: Element>",
        "invariant": "Delegates to T::read (D2) under the fn contract.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-readelem-body",
        "file": AB_UTILS,
        "code": r"T::read\(buffer\)\.load\(order\)",
        "invariant": "Element load via the D2 read protocol.",
        "covering_test": BUF_TESTS,
    },
    {
        # get_value + set_value dispatch bodies (lines-anchored).
        "id": "D-dispatch-body",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [171, 369],
        "invariant": (
            "Kind/element dispatch under the fn contract; each arm "
            "calls read_elem/write_elem with the matching type."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-setvalue-fn",
        "file": AB_UTILS,
        "code": r"pub\(crate\) unsafe fn set_value",
        "invariant": (
            "Caller contract (documented # Safety): enough bytes + "
            "element alignment (debug-asserted). Value carries its "
            "type (match is type-exact)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-writeelem-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn write_elem<T: Element>",
        "invariant": "Delegates to T::read_mut store under the contract.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-write-body",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 357,
        "invariant": "Element store via the D2 write protocol.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-batched-fwd-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn batched_atomic_copy_forward",
        "invariant": (
            "Caller contract (documented # Safety, P6.1: valid ranges; "
            "disjoint or src >= dest). Partition/alignment proof in "
            "the family note."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-batched-bwd-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn batched_atomic_copy_backward",
        "invariant": (
            "Caller contract (valid ranges); backward order is "
            "overlap-correct for src <= dest. Proof in family note."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-batched-b2a-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn batched_copy_bytes_to_atomic",
        "invariant": (
            "Caller contract (valid ranges, disjoint). Proof in "
            "family note; u64 ptr::read over initialized bytes."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-batched-a2b-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn batched_copy_atomic_to_bytes",
        "invariant": (
            "Caller contract (valid ranges, disjoint). Proof in "
            "family note; u64 ptr::write initializes dest."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # All 20 batch-phase blocks (lines-anchored; uniform proof).
        "id": "D-batch-body",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [
            438, 450, 460, 479, 490,
            516, 528, 540, 560, 570,
            593, 605, 615, 631, 642,
            665, 677, 687, 703, 714,
        ],
        "invariant": (
            "Byte fallbacks: in-bounds per contract. Head/tail "
            "phases: partition indices in [0, count). u64 phases: "
            "8-aligned by the misalignment check + head (debug-"
            "asserted); AtomicU8 transparent so reinterpretation is "
            "valid; chunk ranges within [0, count)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-shared-fwd-fn",
        "file": AB_UTILS,
        "code": r"pub\(super\) unsafe fn copy_shared_to_shared\(",
        "invariant": (
            "Caller contract (count bytes each side); overlap "
            "allowed only src >= dest (forward-correct). Callers: "
            "memcpy (disjoint), memmove (directional), grow path."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-shared-fwd-body",
        "file": AB_UTILS,
        "code": r"batched_atomic_copy_forward\(src, dest, count\) \}$",
        "invariant": "Delegates under the fn contract.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-shared-bwd-fn",
        "file": AB_UTILS,
        "code": r"unsafe fn copy_shared_to_shared_backwards\(",
        "invariant": (
            "Caller contract (count bytes each side). Sole caller "
            "memmove (src < dest overlap: backward-correct)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-shared-bwd-body",
        "file": AB_UTILS,
        "code": r"batched_atomic_copy_backward\(src, dest, count\) \}$",
        "invariant": "Delegates under the fn contract.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-memcpy-fn",
        "file": AB_UTILS,
        "code": r"pub\(crate\) unsafe fn memcpy",
        "invariant": (
            "Caller contract (documented # Safety): count bytes each "
            "side, disjoint. Dispatches per representation."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # Shared by the four memcpy arms (all verified). Tuple pattern
        # keeps this from matching the memmove arms below.
        "id": "D-memcpy-arms",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"\(BytesConstPtr::\w+\(src\), BytesMutPtr::\w+\(dest\)\) => unsafe \{$",
        "invariant": (
            "Bytes/Bytes: copy_nonoverlapping per contract. Mixed/ "
            "shared arms: delegate to the matching batched copy "
            "under the contract."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-memmove-naive-fn",
        "file": AB_UTILS,
        "code": r"pub\(crate\) unsafe fn memmove_naive",
        "invariant": (
            "Caller contract (valid from/to ranges). Naive "
            "left-to-right order is INTENDED (documented slice "
            "quirk); in-bounds either way."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # The two naive arms (lines-anchored; text names each).
        "id": "D-memmove-naive-arms",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"BytesMutPtr::\w+\(ptr\) => unsafe \{$",
        "lines": [800, 806],
        "invariant": (
            "Bytes arm: per-byte ptr::copy (overlap-safe), "
            "left-to-right (intended naive order). AtomicBytes arm: "
            "forward shared copy (naive order intended)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-memmove-fn",
        "file": AB_UTILS,
        "code": r"pub\(crate\) unsafe fn memmove\(",
        "invariant": (
            "Caller contract (valid from/to ranges). Direction "
            "picked by src/dest comparison (overlap-correct)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # The two memmove arms (lines-anchored; text names each).
        "id": "D-memmove-arms",
        "file": AB_UTILS,
        "kind": "unsafe_block",
        "code": r"BytesMutPtr::\w+\(ptr\) => unsafe \{$",
        "lines": [823, 829],
        "invariant": (
            "Bytes arm: ptr::copy (overlap-safe memmove). "
            "AtomicBytes arm: backward iff src < dest (both "
            "overlap-correct orders)."
        ),
        "covering_test": BUF_TESTS,
    },
    # Family D1t: utils tests_miri (test-only; fixture bounds verified).
    {
        # Sub-batch regression test pointers (off<8, off+33<64,
        # disjoint).
        "id": "D-test-add-off",
        "file": AB_UTILS,
        "code": r"unsafe \{ data\.as_ptr\(\).add\(off( \+ 32)?\) \};",
        "invariant": "Test-only: 64-elem Vec, offsets in-bounds.",
        "covering_test": "utils.rs batched_forward_sub_batch_same_misalignment",
    },
    {
        # Shared by the seven offset-1/2 fixture pointers.
        "id": "D-test-add12",
        "file": AB_UTILS,
        "code": r"unsafe \{ (src_data|dest_data)\.as_ptr\(\).add\([12]\) \};",
        "invariant": "Test-only: 32-elem Vecs, offsets 1/2 in-bounds.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-test-add-mut",
        "file": AB_UTILS,
        "code": r"dest_data\.as_mut_ptr\(\).add\(2\)",
        "invariant": "Test-only: 32-elem Vec, offset 2 in-bounds.",
        "covering_test": BUF_TESTS,
    },
    {
        # Shared by the two forward test calls (bounds verified).
        "id": "D-test-call-fwd",
        "file": AB_UTILS,
        "code": r"batched_atomic_copy_forward\(src, dest, count\) \};",
        "invariant": "Test-only: valid ranges, disjoint fixtures.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-test-call-bwd",
        "file": AB_UTILS,
        "code": r"batched_atomic_copy_backward\(src, dest, count\) \};",
        "invariant": "Test-only: valid ranges, disjoint fixtures.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-test-call-b2a",
        "file": AB_UTILS,
        "code": r"batched_copy_bytes_to_atomic\(src, dest, count\)",
        "invariant": "Test-only: valid ranges, disjoint fixtures.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-test-call-a2b",
        "file": AB_UTILS,
        "code": r"batched_copy_atomic_to_bytes\(src, dest, count\)",
        "invariant": "Test-only: valid ranges, disjoint fixtures.",
        "covering_test": BUF_TESTS,
    },
    {
        # Shared by the six atomic verification loads.
        "id": "D-test-load",
        "file": AB_UTILS,
        "code": r"\(\*[a-z]+\.add\(i\)\)\.load\(Ordering::Relaxed\)",
        "invariant": "Test-only: in-bounds verification reads.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D-test-load0",
        "file": AB_UTILS,
        "code": r"\(\*dest\.add\(0\)\)\.load",
        "invariant": "Test-only: single-elem verification read.",
        "covering_test": BUF_TESTS,
    },
    {
        # Shared by the two plain verification reads.
        "id": "D-test-read",
        "file": AB_UTILS,
        "code": r"unsafe \{ \*[a-z]+\.add\(i\) \};",
        "invariant": "Test-only: in-bounds verification reads.",
        "covering_test": BUF_TESTS,
    },
    # Family D2: TypedArray Element protocol. Every (element, atomic)
    # pair is size/align-identical (ints, ClampedU8/u8, Float16/u16,
    # f32/u32, f64/u64); elements are NoUninit+AnyBitPattern (any bits
    # valid) and atomics repr-transparent, so reinterpreting buffer
    # storage as &E/&Atomic is valid under the align+size contract
    # (debug-asserted at every impl). File denies unsafe_op_in_unsafe_fn.
    {
        # Zeroable + Pod for Float16 (both verified).
        "id": "D2-pod",
        "file": TA_ELEMENT,
        "code": r"unsafe impl bytemuck::(Zeroable|Pod) for Float16",
        "invariant": (
            "Float16 is repr(transparent) f16; every u16 pattern is a "
            "valid f16 (incl. NaN); no padding."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-read-decl",
        "file": TA_ELEMENT,
        "code": r"unsafe fn read\(buffer: SliceRef<'_>\) -> ElementRef<'_, Self>;",
        "invariant": "Trait decl: align+size contract for implementors.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-readmut-decl",
        "file": TA_ELEMENT,
        "code": r"unsafe fn read_mut\(buffer: SliceRefMut<'_>\) -> ElementRefMut<'_, Self>;",
        "invariant": "Trait decl: align+size contract for implementors.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-read-fn",
        "file": TA_ELEMENT,
        "code": r"unsafe fn read\(buffer: SliceRef<'_>\) -> ElementRef<'_, Self> \{",
        "invariant": (
            "Macro impl (all 12 elements): debug-asserts size+align, "
            "then reinterprets per representation."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-read-plain",
        "file": TA_ELEMENT,
        "code": r"SliceRef::Slice\(buffer\) => unsafe \{",
        "invariant": (
            "&u8 storage as &E: aligned+sized (contract, asserted); "
            "valid by NoUninit+AnyBitPattern."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-read-atomic",
        "file": TA_ELEMENT,
        "code": r"SliceRef::AtomicSlice\(buffer\) => unsafe \{",
        "invariant": (
            "&AtomicU8 storage as &E::Atomic: same size/align "
            "(all pairs), transparent; shared+interior mutability."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-readmut-fn",
        "file": TA_ELEMENT,
        "code": r"unsafe fn read_mut\(buffer: SliceRefMut<'_>\) -> ElementRefMut<'_, Self> \{",
        "invariant": (
            "Macro impl (all 12 elements): debug-asserts size+align, "
            "then reinterprets per representation."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-readmut-plain",
        "file": TA_ELEMENT,
        "code": r"SliceRefMut::Slice\(buffer\) => unsafe \{",
        "invariant": "&mut u8 storage as &mut E: exclusive via &mut.",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D2-readmut-atomic",
        "file": TA_ELEMENT,
        "code": r"SliceRefMut::AtomicSlice\(buffer\) => unsafe \{",
        "invariant": "&AtomicU8 storage as &E::Atomic (shared atomic).",
        "covering_test": BUF_TESTS,
    },
    # Family D3a: futex waiters. All list ops run under the global static
    # Mutex (Send justified; Cell/link cross-thread access serialized;
    # poisoned-mutex early returns strand nodes in an unreachable list,
    # never derefed). Sync waiters: stack-owned, UnsafeRef never
    # reconstructed. Async waiters: one into_raw unit per list entry,
    # reconstructed exactly once on removal (take()+unlink; is_linked
    # guards; all removals under the lock). Element reads: 64-aligned
    # SAB base + spec-validated multiple-of-size offsets; E in {i32,i64}.
    {
        "id": "D3-send",
        "file": AT_FUTEX,
        "code": r"unsafe impl Send for FutexWaiters",
        "invariant": (
            "Exposed only through the global static Mutex: single-"
            "thread-at-a-time access to the intrusive (non-Send) list."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-add-fn",
        "file": AT_FUTEX,
        "code": r"unsafe fn add_waiter\(&mut self",
        "invariant": (
            "Caller contract (unlinked + valid-until-removed). Sole "
            "caller wait() (fresh stack waiter, removed if linked)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-add-body",
        "file": AT_FUTEX,
        "code": r"UnsafeRef::from_raw\(ptr::from_ref\(node\)\)",
        "invariant": (
            "Borrows the valid node (contract); never reconstructed "
            "(sync waiters are stack-owned)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-addasync-fn",
        "file": AT_FUTEX,
        "code": r"unsafe fn add_async_waiter",
        "invariant": (
            "Caller contract (unlinked). Sole caller wait_async "
            "(fresh Arc; removal reconstructs once)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-addasync-body",
        "file": AT_FUTEX,
        "code": r"UnsafeRef::from_raw\(Arc::into_raw\(node\)\)",
        "invariant": (
            "Moves one Arc unit into the list; reconstructed exactly "
            "once on removal (async_data tag + unlink)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-notify-drop",
        "file": AT_FUTEX,
        "code": r"Arc::from_raw\(UnsafeRef::into_raw\(elem\)\)",
        "invariant": (
            "Async entries only (async_data Some <=> from Arc); pop "
            "unlinks, so exactly-once."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-remove-fn",
        "file": AT_FUTEX,
        "code": r"pub\(super\) unsafe fn remove_waiter",
        "invariant": (
            "Caller contract (valid + in-list). Callers guarded by "
            "is_linked under the lock (wait, timeout job)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-remove-cursor",
        "file": AT_FUTEX,
        "code": r"let node = unsafe \{",
        "invariant": (
            "Cursor removal under the contract; None panics "
            "(fail-closed, not UB)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-remove-drop",
        "file": AT_FUTEX,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 358,
        "invariant": (
            "Async entries only; cursor removal unlinked, exactly-once."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-wait-fn",
        "file": AT_FUTEX,
        "code": r"pub\(super\) unsafe fn wait<E: Element \+ PartialEq>",
        "invariant": (
            "Caller contract (aligned addr + size bytes). Callers: "
            "Atomics.wait (validate_atomic_access; E in {i32,i64})."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-wait-read",
        "file": AT_FUTEX,
        "code": r"E::read\(SliceRef::AtomicSlice\(buffer\)\)\.load\(Ordering::SeqCst\)",
        "invariant": (
            "Aligned (64-aligned base + multiple-of-size offset) and "
            "sized (validated index) under the fn contract."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # The four add/remove call blocks (lines-anchored; all verified).
        "id": "D3-addremove-call",
        "file": AT_FUTEX,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [423, 470, 551, 575],
        "invariant": (
            "wait add: fresh unlinked stack waiter, removed if "
            "linked. wait remove: is_linked-guarded. wait_async "
            "add: fresh Arc. timeout-job remove: upgrade+linked "
            "guarded under the lock."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-waitasync-fn",
        "file": AT_FUTEX,
        "code": r"pub\(super\) unsafe fn wait_async<E",
        "invariant": (
            "Caller contract (aligned addr + size bytes). Callers: "
            "Atomics.waitAsync (validated; E in {i32,i64}). Buffer "
            "cloned into the waiter (outlives the call)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-waitasync-read",
        "file": AT_FUTEX,
        "code": r"E::read\(SliceRef::AtomicSlice\(buf\)\)\.load\(Ordering::SeqCst\)",
        "invariant": (
            "Aligned (64-aligned base + multiple-of-size offset) and "
            "sized (validated index) under the fn contract."
        ),
        "covering_test": BUF_TESTS,
    },
    # Family D3b: Atomics builtins. Every unsafe block is guarded by
    # validate_atomic_access (index < length => in-bounds) plus the
    # integer-indexed alignment guarantee (64-aligned base +
    # multiple-of-size offsets), on a non-detached buffer (checked).
    {
        # RMW dispatch + compare_exchange (both verified).
        "id": "D3-rmw-body",
        "file": AT_MOD,
        "code": r"let value: TypedArrayElement = unsafe \{",
        "invariant": (
            "Aligned + in-bounds (validated, non-detached); match "
            "arms pair each kind with its exact element type."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-load-body",
        "file": AT_MOD,
        "code": r"data\.get_value\(access\.kind, Ordering::SeqCst\)",
        "invariant": "Aligned + in-bounds (validated, non-detached).",
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D3-store-body",
        "file": AT_MOD,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 259,
        "invariant": "Aligned + in-bounds (validated, non-detached).",
        "covering_test": BUF_TESTS,
    },
    {
        # Async + sync wait calls (both verified).
        "id": "D3-wait-call",
        "file": AT_MOD,
        "code": r"let result = unsafe \{",
        "invariant": (
            "Addr validated (multiple-of-size, in-bounds); E=i32/i64 "
            "by kind branch; buffer borrow alive for the call "
            "(async path clones into the waiter)."
        ),
        "covering_test": BUF_TESTS,
    },
    # Family D4: buffer call sites (typed_array/dataview/shared). Bounds
    # come from validated indices + debug asserts; alignment from the
    # 64-aligned base + multiple-of-size offsets (dataview uses memcpy,
    # alignment-free by design); disjointness from fresh allocations or
    # the spec's same-buffer clone (set() step 19).
    {
        "id": "D4-slice-copy",
        "file": AB_SHARED,
        "code": r"copy_shared_to_shared\(from_buf\.as_ptr\(\), to_buf\.as_ptr\(\), new_len\)",
        "invariant": (
            "Source range in-bounds (get_slice_range + debug assert); "
            "target fresh (disjoint) with >= new_len capacity."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D4-box-conv",
        "file": AB_SHARED,
        "code": r"^Ok\(unsafe \{$",
        "invariant": (
            "u8 box parts -> AlignedBox<[AtomicU8]>: transparent "
            "cast (same size/align); align+len preserved; ownership "
            "moved from into_raw_parts (no double free)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D4-dv-get",
        "file": DV_MOD,
        "code": r"let value: TypedArrayElement = unsafe \{",
        "invariant": (
            "Range checked (getIndex+size <= viewSize); memcpy from "
            "the buffer into a stack T (exact size); disjoint; "
            "alignment-free by design (byte copy)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D4-dv-set",
        "file": DV_MOD,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 848,
        "invariant": (
            "Range checked; memcpy from a stack T into the buffer; "
            "disjoint; alignment-free by design."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # The four bulk-copy blocks (lines-anchored; text names each).
        "id": "D4-ta-bulk",
        "file": TA_BUILTIN,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [613, 1879, 2163, 2186],
        "invariant": (
            "copyWithin memmove: asserted from/to ranges; memmove "
            "overlap-safe. set() memcpy: asserted counts; same-"
            "buffer cloned first (disjoint). slice memmove_naive: "
            "asserted ranges; naive order intended. slice memcpy: "
            "asserted src range; target freshly allocated "
            "(disjoint); .add offsets in-bounds."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # set()/from element-get loop bodies (both verified).
        "id": "D4-ta-loop-get",
        "file": TA_BUILTIN,
        "code": r"let value = unsafe \{",
        "invariant": (
            "Indices advance within validated limits (limit/count "
            "bounds); each subslice in-bounds + aligned."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        # set()/from element-set loop bodies (lines-anchored).
        "id": "D4-ta-loop-set",
        "file": TA_BUILTIN,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [1909, 2987],
        "invariant": (
            "Indices advance within validated limits; fresh/checked "
            "targets in-bounds + aligned."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D4-ta-idx-get",
        "file": TA_OBJECT,
        "code": r"let value = unsafe \{",
        "invariant": (
            "Index validated (is_valid_integer_index); buffer "
            "aligned (integer-indexed guarantee)."
        ),
        "covering_test": BUF_TESTS,
    },
    {
        "id": "D4-ta-idx-set",
        "file": TA_OBJECT,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 770,
        "invariant": (
            "Index validated (validate_index); buffer aligned; "
            "non-detached (checked)."
        ),
        "covering_test": BUF_TESTS,
    },
    # Family E: Trace-bypass audit (129 sites). Semantics (boa_macros):
    # unsafe_empty_trace = empty trace impl (sound iff the type reaches
    # no Gc); unsafe_no_drop = normal field tracing, no Drop glue (sound:
    # every no_drop type uses the derived EMPTY Finalize and has no
    # manual Drop/Finalize — verified by grep — so the glue is a no-op);
    # unsafe_ignore_trace = skip this field (sound iff the field reaches
    # no Gc). Backstop theorem (collect/trace_non_roots): an untraced Gc
    # handle keeps its target ROOTED (refcount exceeds the heap count),
    # so even a Gc-containing bypass is a bounded leak, never UAF; only
    # two bypasses contain Gc (E-gc-caveat), both analyzed leak-free in
    # practice (owner-drop releases; cycles keep a traced back-edge).
    # NOTE: these rules come after all file-scoped rules, so the earlier
    # B-gctest-bypass keeps its two test sites (first-match-wins).
    {
        # All 27 no_drop sites (uniform justification, verified).
        "id": "E-nodrop",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"#\[boa_gc\(unsafe_no_drop\)\]",
        "invariant": (
            "Skips only the Drop glue (field tracing still derived); "
            "sound because Finalize is derived-empty and no manual "
            "Drop/Finalize exists (grep-verified), so the glue is a "
            "no-op; refcounting runs via fields' own Drops."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # boa_ast-rooted targets (boa_ast has no boa_gc dep: cannot
        # name Gc, sound by crate independence).
        "id": "E-ast",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"boa_ast::(Module|Script|scope::Scope)",
        "invariant": (
            "boa_ast types cannot reach Gc (no boa_gc dependency)."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # Bare-Scope/BindingLocator/CapturedBinding (all boa_ast imports,
        # verified at each site).
        "id": "E-ast-scope",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(=> (compile|scope): Scope,|Scope\(#\[unsafe_ignore_trace\] Scope\)|BindingLocator|CapturedBinding)",
        "invariant": (
            "boa_ast::scope types (Scope tree, locators, captured "
            "bindings): JsString/u32 data, no Gc (crate independence)."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # JsBigInt: the empty_trace site + the inline field site.
        "id": "E-bigint",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"JsBigInt",
        "invariant": "Rc<num_bigint::BigInt>: arbitrary precision ints.",
        "covering_test": GC_TESTS_E,
    },
    {
        # Rc<Cell<..>> promise counters.
        "id": "E-rc-cell",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"Rc<Cell<(bool|i32)>>",
        "invariant": "Shared primitive counters; no Gc.",
        "covering_test": GC_TESTS_E,
    },
    {
        # Primitive cells/flags/containers.
        "id": "E-prim",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(RefCell<Vec<bool>>|(Thin)?Vec<Option<u32>>|Cell<(bool|CodeBlockFlags)>|depth: u32|ignore_bom: bool|CallFrameFlags|Rc<RefCell<Vec<PropertyKey>>>)",
        "invariant": (
            "Primitives (bool/u32), primitive containers, bitflags, "
            "and PropertyKey vectors (PropertyKey is GC-free, E-leaf)."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # Fieldless/small enums.
        "id": "E-enum",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(PropertyNameKind|ReactionType|KeyedVariant|AsyncGeneratorState|IterationKind|ResponseType|UrlSearchParamsIteratorKind)",
        "invariant": "Fieldless marker enums; no data, no Gc.",
        "covering_test": GC_TESTS_E,
    },
    {
        # JsString-keyed sets and string/index accessors.
        "id": "E-strkeys",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(IndexSet<JsString|FxHashSet<JsString|BindingAccessor)",
        "invariant": (
            "JsString (refcounted, non-GC) collections; "
            "BindingAccessor is JsString/u32."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # Bytecode/VM metadata.
        "id": "E-vmmeta",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(Bytecode|ThinVec<Handler>|slot: Slot|ShadowEntry|Backtrace|IgnoreEq<ErrorStack>|ModuleCode|PropertyTable|task::Waker|InternalObjectMethods)",
        "invariant": (
            "VM metadata (bytecode bytes, handler ranges, slots, "
            "shadow entries, backtraces, module records, property "
            "tables of PropertyKey/Slot, wakers, vtable fns); no Gc."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # External/runtime leaf types (external crates cannot name Gc;
        # std/sync primitives hold no JS data).
        "id": "E-external",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(AlignedVec<u8>|data: Arc<Inner>|reqwest::blocking::Client|HttpHeaderMap|HttpRequest<Vec<u8>>|Rc<Vec<u8>>|requests_received|request_mapper|encoding: Encoding|SharedUrl|CancellationToken|async_channel::(Weak)?Sender<\(\)>|UnboundedSender<JsValueStore>|FetcherRc|MessageSenderRc|Option<Rc<Inner>>)",
        "invariant": (
            "External-crate data (buffers, http, url, reqwest, "
            "encoding, tokio), std/sync primitives, channels of "
            "unit/GC-free stores, host-trait Rc wrappers (in-tree "
            "impls hold no Gc; never heap-boxed), AST source text."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # Temporal (temporal_rs inners) + RegExp (regress Regex).
        "id": "E-empty-temporal",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"pub struct (Duration|Instant|PlainDate|PlainDateTime|PlainMonthDay|PlainTime|PlainYearMonth|ZonedDateTime|RegExp) \{",
        "invariant": (
            "External-crate inners (temporal_rs calendar data, "
            "regress matcher) + JsStrings; cannot name Gc."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # icu-based intl formatters (DateTimeFormat separate: E-gc-caveat).
        "id": "E-empty-icu",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"pub\(crate\) struct (ListFormat|PluralRules|Segmenter) \{",
        "invariant": "icu_* formatter state (locales, compiled rules).",
        "covering_test": GC_TESTS_E,
    },
    {
        # GC-free leaf types with empty trace.
        "id": "E-leaf",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(pub struct (Intl|JsSymbol)|pub enum PropertyKey|pub\(crate\) struct SourceInfo)",
        "invariant": (
            "Intl (JsSymbol), PropertyKey (JsString/JsSymbol/u32), "
            "JsSymbol (Arc<u64/bytes>), SourceInfo (Rc of source "
            "map+text). No Gc."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # The two Gc-containing bypasses (sound by rooted conservatism;
        # analyzed leak-free in practice; see family note).
        "id": "E-gc-caveat",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"(resolved_bindings: FxHashMap<JsString, ResolvedBinding>|pub\(crate\) struct DateTimeFormat)",
        "invariant": (
            "CAVEAT (sound, not minimal): skips live Gc handles "
            "(Module refs; bound_format/resolved_options caches). "
            "Targets stay rooted while the owner lives (never UAF); "
            "released on owner drop; garbage cycles keep a traced "
            "back-edge, so they still collect."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # Closure fields (Copy-or-caller-contract design, documented).
        "id": "E-closure",
        "file": ANY_RS,
        "kind": "trace_bypass",
        "code": r"=> f: F,",
        "invariant": (
            "Safe constructors take Copy closures only (cannot "
            "capture Gc: !Copy); unsafe constructors document the "
            "no-traceable-captures contract (UB warning)."
        ),
        "covering_test": GC_TESTS_E,
    },
    {
        # Macro implementation (attribute-name parsing), not bypasses.
        "id": "E-macro-notbypass",
        "file": "core/macros/src/lib\\.rs",
        "kind": "trace_bypass",
        "invariant": (
            "Not a bypass: derive-macro implementation code that "
            "parses/matches the attribute names (strings/idents)."
        ),
        "covering_test": "boa_macros unit tests (trybuild/expand)",
    },
    # Family F1: opcode operand decoding. All 22 Readable impls are
    # padding-free any-bit-pattern types (ints, floats, tuples), read
    # via read_unaligned (alignment-free). read() asserts bounds
    # (fail-closed panic); try_read checks (None); both are the only
    # read_unchecked callers.
    {
        "id": "F-args-trait",
        "file": VM_ARGS,
        "code": r"unsafe trait Readable",
        "invariant": "Implementor contract: safely readable from bytes.",
        "covering_test": VM_TESTS,
    },
    {
        # All 22 impls (verified: padding-free, any-bits-valid).
        "id": "F-args-impl",
        "file": VM_ARGS,
        "code": r"unsafe impl Readable for",
        "invariant": (
            "Ints/floats/tuples thereof: no padding, every bit "
            "pattern valid; read via read_unaligned."
        ),
        "covering_test": VM_TESTS,
    },
    {
        # read() + try_read() calls (both bounds-established).
        "id": "F-args-read",
        "file": VM_ARGS,
        "code": r"let result = unsafe \{ read_unchecked\(bytes, offset\) \};",
        "invariant": (
            "read(): assert(len >= offset+size) above. try_read(): "
            "checked_add + len check above."
        ),
        "covering_test": VM_TESTS,
    },
    {
        "id": "F-args-readunchecked-fn",
        "file": VM_ARGS,
        "code": r"unsafe fn read_unchecked<T: Readable>",
        "invariant": (
            "Caller contract (in-bounds). Sole callers read/try_read "
            "(asserted/checked)."
        ),
        "covering_test": VM_TESTS,
    },
    {
        "id": "F-args-readunchecked-body",
        "file": VM_ARGS,
        "code": r"bytes\.as_ptr\(\)\.add\(offset\)\.cast::<T>\(\)\.read_unaligned\(\)",
        "invariant": (
            "In-bounds pointer (contract); unaligned read of a "
            "padding-free any-bits type."
        ),
        "covering_test": VM_TESTS,
    },
    # Family F2: JsSymbol (Arc misuse-proof tagging). Ptr arm holds one
    # Arc::into_raw unit (Clone +1 via clone+forget, Drop -1 via
    # from_raw+drop, into_raw/from_raw move it to nan_boxed); Tag arm
    # holds only WellKnown discriminants (sole constructor: the macro),
    # so from_tag round-trips. Shared payload is Sync (u64 + Box).
    {
        "id": "F-sym-send",
        "file": ENG_SYM,
        "code": r"unsafe impl Send for JsSymbol",
        "invariant": "Arc-refcounted immutable payload; Tag is data.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-sync",
        "file": ENG_SYM,
        "code": r"unsafe impl Sync for JsSymbol",
        "invariant": "Shared payload Sync (u64 + Box<[u16]>).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-new",
        "file": ENG_SYM,
        "code": r"Tagged::from_ptr\(Arc::into_raw\(arc\)\.cast_mut\(\)\)",
        "invariant": (
            "into_raw never null; aligned (RawJsSymbol align 8); "
            "unit owned by the new JsSymbol (Drop balances)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-desc",
        "file": ENG_SYM,
        "code": r"ptr\.as_ref\(\)\.description\.as_ref\(\)",
        "invariant": "Arc unit held: pointer valid for shared reads.",
        "covering_test": ENG_TESTS,
    },
    {
        # All three tag round-trips (same proof).
        "id": "F-sym-wellknown",
        "file": ENG_SYM,
        "code": r"WellKnown::from_tag\(tag\)\.unwrap_unchecked\(\)",
        "invariant": (
            "Tags constructed only from WellKnown hashes (sole "
            "constructor: the macro); from_tag round-trips."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-hash",
        "file": ENG_SYM,
        "code": r"ptr\.as_ref\(\)\.hash",
        "invariant": "Arc unit held: pointer valid for shared reads.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-fromraw-fn",
        "file": ENG_SYM,
        "code": r"pub\(crate\) unsafe fn from_raw\(ptr: NonNull<RawJsSymbol>\)",
        "invariant": (
            "Caller contract (into_raw pairing). Sole users: "
            "nan_boxed JsValue codec (balanced, family A)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-clone",
        "file": ENG_SYM,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 327,
        "invariant": (
            "from_raw on the valid Arc ptr + clone + forget both: "
            "net +1 unit for the new handle."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-sym-drop",
        "file": ENG_SYM,
        "code": r"drop\(Arc::from_raw\(ptr\.as_ptr\(\).cast_const\(\)\)\)",
        "invariant": (
            "Reconstitutes the owned unit and drops it (-1); "
            "exactly-once (Drop runs once; into_raw uses "
            "ManuallyDrop)."
        ),
        "covering_test": ENG_TESTS,
    },
    # Family F3a: host-closure interop. The unsafe trait documents
    # "must not contain GC objects"; the 4 impls forward under that
    # contract (host UB if violated — standard unsafe-API design).
    {
        # All four impl fns (same contract).
        "id": "F-interop-fn",
        "file": ENG_INTEROP,
        "code": r"unsafe fn into_js_function_unsafe",
        "invariant": (
            "Trait contract (documented # Safety): the host closure "
            "holds no GC objects. Forwards to from_closure under it."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        # All four impl bodies (lines-anchored; same proof).
        "id": "F-interop-body",
        "file": ENG_INTEROP,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [53, 79, 105, 127],
        "invariant": (
            "Wraps the host closure in RefCell (recursion guard) "
            "and calls from_closure under the trait contract."
        ),
        "covering_test": ENG_TESTS,
    },
    # Family F3b: NativeFunction. Trace impls are complete (marks the
    # Gc closure + realm; fn pointers need no tracing). Safe
    # constructors pass Copy closures (cannot capture Gc); unsafe ones
    # document the contract; into_raw/from_raw coercion moves the unit.
    {
        "id": "F-natobj-trace",
        "file": ENG_NATFN,
        "code": r"unsafe impl Trace for NativeFunctionObject",
        "invariant": (
            "Complete: marks f + realm; skips JsString name + "
            "constructor enum (GC-free)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-natfn-trace",
        "file": ENG_NATFN,
        "code": r"unsafe impl Trace for NativeFunction",
        "invariant": "Marks the Closure arm's Gc; PointerFn needs none.",
        "covering_test": ENG_TESTS,
    },
    {
        # The two Copy-constructor calls (both verified).
        "id": "F-natfn-copy",
        "file": ENG_NATFN,
        "code": r"unsafe \{ Self::from_closure",
        "invariant": (
            "Copy closures only (bound): cannot capture Gc (!Copy); "
            "contract holds by construction."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-natfn-closure-fn",
        "file": ENG_NATFN,
        "code": r"pub unsafe fn from_closure<F>",
        "invariant": (
            "Caller contract (documented # Safety + Caveats): no "
            "traceable captures."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        # The two unsafe-constructor bodies (lines-anchored).
        "id": "F-natfn-closure-body",
        "file": ENG_NATFN,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [260, 289],
        "invariant": (
            "from_closure body: delegates with () captures. "
            "from_closure_with_captures body: into_raw unit "
            "immediately re-wrapped by from_raw (unsized "
            "coercion; no count change, no drop between)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-natfn-closure-caps-fn",
        "file": ENG_NATFN,
        "code": r"pub unsafe fn from_closure_with_captures<F, T>",
        "invariant": (
            "Caller contract (documented # Safety + Caveats): no "
            "traceable captures (captures: T go through Trace)."
        ),
        "covering_test": ENG_TESTS,
    },
    # Family F3c: JsObject casts. Object<T>/ObjectData<T> are repr(C)
    # (documented for casting) with the T-dependent data LAST, so the
    # common prefix is layout-identical across T; erased data access is
    # ZST-only. Every data downcast is is::<T>()-checked except the
    # contract-carrying downcast_unchecked; raw pairing is nan_boxed's.
    {
        "id": "F-jsobj-fromraw-fn",
        "file": ENG_JSOBJ,
        "code": r"pub\(crate\) unsafe fn from_raw\(raw: NonNull<GcBox<ErasedVTableObject>>\)",
        "invariant": (
            "Caller contract (into_raw pairing). Sole users: "
            "nan_boxed codec (balanced, family A)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-fromraw-body",
        "file": ENG_JSOBJ,
        "code": r"let inner = unsafe \{ Gc::from_raw\(raw\) \};",
        "invariant": "Re-wraps the into_raw unit under the contract.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-downcast-checked",
        "file": ENG_JSOBJ,
        "code": r"let object = unsafe \{ self\.downcast_unchecked::<T>\(\) \};",
        "invariant": "Guarded by is::<T>() above.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-downcast-fn",
        "file": ENG_JSOBJ,
        "code": r"pub unsafe fn downcast_unchecked<T: NativeObject>",
        "invariant": (
            "Caller contract (documented # Safety): inner data is T. "
            "In-tree callers check is::<T>() first."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-downcast-body",
        "file": ENG_JSOBJ,
        "code": r"Gc::cast_unchecked::<VTableObject<T>>\(self\.inner\)",
        "invariant": (
            "Handle reinterpretation under the contract; layout "
            "compatible (repr(C) prefix)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-refcast",
        "file": ENG_JSOBJ,
        "code": r"GcRef::cast::<Object<T>>\(obj\)",
        "invariant": "Guarded by is::<T>(); borrow guard moved.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-refmutcast",
        "file": ENG_JSOBJ,
        "code": r"GcRefMut::cast::<Object<T>>\(obj\)",
        "invariant": "Guarded by is::<T>(); borrow guard moved.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-jsobj-upcast",
        "file": ENG_JSOBJ,
        "code": r"Gc::cast_unchecked::<ErasedVTableObject>\(self\.inner\)",
        "invariant": (
            "Upcast to erased: repr(C) common prefix identical; "
            "erased data access is ZST-only."
        ),
        "covering_test": ENG_TESTS,
    },
    # Family F3d: macro templates (quote! blocks generating unsafe code
    # at user sites; justified by construction + application audit).
    {
        "id": "F-macro-sym",
        "file": MACROS_LIB,
        "code": r"Self::new_unchecked\(#idx\)",
        "invariant": (
            "Template: idx is enumerate()+1, always >= 1 (nonzero "
            "contract holds by construction)."
        ),
        "covering_test": "boa_interner unit tests",
    },
    {
        # Empty-trace template fns (same justification).
        "id": "F-macro-empty-tmpl",
        "file": MACROS_LIB,
        "code": r"unsafe fn (trace\(&self, _tracer: &mut ::boa_gc::Tracer\)|trace_non_roots\(&self\)) \{\}",
        "invariant": (
            "Template generating the empty impl; each application "
            "audited for GC-freeness (Family E)."
        ),
        "covering_test": "boa_macros unit tests (trybuild/expand)",
    },
    {
        "id": "F-macro-trace-fn",
        "file": MACROS_LIB,
        "code": r"unsafe fn trace\(&self, tracer: &mut ::boa_gc::Tracer\) \{",
        "invariant": (
            "Template: delegates to each field's Trace (bounds "
            "added); per-field skips audited (Family E)."
        ),
        "covering_test": "boa_macros unit tests (trybuild/expand)",
    },
    {
        # The three mark-delegation template blocks.
        "id": "F-macro-trace-body",
        "file": MACROS_LIB,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [378, 389, 400],
        "invariant": (
            "Template: calls the field's trace/trace_non_roots/"
            "run_finalizer (implementor contract, standard)."
        ),
        "covering_test": "boa_macros unit tests (trybuild/expand)",
    },
    {
        "id": "F-macro-nonroots-fn",
        "file": MACROS_LIB,
        "code": r"unsafe fn trace_non_roots\(&self\) \{",
        "invariant": "Template: delegates to each field (bounds added).",
        "covering_test": "boa_macros unit tests (trybuild/expand)",
    },
    # Family F4a: synthetic module initializers (mirrors NativeFunction:
    # Copy ctors uphold the contract by construction; unsafe ctors
    # document it; into_raw/from_raw coercion moves the unit).
    {
        # The two Copy-constructor calls (both verified).
        "id": "F-syn-copy",
        "file": ENG_SYN,
        "code": r"unsafe \{ Self::from_closure",
        "invariant": "Copy closures only: cannot capture Gc (!Copy).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-syn-closure-fn",
        "file": ENG_SYN,
        "code": r"pub unsafe fn from_closure<F>",
        "invariant": "Caller contract (documented # Safety).",
        "covering_test": ENG_TESTS,
    },
    {
        # The two unsafe-constructor bodies (lines-anchored).
        "id": "F-syn-closure-body",
        "file": ENG_SYN,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [99, 129],
        "invariant": (
            "from_closure body: delegates with () captures. "
            "from_closure_with_captures body: into_raw unit "
            "immediately re-wrapped (unsized coercion)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-syn-closure-caps-fn",
        "file": ENG_SYN,
        "code": r"pub unsafe fn from_closure_with_captures<F, T>",
        "invariant": "Caller contract (documented # Safety).",
        "covering_test": ENG_TESTS,
    },
    # Family F4b: NativeObject downcasts (dyn -> T via data pointer;
    # correct under TypeId equality; all in-tree callers check).
    {
        # The two checked wrappers (both verified).
        "id": "F-objmod-checked",
        "file": ENG_OBJMOD,
        "code": r"unsafe \{ Some\(self\.downcast_(ref|mut)_unchecked\(\)\) \}",
        "invariant": "Guarded by is::<T>() (TypeId equality).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-objmod-ref-fn",
        "file": ENG_OBJMOD,
        "code": r"pub unsafe fn downcast_ref_unchecked<T: NativeObject>",
        "invariant": (
            "Caller contract (documented # Safety): contained value "
            "is T (debug_asserted). Sole callers check is::<T>()."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-objmod-ref-body",
        "file": ENG_OBJMOD,
        "code": r"unsafe \{ &\*ptr\.cast::<T>\(\) \}",
        "invariant": (
            "Trait-object data pointer cast to T (correct under "
            "TypeId equality); shared borrow preserved."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-objmod-mut-fn",
        "file": ENG_OBJMOD,
        "code": r"pub unsafe fn downcast_mut_unchecked<T: NativeObject>",
        "invariant": (
            "Caller contract (documented # Safety). Sole callers "
            "check is::<T>()."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-objmod-mut-body",
        "file": ENG_OBJMOD,
        "code": r"unsafe \{ &mut \*ptr\.cast::<T>\(\) \}",
        "invariant": "Data-pointer cast under TypeId equality (&mut).",
        "covering_test": ENG_TESTS,
    },
    # Family F4c: unsafe-fn-pointer JsData impls (declarations only).
    {
        # All five fn_one! lines (same justification).
        "id": "F-datatypes-fnptr",
        "file": ENG_DATATYPES,
        "kind": "unsafe_extern",
        "code": r"fn_one!\(unsafe extern",
        "invariant": (
            "Declares JsData impls for unsafe-fn-pointer TYPES; no "
            "unsafe code runs (call sites are separately checked)."
        ),
        "covering_test": ENG_TESTS,
    },
    # Family F4d: NonMaxU32 witnesses (each value provably != MAX).
    {
        # All five sites (each verified; text names each).
        "id": "F-prop-nonmax",
        "file": ENG_PROP,
        "kind": "unsafe_block",
        "code": r"NonMaxU32::new_unchecked",
        "invariant": (
            "Literal 0; <10-char parse (< 10^9, no overflow); u8/u16 "
            "widenings; non-negative i32 (31 bits). None is MAX."
        ),
        "covering_test": ENG_TESTS,
    },
    # Family F4e: VM hot paths. Register indices come from the
    # compiler for Boa-emitted bytecode (P5 validity model) with
    # debug_assert bounds checks; frames always holds the dummy frame
    # (pop_frame refuses index 0; construction pushes it).
    {
        "id": "F-vm-trace",
        "file": ENG_VM,
        "code": r"unsafe impl Trace for ActiveRunnable",
        "invariant": "Complete: marks both arms (Script, Module).",
        "covering_test": VM_TESTS,
    },
    {
        "id": "F-vm-setreg",
        "file": ENG_VM,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 469,
        "invariant": (
            "Register write: compiler-emitted index in-bounds "
            "(validity model; debug-asserted)."
        ),
        "covering_test": VM_TESTS,
    },
    {
        "id": "F-vm-getreg",
        "file": ENG_VM,
        "code": r"self\.stack\.stack\.get_unchecked\(rp \+ index\)",
        "invariant": "Register read: in-bounds (model; debug-asserted).",
        "covering_test": VM_TESTS,
    },
    {
        "id": "F-vm-takereg",
        "file": ENG_VM,
        "code": r"std::mem::take\(self\.stack\.stack\.get_unchecked_mut\(rp \+ index\)\)",
        "invariant": (
            "Register take: in-bounds; leaves valid undefined."
        ),
        "covering_test": VM_TESTS,
    },
    {
        # frame() + frame_mut() (same invariant).
        "id": "F-vm-frame",
        "file": ENG_VM,
        "code": r"self\.frames\.last\(\)\.unwrap_unchecked\(\)",
        "invariant": "Frames never empty (dummy frame pinned).",
        "covering_test": VM_TESTS,
    },
    {
        "id": "F-vm-framemut",
        "file": ENG_VM,
        "code": r"self\.frames\.last_mut\(\)\.unwrap_unchecked\(\)",
        "invariant": "Frames never empty (dummy frame pinned).",
        "covering_test": VM_TESTS,
    },
    # Family F4f: string interner. InternedStr validity is established
    # by its sole (unsafe) constructor and preserved by the
    # head/full retention protocol (old heads kept alive; statics
    # never invalidate), so eq/hash/index are sound. (P6.1: fixed the
    # misleading safe-fn SAFETY comments to cite construction.)
    {
        "id": "F-istrn-new-fn",
        "file": INT_INTERNED,
        "code": r"pub\(super\) const unsafe fn new\(ptr: NonNull<\[Char\]>\)",
        "invariant": (
            "Caller contract (documented # Safety): valid + "
            "outliving pointer. Callers: live slices, statics, "
            "interner heads."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-istrn-asref-fn",
        "file": INT_INTERNED,
        "code": r"pub\(super\) unsafe fn as_ref\(&self\)",
        "invariant": "Caller contract: upholds the struct invariants.",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-istrn-asref-body",
        "file": INT_INTERNED,
        "code": r"unsafe \{ self\.ptr\.as_ref\(\) \}",
        "invariant": "Derefs under the constructor contract.",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-istrn-eq",
        "file": INT_INTERNED,
        "code": r"unsafe \{ self\.as_ref\(\) == other\.as_ref\(\) \}",
        "invariant": (
            "Both values uphold the constructor invariant (only "
            "construction path); comparison reads valid memory."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-istrn-hash",
        "file": INT_INTERNED,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": "Hashes valid memory (constructor invariant).",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-get",
        "file": INT_RAW,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 74,
        "invariant": (
            "Probe InternedStr borrows the live query slice (valid "
            "for the immediate hash+eq)."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-static",
        "file": INT_RAW,
        "code": r"let string = unsafe \{ InternedStr::new\(string\.into\(\)\) \};",
        "invariant": "'static slice: valid forever.",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-static-idx",
        "file": INT_RAW,
        "code": r"unsafe \{ self\.next_index\(string\) \}",
        "invariant": "Static memory never invalidated.",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-index",
        "file": INT_RAW,
        "code": r"ptr\.as_ref\(\)",
        "invariant": (
            "Spans entries point into retained heads/full or "
            "statics (always alive)."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-nextindex-fn",
        "file": INT_RAW,
        "code": r"unsafe fn next_index\(&mut self, string: InternedStr<Char>\)",
        "invariant": (
            "Caller contract (head/statics memory). Callers: "
            "intern_static (static), intern (retained head)."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-intern",
        "file": INT_RAW,
        "kind": "unsafe_block",
        "code": r"let interned_str = unsafe \{",
        "invariant": (
            "Head-growth protocol: old heads retained in full "
            "(existing pointers stay valid); new pointer into the "
            "live head."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-raw-intern-idx",
        "file": INT_RAW,
        "code": r"unsafe \{ self\.next_index\(interned_str\) \}",
        "invariant": "Head memory retained for interner lifetime.",
        "covering_test": INT_TESTS,
    },
    # Family F4g: test262 $262 agent (harness-only closures capturing
    # channels/bus/handles; no GC objects captured).
    {
        # All five harness closures (each verified GC-free).
        "id": "F-t262-agent",
        "file": RT_TEST262,
        "kind": "unsafe_block",
        "code": r"let (start|broadcast|get_report|receive_broadcast|report) = unsafe \{",
        "invariant": (
            "Host closures capture bus/channels/handles only "
            "(Rc/RefCell/mpsc, thread spawns); no Gc captured; "
            "contract holds."
        ),
        "covering_test": RT_TESTS,
    },
    # Family F5a: manual Trace impls (each checked complete: every Gc
    # field marked, every skipped field GC-free).
    {
        "id": "F-trace-fielddef",
        "file": ENG_FUNCST,
        "code": r"unsafe impl Trace for ClassFieldDefinition",
        "invariant": "Marks JsFunction in both arms (keys GC-free).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-genstate",
        "file": ENG_GEN,
        "code": r"unsafe impl Trace for GeneratorState",
        "invariant": "Marks context in both suspended arms.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-collator",
        "file": ENG_COLL,
        "code": r"unsafe impl Trace for Collator",
        "invariant": "Marks bound_compare (rest: icu + primitives).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-numfmt",
        "file": ENG_NUMFMT,
        "code": r"unsafe impl Trace for NumberFormat",
        "invariant": "Marks bound_format (rest: icu + primitives).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-ordmap",
        "file": ENG_ORDMAP,
        "code": r"unsafe impl<V: Trace> Trace for OrderedMap<V>",
        "invariant": "Marks Key arms + all values (Empty: usize).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-promstate",
        "file": ENG_PROM,
        "code": r"unsafe impl Trace for PromiseState",
        "invariant": "Marks Fulfilled/Rejected values.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-resolving",
        "file": ENG_PROM,
        "code": r"unsafe impl Trace for ResolvingFunctions",
        "invariant": "Marks resolve + reject.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-promcap",
        "file": ENG_PROM,
        "code": r"unsafe impl Trace for PromiseCapability",
        "invariant": "Marks promise + functions.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-ordset",
        "file": ENG_ORDSET,
        "code": r"unsafe impl Trace for OrderedSet",
        "invariant": "Marks Key arms (Empty: usize).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-thisbind",
        "file": ENG_THISB,
        "code": r"unsafe impl Trace for ThisBindingStatus",
        "invariant": "Marks Initialized value.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-privenv",
        "file": ENG_PRIVENV,
        "code": r"unsafe impl Trace for PrivateEnvironment",
        "invariant": "Empty: usize + Vec<JsString>.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-naterr",
        "file": ENG_ERR,
        "code": r"unsafe impl Trace for JsNativeError",
        "invariant": "Marks kind/cause/realm (message/stack GC-free).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-naterrkind",
        "file": ENG_ERR,
        "code": r"unsafe impl Trace for JsNativeErrorKind",
        "invariant": "Marks Aggregate errors (rest: unit variants).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-hostdef",
        "file": ENG_HOSTDEF,
        "code": r"unsafe impl<T: \?Sized \+ Trace> Trace for HostDefined<T>",
        "invariant": "Marks all map values (keys: TypeIds).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-transition",
        "file": ENG_SHAPE,
        "code": r"unsafe impl Trace for TransitionKey",
        "invariant": "Empty: PropertyKey + bitflags.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-shapeflags",
        "file": ENG_SHAPE,
        "code": r"unsafe impl Trace for ShapeFlags",
        "invariant": "Empty bitflags.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-enumvalue",
        "file": ENG_LEGACY,
        "code": r"unsafe impl Trace for EnumBasedValue",
        "invariant": "Marks Object arm (rest: primitives/non-GC).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-codeblockflags",
        "file": ENG_CODEBLK,
        "code": r"unsafe impl Trace for CodeBlockFlags",
        "invariant": "Empty bitflags.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-completion",
        "file": ENG_COMPL,
        "code": r"unsafe impl Trace for CompletionRecord",
        "invariant": "Marks all three arms.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-trace-sym",
        "file": INT_SYM,
        "code": r"unsafe impl Trace for Sym",
        "invariant": "Empty: NonZeroUsize.",
        "covering_test": INT_TESTS,
    },
    # Family F5b: remaining engine small sites.
    {
        "id": "F-bigint-intoraw",
        "file": ENG_BIGINT,
        "code": r"NonNull::new_unchecked\(Rc::into_raw\(self\.inner\)\.cast_mut\(\)\)",
        "invariant": "into_raw never null; unit moved (from_raw pairs).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-bigint-fromraw-fn",
        "file": ENG_BIGINT,
        "code": r"pub\(crate\) unsafe fn from_raw\(ptr: \*const RawBigInt\)",
        "invariant": (
            "Caller contract (into_raw pairing). Sole users: "
            "nan_boxed codec (balanced, family A)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-bigint-fromraw-body",
        "file": ENG_BIGINT,
        "code": r"inner: unsafe \{ Rc::from_raw\(ptr\) \},",
        "invariant": "Re-wraps the into_raw unit under the contract.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-date-ascii",
        "file": ENG_DATEU,
        "code": r"str::from_utf8_unchecked\(s\)",
        "invariant": "Guarded by is_ascii above (Latin1 arm).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-conv-asm",
        "file": ENG_CONV,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "invariant": (
            "aarch64+jsconv only: FJCVTZS with NaN checked first; "
            "register constraints valid (in vreg, out reg)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-str-toint",
        "file": ENG_STRMOD,
        "code": r"to_int_unchecked::<u32>",
        "invariant": "Integer, finite, in u32 range (checked above).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-str-codeunit",
        "file": ENG_STRMOD,
        "code": r"code_unit_at\(i as usize\)\.unwrap_unchecked\(\)",
        "invariant": "Index validated (0 <= i < len) by the guard.",
        "covering_test": ENG_TESTS,
    },
    {
        # slice() + substring() calls (both clamped + ordered).
        "id": "F-str-slice",
        "file": ENG_STRMOD,
        "code": r"JsString::slice_unchecked\(&string, from, to\)",
        "invariant": "from/to clamped to len, from <= to (checked).",
        "covering_test": ENG_TESTS,
    },
    {
        # Both hex-pair reads (same proof).
        "id": "F-uri-hex",
        "file": ENG_URI,
        "code": r"let \(high, low\) = unsafe \{",
        "invariant": (
            "First: k+2 < len checked. Loop: k+3(n-1) < len "
            "lookahead covers k+1/k+2 for n in 2..=4."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-interop-decl",
        "file": ENG_INTEROPMOD,
        "code": r"unsafe fn into_js_function_unsafe\(self, context: &mut Context\)",
        "invariant": (
            "Trait decl: no-GC-captures contract (documented). "
            "Impls audited (F3a)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-mod-init-body",
        "file": ENG_MOD,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 784,
        "invariant": (
            "CAVEAT (sound, not minimal): captures Vecs of JsString "
            "+ NativeFunction (may hold Gc) under from_closure. "
            "Targets stay rooted while the module lives (never UAF); "
            "released on module drop; exports hold traced clones."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-mod-test-body",
        "file": ENG_MOD,
        "code": r"let module = unsafe \{",
        "invariant": (
            "Test-only: captures int counters (Copy path for the "
            "JsValue closure); contract holds."
        ),
        "covering_test": "module::into_js_module test",
    },
    {
        "id": "F-cont-copy",
        "file": ENG_CONT,
        "code": r"unsafe \{ Self::from_closure_with_captures\(closure, captures\) \}",
        "invariant": "Copy closure: cannot capture Gc (!Copy).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-cont-fn",
        "file": ENG_CONT,
        "code": r"pub\(crate\) unsafe fn from_closure_with_captures<F, T>",
        "invariant": "Caller contract (documented # Safety + UB warning).",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-cont-body",
        "file": ENG_CONT,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 116,
        "invariant": (
            "into_raw unit immediately re-wrapped (unsized "
            "coercion; no count change)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-nonmax-fn",
        "file": ENG_NONMAX,
        "code": r"pub const unsafe fn new_unchecked\(inner: u32\)",
        "invariant": (
            "Contract point (!= MAX; debug-asserted). Callers "
            "audited (F4d + checked ctor)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-nonmax-checked",
        "file": ENG_NONMAX,
        "code": r"Some\(Self::new_unchecked\(inner\)\)",
        "invariant": "Guarded by the != MAX check above.",
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-tadisp-get",
        "file": ENG_TADISP,
        "code": r"let element = unsafe \{",
        "invariant": (
            "Indices within array_length-verified bounds; aligned "
            "(typedarray invariant)."
        ),
        "covering_test": ENG_TESTS,
    },
    {
        "id": "F-proc-provider",
        "file": ENG_PROC,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 76,
        "invariant": (
            "CAVEAT (sound, not minimal): captures Rc<P: "
            "ProcessProvider> (generic; could hold Gc). In-tree P "
            "is unit. Any Gc stays rooted while the method lives "
            "(never UAF); released on drop."
        ),
        "covering_test": RT_TESTS,
    },
    # Family F5c: interner primitives.
    {
        "id": "F-fixed-push-fn",
        "file": INT_FIXED,
        "code": r"pub\(super\) unsafe fn push\(&mut self, string: &\[Char\]\)",
        "invariant": (
            "Caller contract (outlives). Capacity checked here; "
            "sole caller interner intern() retains heads."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-fixed-push-body",
        "file": INT_FIXED,
        "code": r"unsafe \{ self\.push_unchecked\(string\) \}",
        "invariant": "Capacity checked above (no realloc possible).",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-fixed-pushunchecked-fn",
        "file": INT_FIXED,
        "code": r"pub\(super\) unsafe fn push_unchecked\(&mut self, string: &\[Char\]\)",
        "invariant": (
            "Caller contract (outlives + capacity + align). "
            "Callers: push (checked) + interner intern()."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-fixed-pushunchecked-body",
        "file": INT_FIXED,
        "code": r"InternedStr::new\(ptr\.into\(\)\)",
        "invariant": "Live slice of the retained head (contract).",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-lib-get",
        "file": INT_LIB,
        "code": r"Sym::new_unchecked\(i \+ 1 \+ COMMON_STRINGS_UTF8\.len\(\)\)",
        "invariant": "Nonzero (>= 1); non-overflowing (upstream caps).",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-lib-utf8",
        "file": INT_LIB,
        "code": r"let utf8 = unsafe \{",
        "invariant": "utf8 interner stores only valid UTF-8 strs.",
        "covering_test": INT_TESTS,
    },
    {
        # Both common-string lookups (same proof).
        "id": "F-lib-common",
        "file": INT_LIB,
        "code": r"Sym::new_unchecked\(idx \+ 1\)",
        "invariant": "Nonzero (idx+1 >= 1); static length assert.",
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-sym-newunck-fn",
        "file": INT_SYM,
        "code": r"pub\(super\) const unsafe fn new_unchecked\(value: usize\)",
        "invariant": (
            "Contract point (nonzero). Callers: idx+1 forms "
            "(>= 1) + static_syms template (F3d)."
        ),
        "covering_test": INT_TESTS,
    },
    {
        "id": "F-sym-newunck-body",
        "file": INT_SYM,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 42,
        "invariant": "NonZeroUsize::new_unchecked under the contract.",
        "covering_test": INT_TESTS,
    },
    # Family F5d: parser lexer (ASCII-only buffers).
    {
        "id": "F-par-num",
        "file": PAR_NUM,
        "code": r"str::from_utf8_unchecked\(buf\.as_slice\(\)\)",
        "invariant": "Buffer holds ASCII digits/exponent/signs only.",
        "covering_test": PAR_TESTS,
    },
    {
        "id": "F-par-regex",
        "file": PAR_REGEX,
        "code": r"str::from_utf8_unchecked\(&buf\[..len\]\)",
        "invariant": "Buffer holds ASCII flag bytes only.",
        "covering_test": PAR_TESTS,
    },
    # Family F6: wintertc. Console/method closures mirror the process
    # pattern (generic hosts; in-tree impls GC-free; rooted backstop).
    # Store replace() publishes completed inners into pre-registered
    # (seen-map-cloned) fresh stores: thread-local construction, no
    # concurrent readers, Empty asserted (no drop-leak).
    {
        # console_method + console_method_mut (lines-anchored).
        "id": "F-wtc-console",
        "file": WTC_CONSOLE,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [366, 378],
        "invariant": (
            "Captures Rc<RefCell<Console>> (primitives/strings) + "
            "Rc<L: Logger> (in-tree: units; test logger holds a "
            "Gc cell: rooted while the method lives, released on "
            "drop — never UAF)."
        ),
        "covering_test": WTC_TESTS,
    },
    {
        # The four structured-clone publishes (lines-anchored).
        "id": "F-wtc-publish",
        "file": WTC_FROM,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [110, 204, 230, 292],
        "invariant": (
            "Fresh thread-local store (only clones: the local seen "
            "map, holding no live refs); Empty overwritten "
            "(asserted in replace)."
        ),
        "covering_test": WTC_TESTS,
    },
    {
        "id": "F-wtc-replace-fn",
        "file": WTC_STORE,
        "code": r"unsafe fn replace\(&mut self, other: ValueStoreInner\)",
        "invariant": (
            "Private; contract (documented # SAFETY): old inner "
            "Empty (asserted: no drop-leak) + creator discipline; "
            "raw write has no live-ref aliasing at call sites."
        ),
        "covering_test": WTC_TESTS,
    },
    {
        "id": "F-wtc-replace-body",
        "file": WTC_STORE,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "line": 177,
        "invariant": (
            "Non-null Arc ptr (asserted); Empty asserted; "
            "assignment publishes to the shared clones."
        ),
        "covering_test": WTC_TESTS,
    },
    {
        # P6.2 miri-test macro blocks (plain/shared/dispatch).
        "id": "F-miri-element-test",
        "file": TA_ELEMENT,
        "kind": "unsafe_block",
        "code": r"^unsafe \{$",
        "lines": [423, 435, 448],
        "invariant": (
            "Test-only: Element::read/read_mut + get/set_value "
            "over 8-byte 8-aligned fixtures (every element type "
            "fits); valid ranges by construction."
        ),
        "covering_test": "element::miri roundtrip tests (normal + Miri)",
    },
]


def apply_rule(site, rule):
    if not re.fullmatch(rule["file"], site["file"]):
        return False
    if "kind" in rule and rule["kind"] != site["kind"]:
        return False
    if "code" in rule and not re.search(rule["code"], site["code"]):
        return False
    # Optional exact line(s): only for code-identical blocks whose bodies
    # differ (fail-closed: source drift unmatches instead of mislabeling).
    if "line" in rule and rule["line"] != site["line"]:
        return False
    if "lines" in rule and site["line"] not in rule["lines"]:
        return False
    site["safety_status"] = "justified"
    site["invariant"] = rule["invariant"]
    site["covering_test"] = rule["covering_test"]
    site["owner"] = rule.get("owner", default_owner(site["file"]))
    site["audit_rule"] = rule["id"]
    return True


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: apply-unsafe-audit.py <fresh.json> <out.json>")
    inventory = json.load(open(sys.argv[1]))
    unmatched = []
    for site in inventory["sites"]:
        for rule in RULES:
            if apply_rule(site, rule):
                break
        else:
            unmatched.append(site)
    json.dump(inventory, open(sys.argv[2], "w"), indent=2)
    open(sys.argv[2], "a").write("\n")
    matched = len(inventory["sites"]) - len(unmatched)
    print(f"audit coverage: {matched}/{len(inventory['sites'])}")
    if unmatched:
        print(f"UNMATCHED ({len(unmatched)}):")
        for site in unmatched[:30]:
            print(f"  {site['file']}:{site['line']} [{site['kind']}] {site['code'][:100]}")
        sys.exit(1)


if __name__ == "__main__":
    main()
