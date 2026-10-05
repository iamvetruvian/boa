# Host-Boundary Map (P0.7)

What the engine exposes to hosts and what hosts inject into the engine — the
complete list of surfaces that accept potentially untrusted input. P4 designs
adversarial-input fuzz targets from the last table; P2.5 covers the API
behavior with unit tests + WPT.

## `boa_runtime` vs `boa_wintertc`

| | `boa_wintertc` (TC55 minimum web APIs) | `boa_runtime` (example web runtime) |
|---|---|---|
| Own modules | `abort`, `base64`, `clone`, `console`, `encoding`, `events`, `microtask`, `store`, `timers`, `url` (feature `url`), `fetch` (feature `fetch`) | `text`, `message`, `url` (feature `url`), `fetch` (feature `fetch`), `abort` (feature `fetch`), `process` (feature `process`), `test262` (feature `test262`) |
| Re-exports | — (standalone, depends only on `boa_engine`) | `console`, `base64`, `clone`, `microtask`, `store` from wintertc; `timers` as `interval` |
| `register` installs | console, timers, encoding, microtask, clone, base64, abort + `url`/`fetch` by feature (`wintertc/src/lib.rs:63`) | Base64, Timeout, Encoding, Microtask, StructuredClone + `url`/`process`/abort by feature, then caller extensions via `RuntimeExtension` (`runtime/src/lib.rs:172`; tuple impls up to 12 at `extensions.rs:173`) |

## What the CLI injects (`cli/src/main.rs`)

- Always: `console` (+ `fetch` with `BlockingReqwestFetcher` when the `fetch`
  feature is on) via `add_runtime` (`main.rs:830`).
- `--debug-object`: `$boa` with `function`, `object`, `shape`, `optimizer`,
  `gc`, `realm`, `limits`, `string` sub-objects
  (`cli/src/debug/mod.rs:27-74`).
- `--test262-object`: `$262` (`createRealm`, `detachArrayBuffer`,
  `evalScript`, `gc`, `global`, `agent`, …) plus
  `var print = console.log.bind(console)` (`main.rs:593-607`).
- Module mode (`-m`): `SimpleModuleLoader` rooted at `-r/--root` (default `.`)
  (`main.rs:571`); scripts via `FILE` args, `-e` expression, or stdin REPL.
- Diagnostics: `--dump-ast`, `--trace`, `--flowgraph`, `--time`, `--strict`,
  `--optimize`, `--no-can-block` (disables `Atomics.wait` blocking).

## What the tester injects per test (`tests/tester/src/exec/mod.rs:522`)

Fresh `Context` per test: `SimpleModuleLoader` rooted at the test's parent
dir, `can_block` from test flags, `print()` capture fn (async-completion
protocol), `$262` object, optional `console`, plus harness files
(`assert.js`, `sta.js`, `doneprintHandle.js`) and the test's `includes`.

## Embedding entry points

- Rust API: `Context::eval(Source)` (`context/mod.rs:205`),
  `Script::parse/codeblock/evaluate`, `Module::parse/load_link_evaluate`;
  `Source::{from_bytes, from_filepath, from_reader}`.
- `ffi/wasm`: `evaluate(src: &str) -> Result<String, JsValue>`
  (`ffi/wasm/src/lib.rs:19`) — whole engine over a string boundary.
- 30 example binaries (`examples/src/bin/`), incl. `loadstring`, `loadfile`,
  `modules`, `modulehandler`, `tokio_event_loop`, `smol_event_loop`,
  `runtime_limits`, synthetic modules, and per-builtin demos.

## Engine feature flags that change behavior under test

`default = float16,xsum,temporal`. Behavior-affecting: `intl`, `intl_bundled`,
`annex-b`, `experimental`, `temporal`, `jsvalue-enum` (repr change),
`fuzz` (fuel + `Arbitrary`), `flowgraph`, `trace`, `deser`, `either`,
`system-time-zone`, `native-backtrace`, `embedded_lz4`, `float16`, `xsum`,
`js` (see `core/engine/Cargo.toml:14-93`). Baselines record their feature row
(`docs/baseline.md`); gates must compare within the same row.

## Untrusted-input surfaces (P4 target bounds)

| # | Surface | Entry code | Threat shape | P4 target |
|---|---|---|---|---|
| 1 | JS source bytes (scripts, modules, `eval`, `$262.evalScript`, `-e`, stdin, `ffi/wasm::evaluate`) | `Source::*`, `Script/Module::parse`, `cli/src/main.rs:612`, `ffi/wasm/src/lib.rs:19` | parser crashes/hangs, evil syntax | parser robustness (existing idempotency target + byte-level target) |
| 2 | Module specifiers + resolved file bytes | `SimpleModuleLoader` (root `-r` / test parent dir) | path traversal outside root, hostile module graphs | module-specifier fuzz |
| 3 | CLI flags + `FILE` paths | `cli/src/main.rs:89-179` | option confusion, unreadable/huge files | host-injection paths |
| 4 | Fetch responses (status/headers/body bytes) | `BlockingReqwestFetcher` (`cli/Cargo.toml:46`), `runtime/wintertc fetch` | malformed bodies, huge payloads | fetch-response fuzz |
| 5 | URL strings | `runtime/wintertc url` modules | parser panics, non-termination | url-parse fuzz |
| 6 | `structuredClone` payloads / message ports | `wintertc store/clone`, `runtime message` | deep/cyclic graphs, hostile deserialization | clone/message fuzz |
| 7 | `Atomics.wait` + shared buffers across agents | `builtins/atomics`, `futex.rs`, `test262.rs` agent threads | deadlocks, data races (UB), wait-forever | concurrency stress (P4.4/P6) |
| 8 | ICU locale/data inputs (`Intl`, `Temporal`) | `builtins/intl`, `builtins/temporal`, `boa_icu_provider` | ICU4X panics, locale canonicalization gaps | intl/temporal differential vs oracle |
| 9 | Bytecode (only if a surface ever accepts it) | none known — `ByteCompiler::finish` is the sole producer | n/a today | P5 documents out-of-contract; fuzz only valid bytecode |

`$boa` debug object and flowgraph/trace dumps are trusted-developer surfaces
(host opt-in, never exposed to page JS); they are correctness-tested but not
adversarially fuzzed.
