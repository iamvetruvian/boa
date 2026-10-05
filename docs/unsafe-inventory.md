# Unsafe Inventory (P0.4)

Machine-readable inventory: [`unsafe-inventory.json`](unsafe-inventory.json)
(schema v1: `file`, `line`, `kind`, `code`, `safety_status`, `invariant`,
`covering_test`, `owner`). This document summarizes it and hands it to P6.

## Headline counts (scope: `core/*`)

| Kind | Count |
|---|---|
| `unsafe` blocks | 366 |
| `unsafe fn` | 123 |
| `unsafe impl` | 89 |
| `unsafe extern` (fn-type declarations inside macros — lowest priority) | 10 |
| `unsafe trait` (`Trace`, `Readable`) | 2 |
| **Total sites** | **590** |

| Crate | Sites | Concentration |
|---|---|---|
| `core/engine` | 276 | value codec, array buffers, atomics/futex, opcode args |
| `core/gc` | 175 | allocator, `Trace` impls, sweep/finalize, ephemerons |
| `core/string` | 92 | builder alloc/realloc, vtables, slice repr |
| `core/interner` | 23 | fixed-string bump storage |
| `core/macros` | 8 | `Trace`-derive template code (generated, not hand-written) |
| `core/runtime` | 6 | process + `$262` agent closures |
| `core/wintertc` | 8 | value store |
| `core/parser` | 2 | — |
| `core/ast`, `core/icu_provider` | 0 | clean |

Top files: `builtins/array_buffer/utils.rs` (73), `gc/trace.rs` (50),
`gc/lib.rs` (36), `string/builder.rs` (32), `value/inner/nan_boxed.rs` (28),
`vm/opcode/args.rs` (26), `gc/pointers/gc.rs` (22).

Every entry ships with `safety_status: "unjustified"` and null
invariant/test/owner fields. That is the correct P0 end state: anything
without a justification is a P6 work item by construction.

## Out of committed scope (informational)

- `utils/tag_ptr` (3 sites), `examples/src/bin/closures.rs` (1),
  `tests/tester/src/exec/mod.rs` (1) — 5 sites outside `core/*`, found with
  the same generator. `utils/small_btree`, `cli`, `benches`, `tests/wpt`,
  `tools/*`, `ffi/*` have zero sites.
- 131 `Trace`-bypass attributes (`#[unsafe_ignore_trace]` /
  `#[boa_gc(unsafe_empty_trace)]` / `#[boa_gc(unsafe_no_drop)]`) under
  `core/*` — not `unsafe` items, but each must prove its skipped field holds
  no live `Gc` in the P6 audit.

## Method and limitations

Generator: `scripts/generate-unsafe-inventory.py` (deterministic: sorted
output, stable schema). It matches `unsafe fn|impl|trait|extern` and
`unsafe {` per source line, skipping full-line `//` comments and matches that
only occur inside trailing comments. Cross-checked against raw `grep` counts
(123 fns, 91 impls+traits, ~369 block-mentioning lines incl. comments):
agreement is exact except for comment lines the generator correctly excludes.
Known limitations: an `unsafe` keyword split across two physical lines would
be missed (rustfmt never emits that shape); matches inside string literals on
code lines are counted (spot-checked: the sample contained none).

Regenerate: `python3 scripts/generate-unsafe-inventory.py core >
docs/unsafe-inventory.json`. P6 adds a CI check that fails when the
regenerated inventory differs from the committed one without audit sign-off.

## Handoff to P6 (unsafe audit)

1. For each of the 590 sites: write the `SAFETY` justification (allocation,
   alignment, lifetime, aliasing, soundness obligation), name covering tests,
   assign an owner; flip `safety_status` to `justified` or refactor the site
   into safe code.
2. Audit order follows the choke list: NaN-box codec → GC core/rooting →
   string/interner → array-buffer/atomics → the rest.
3. Extend the existing `unsafe`-lint denies (`core/string`, array-buffer,
   atomics, typed-array element) to `value/*` and `core/gc` so new
   unjustified `unsafe` fails CI; gate inventory deltas by diff.

## P6.1 audit completion (724/724 justified)

Pipeline: `scripts/generate-unsafe-inventory.py` (schema v3: bypass sites
carry the applied-to `// =>` context) + `scripts/apply-unsafe-audit.py`
(360 first-match-wins rules with `file`/`kind`/`code`/`line(s)` selectors).
The committed JSON is ALWAYS fully derived — manual edits forbidden:

```
python3 scripts/generate-unsafe-inventory.py core > /tmp/fresh.json
python3 scripts/apply-unsafe-audit.py /tmp/fresh.json docs/unsafe-inventory.json
```

CI (`unsafe_inventory` job in `rust.yml`) runs
`scripts/check-unsafe-inventory.sh`: fails unless the committed file equals
a fresh regen+apply AND coverage is 100% (the apply step exits nonzero
otherwise). Scope note: CI pins stable 1.94.0, so warn-level
`missing_safety_doc`/`undocumented_unsafe_blocks` were deliberately NOT added
(`-D warnings` on main would fail on pre-existing sites); the audit-coverage
gate is the enforcement instead.

Headline counts (scope: `core/*`, schema v3):

| Kind | Count |
|---|---|
| `unsafe` blocks | 371 |
| `unsafe fn` | 123 |
| `unsafe impl` | 89 |
| `trace_bypass` | 129 |
| `unsafe extern` (declarations only) | 10 |
| `unsafe trait` | 2 |
| **Total sites** | **724** |

| Crate | Sites |
|---|---|
| `core/engine` | 384 |
| `core/gc` | 177 |
| `core/string` | 92 |
| `core/runtime` | 25 |
| `core/interner` | 23 |
| `core/macros` | 13 |
| `core/wintertc` | 8 |
| `core/parser` | 2 |

Families: A nan_boxed codec (28) → B gc core/rooting (177: trace,
allocator, Gc handles, ephemerons, cells, vtables, weak maps, tests,
incl. 2 test bypasses) → C boa_string (92: builder, JsString/JsStr,
vtables, tests) → D buffers/atomics (124: utils batch copies, Element
protocol, futex, Atomics, call sites) → E trace bypass (127 + the 2
under B = 129) → F rest (176: opcode args, symbols, interop, JsObject
casts, macros, VM, interner, parser, wintertc).

Backstop theorem (proved from `collect`/`trace_non_roots`): an untraced `Gc`
handle keeps its target ROOTED, so a bypass that skips live handles is a
bounded leak, never UAF. Five sites rely on it (each analyzed individually,
all leak-free in practice via owner-drop release + traced back-edges):
DateTimeFormat caches, namespace resolved_bindings, synthetic-module init
captures, ProcessProvider capture, console Logger capture.

SAFETY fixes landed in this audit:
- `string/builder.rs`: `current_layout` uses `Layout::new` (never forms `&`
  to the uninitialized header); `extend_from_slice_unchecked` documents
  non-overlap.
- `array_buffer/utils.rs`: forward-copy contract admits `src >= dest`
  overlap (was documented non-overlap, called so by memmove).
- `interner/interned_str.rs`: safe-fn SAFETY comments cite the constructor
  invariant (were phrased as caller contracts).

Speculative UB alarms investigated and cleared: uri hex-pair lookahead
(`k + 3(n-1)` covers all iterations), wintertc `replace` (private,
Empty-asserted, thread-local), JsObject upcasts (`repr(C)` prefix + ZST
erased access), GcCell write-skips (paired-conservative), ephemeron key
deref (clear-before-sweep ordering).

P6.2 adds 3 test-only sites (typed_array element roundtrip macro), total
727/727. The 6.2 Miri expansion (16 `mod miri` sites, multi-seed schedule,
filter gate) is tracked in the P6 plan notes; counts above are the 6.1
close.
