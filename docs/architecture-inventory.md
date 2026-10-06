# Architecture Inventory (P0.3)

Static map of the Boa workspace at v0.22.0 (commit `39cd1121`), produced by
step-0 phase P0. Purpose: aim later effort at choke points first (P2 fix-loop
ordering), map touched crates to Test262 subsets for affected-subset gating
(P1.5), and record single points of catastrophic failure. Unsafe-site counts
are `unsafe`-mentioning source lines per crate (exact site list: see
[`unsafe-inventory.json`](unsafe-inventory.md)); concurrency facts are detailed
in [`concurrency-inventory.md`](concurrency-inventory.md).

Criticality scale: **S0** = wrong here miscompiles/mis-executes everything;
**S1** = wrong here breaks a semantic area; **S2** = wrong here breaks an
isolated builtin/API; **S3** = tooling — wrong here misleads the signal, not JS
semantics.

## Dependency graph (internal crates only)

```text
                          ┌──────────┐
                          │ boa_macros│  (proc macros: Trace derives, Class)
                          └────┬─────┘
                               │ used by almost everything (derive only)
     ┌─────────────┬───────────┼──────────────┬────────────┐
     ▼             ▼           ▼              ▼            ▼
┌──────────┐ ┌──────────┐ ┌─────────┐  ┌────────────┐ ┌──────────┐
│boa_string│ │boa_intern│ │ boa_gc  │  │  boa_ast   │ │boa_parser│
│ (no deps)│ │er→gc     │ │→string  │◄─┤→interner   │◄┤→ast      │
└────┬─────┘ └────┬─────┘ └────┬────┘  │  →string   │ │ →interner│
     │            │            │       └────────────┘ └──────────┘
     └────────────┴────────────┴─────────────┬──────────────────┘
                                             ▼
                        ┌────────────────────────────────────────┐
                        │               boa_engine               │
                        │ parser→AST→bytecompiler→VM→builtins    │
                        │ (+ boa_icu_provider, small_btree,      │
                        │    tag_ptr)                            │
                        └──────┬─────────────────────────┬───────┘
                               │                         │
                               ▼                         ▼
                      ┌──────────────────┐      ┌────────────────┐
                      │  boa_wintertc    │      │  boa_runtime   │
                      │  (TC55 web APIs) │◄─────┤  (web runtime, │
                      └──────────────────┘      │   re-exports   │
                                                │   wintertc)    │
                                                └───────┬────────┘
                                                        │
            ┌───────────────┬───────────────┬───────────┼───────────┐
            ▼               ▼               ▼           ▼           ▼
       ┌─────────┐   ┌───────────┐   ┌────────────┐ ┌────────┐ ┌─────────┐
       │ boa_cli │   │boa_examples│  │ boa_tester │ │boa_ben │ │ boa_wpt │
       │(+parser)│   │(+ast,parser│  │ (Test262)  │ │ches    │ │ (WPT)   │
       └─────────┘   │ ,interner) │  └────────────┘ └────────┘ └─────────┘
                     └───────────┘
```

`ffi/wasm` (wasm compat layer) depends on engine/runtime from outside the
default test flow; `tools/gen-icu4x-data` generates ICU data offline;
`tests/fuzz` (own workspace) depends on ast/engine/interner/parser.

## Crate inventory

| Crate                  | Purpose                                                  | Feeds into                              | Sem | Unsafe-adjacent                                         | Threads/locks                                                                         | Tests today                                                              |
| ---------------------- | -------------------------------------------------------- | --------------------------------------- | --- | ------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `core/ast`             | AST nodes, scope analysis, visitor                       | parser, engine, fuzz                    | S0  | no (0 lines)                                            | none                                                                                  | unit tests per node; `Arbitrary` impls for fuzz                          |
| `core/parser`          | Lexer + ECMAScript parser, error recovery                | engine (via AST), cli                   | S0  | no (2 lines)                                            | none                                                                                  | unit tests; Test262 (parse-phase negatives); parser-idempotency fuzzer   |
| `core/engine`          | Bytecompiler, VM, builtins, contexts, jobs, modules      | runtime, wintertc, cli, tester, benches | S0  | **yes (412)** — value codec, GC handles, buffers, futex | shared-memory only: `SharedArrayBuffer` atomics + futex `Mutex`; rest single-threaded | `src/tests/**` harness (`TestAction`), Test262, insta-bytecode snapshots |
| `core/gc`              | Mark-sweep GC, `Trace`/`Finalize`, ephemerons, weak maps | engine, interner, runtime               | S0  | **yes (183)** — allocator, sweep, rooting               | thread-local heap (`BOA_GC`); no shared GC state                                      | `src/test/**` incl. `miri` mods; Miri CI job                             |
| `core/string`          | `JsString` rope/slice/latin1/utf16 repr, builder         | engine, gc, ast                         | S0  | **yes (101)** — refcount, vtables, alloc/realloc        | `JsStr: Send+Sync` (audited in P6); else none                                         | `src/tests.rs`                                                           |
| `core/interner`        | String interner (`Sym`)                                  | ast, parser, engine                     | S1  | yes (24) — fixed-string bump storage                    | none                                                                                  | `src/tests.rs`                                                           |
| `core/macros`          | Proc macros (`Trace` derive, `Class`, modules)           | all (compile-time)                      | S1  | generated-code only (17)                                | n/a                                                                                   | `tests/macros` trybuild-style                                            |
| `core/icu_provider`    | Bundled ICU4X data provider                              | engine (`intl_bundled`)                 | S2  | no (0)                                                  | none                                                                                  | via `Intl` Test262 + unit tests                                          |
| `core/runtime`         | Example web runtime, re-exports wintertc TC55 APIs       | cli, tester, examples                   | S1  | yes (25)                                                | `$262.agent` spawns worker threads; message channels                                  | `tests/clone.rs` (rstest), per-module tests                              |
| `core/wintertc`        | WinterTC TC55 minimum web APIs                           | runtime, direct embedders               | S2  | yes (9)                                                 | none (single-threaded)                                                                | per-module tests + WPT suites                                            |
| `utils/tag_ptr`        | Tagged-pointer utility                                   | engine                                  | S1  | check in P6 (see unsafe inventory if any)               | none                                                                                  | unit tests                                                               |
| `utils/small_btree`    | Small B-tree map                                         | engine (property maps)                  | S1  | check in P6                                             | none                                                                                  | unit tests                                                               |
| `cli` (`boa`)          | CLI: REPL, file/stdin/`-e` eval, debug + `$262` objects  | users                                   | S3  | via engine only                                         | REPL thread + `async_channel`; smol `Executor` for jobs                               | manual + e2e-ish runs                                                    |
| `examples`             | 30 embedding examples (incl. tokio/smol loops, limits)   | docs/embedders                          | S3  | via engine only                                         | tokio/smol examples only                                                              | built + run in CI                                                        |
| `benches`              | Criterion `scripts` bench (35 JS files)                  | perf baseline                           | S3  | via engine only                                         | none                                                                                  | n/a (measurement, not assertion)                                         |
| `tests/tester`         | Test262 runner (`run`/`compare`)                         | gates                                   | S3  | no                                                      | rayon suite/test parallelism                                                          | 2 unit tests (`results.rs` commit detection)                             |
| `tests/insta-bytecode` | Bytecode snapshot tests                                  | compiler-change signal                  | S3  | no                                                      | none                                                                                  | insta snapshots in CI                                                    |
| `tests/wpt`            | WPT runner (`boa_wpt`, excluded from default workspace)  | host-API signal                         | S3  | via runtime                                             | 10 s watchdog per case                                                                | `cargo test -p boa_wpt` (not in CI yet — P2.5)                           |
| `tests/fuzz`           | 3 cargo-fuzz targets (own workspace)                     | P4 fleet                                | S3  | harnesses feed unsafe core                              | LibFuzzer in-process                                                                  | build-only in CI today (P4 schedules runs)                               |
| `ffi/wasm`             | Wasm compat layer                                        | wasm embedders                          | S2  | via engine only                                         | none                                                                                  | `tests/web.rs` (wasm)                                                    |
| `tools/gen-icu4x-data` | Offline ICU4X data generator                             | `boa_icu_provider` data                 | S3  | no                                                      | none                                                                                  | run on data refresh                                                      |

## Engine module inventory (`core/engine/src/`)

The pipeline: `Script::parse`/`Module::parse` → `bytecompiler` → `vm`
(`CodeBlock` + `Context::execute_one` dispatch) → builtins via call frames.

| Module                                                         | Role                                                                   | Sem   | Notes                                                         |
| -------------------------------------------------------------- | ---------------------------------------------------------------------- | ----- | ------------------------------------------------------------- |
| `value/`                                                       | `JsValue` NaN-boxed repr (default) or enum (`jsvalue-enum`)            | S0    | tag codec = top choke point; `Trace` only marks `Object`      |
| `vm/`                                                          | Register VM, `CodeBlock`, ~196 opcodes, dispatch, ICs, `RuntimeLimits` | S0    | `opcode/` families, `inline_cache/` (PIC cap 4), `flowgraph/` |
| `bytecompiler/`                                                | AST → bytecode, registers, scopes, jumps, handlers                     | S0    | `mod.rs:2788 finish()` output contract → P5 validity model    |
| `object/`                                                      | Objects, shapes, property maps/lookup, internal methods                | S0    | shape/p lookup = top choke point                              |
| `environments/`                                                | Declarative/function/private environments, bindings                    | S0    | lifetime errors = UAF-class bugs                              |
| `context/`                                                     | `Context`, intrinsics, ICU/time wiring, `ContextBuilder`               | S0    | per-eval state; `FixedClock` for determinism                  |
| `builtins/`                                                    | ~35 builtins (one dir each) + `Temporal`/`Intl`/typed arrays           | S1–S2 | fix-loop target for P2; spec-step comments mandatory          |
| `module/`                                                      | Module records, linking, `SimpleModuleLoader`, synthetic modules       | S1    | loader rooted per test dir in tester                          |
| `job.rs`                                                       | Job queue, `SimpleJobExecutor`, promise jobs                           | S1    | must `run_jobs()` after every eval                            |
| `error/`                                                       | `JsError`, native kinds, backtraces                                    | S1    | `Display` appends backtrace — never raw-compare               |
| `string.rs`, `symbol.rs`, `bigint.rs`, `property/`, `class.rs` | Value-adjacent types                                                   | S1    | `JsSymbol: Send+Sync` (P6 audit)                              |
| `optimizer/`                                                   | Bytecode optimizer (`OptimizerOptions`)                                | S1    | tester `-O` off by default; fuzz covers via `Script::parse`   |
| `native_function/`                                             | Native fn objects, captures, continuations                             | S1    | `from_closure` `unsafe` + `SAFETY` pattern                    |
| `interop/`                                                     | Rust↔JS conversions                                                    | S2    | `into_js_function_unsafe` contracts                           |
| `realm.rs`, `script.rs`, `spanned_source_text.rs`              | Realms, script wrapper, sources                                        | S1    | `Script::parse/codeblock/evaluate` = harness entry            |
| `sys/`                                                         | Platform glue                                                          | S2    | —                                                             |
| `tests/`                                                       | JS-behavior test harness + suites                                      | S3    | `TestAction` runner all new tests must use                    |

## Single points of catastrophic failure (choke list)

Ordered by blast radius; P2 fixes areas in this order and P6 proves/audits
top-down:

1. `JsValue` tag encode/decode (`core/engine/src/value/inner/nan_boxed.rs`) —
   every value flows through it; Kani target in P6.
2. GC rooting/tracing (`core/gc`: `lib.rs` collect, `trace.rs` obligations,
   `GcHeader` root counts) — missed edge = use-after-free.
3. Property lookup + shapes (`core/engine/src/object/`, `vm/inline_cache/`) —
   wrong lookup corrupts all object semantics.
4. Bytecode dispatch + `CodeBlock` validity (`vm/mod.rs:execute_one`,
   `bytecompiler/mod.rs:finish`) — P5 writes the validity model.
5. Environment/binding lifetime (`environments/`, `bytecompiler/env.rs`) —
   dangling scope = corruption.
6. String interner + `JsString` repr (`core/string`, `core/interner`) — every
   property key and literal flows through these.
7. Module loader + job queue (`module/`, `job.rs`, tester `exec/mod.rs`) —
   ordering bugs break async/module semantics globally.

## Test-coverage status (honest P0 snapshot)

- Rust line/branch coverage exists as a CI job (tarpaulin → Codecov) but no
  per-crate baseline was recorded in P0; P7.1 publishes the four coverage
  dimensions per commit — until then, "coverage" below means test-location
  pointers, not measured percentages.
- Test262 (53,578 tests) exercises parser/bytecompiler/VM/builtins end to end;
  unit tests live next to the code (`mod tests` / `tests.rs`); bytecode-shape
  changes are caught by `tests/insta-bytecode` snapshots in CI.
- Host APIs (`boa_runtime`/`boa_wintertc`) are covered by per-module tests plus
  WPT suites run manually (`cargo test -p boa_wpt`) — no CI job yet (P2.5).
- Spec-area → test mapping does not exist yet; P2.1 builds
  `docs/conformance-gap.md` on top of this inventory.
