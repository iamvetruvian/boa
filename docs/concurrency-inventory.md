# Concurrency Inventory (P0.5)

Enumeration of threads, atomics, locks, channels, executors, and `Send`/`Sync`
impls across the workspace. Verdict: **the engine is effectively
single-threaded** — one thread-local GC heap per thread, no shared engine
state — with exactly three shared-memory surfaces (shared array buffers,
the futex waiters map, the symbol registry) plus harness/CLI-level threading.
This inventory decides Loom's scope in P6 (see verdict at the end).

## Thread-local state (no sharing)

| Site                                                                                       | Contents                                                                                                               |
| ------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| `core/gc/src/lib.rs:44-51`                                                                 | `BOA_GC` heap (`RefCell<BoaGc>`) + `GC_DROPPING` flag — the collector is per-thread by construction                    |
| `core/engine/src/context/mod.rs:48-50`                                                     | `CANNOT_BLOCK_COUNTER` — contexts on one thread share a runtime but must stay on that thread (documented on `Context`) |
| `core/engine/src/object/jsobject.rs:1254`, `vm/code_block.rs:360`, `module/source.rs:1191` | test-only thread-locals                                                                                                |

Consequence: two threads each running a `Context` never share GC or engine
state (objects must not cross threads). GC tests enforce isolation with
thread-per-test (`core/gc/src/test/mod.rs:54-58`).

## Shared-memory surfaces (the only cross-thread engine state)

1. **`SharedArrayBuffer` backing stores** (`builtins/array_buffer/shared.rs`):
   `Arc<Inner>` of `AtomicU8` slices (`portable_atomic`), accessed with
   explicit orderings through `SliceRef`/`SliceMut` (`utils.rs`). This is
   real shared memory by spec design (`Atomics`, workers).
2. **Futex waiters map** (`builtins/atomics/futex.rs:252-253`): global
   `static CRITICAL_SECTION: Mutex<FutexWaiters>` + `Condvar`; `wait_async`
   resolves through `oneshot::async_channel` (`futex.rs:535`). `unsafe impl
Send for FutexWaiters` (`futex.rs:248`) is justified by global-mutex
   confinement (P6 re-verifies).
3. **Symbol registry** (`builtins/symbol/mod.rs:36`): `dashmap::DashMap` for
   the global symbol table.

## `Send`/`Sync` impls (complete list)

| Impl                                      | File                            | Note                     |
| ----------------------------------------- | ------------------------------- | ------------------------ |
| `unsafe impl Send for FutexWaiters`       | `builtins/atomics/futex.rs:248` | mutex-confined; P6 audit |
| `unsafe impl Send/Sync for JsSymbol`      | `symbol.rs:152-154`             | P6 audit                 |
| `unsafe impl Send for GcRefCell<T: Send>` | `gc/cell.rs:522`                | conditional; P6 audit    |
| `unsafe impl Send/Sync for JsStr`         | `string/str.rs:35-39`           | P6 audit                 |

## Threads (all harness/CLI/test level — never inside the engine)

| Thread                     | File                                 | Purpose                                                                                                                                                       |
| -------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `$262.agent.start` workers | `core/runtime/src/test262.rs:244`    | Test262 agent threads; joined via `WorkerHandles::join_all`; broadcast via `bus::Bus` (`test262.rs:228`); worker panics re-panic into the per-test panic path |
| CLI readline thread        | `cli/src/main.rs:815-828`            | REPL input; ships lines over `async_channel::unbounded` (`main.rs:567`)                                                                                       |
| rayon pool                 | `tests/tester/src/exec/mod.rs:45,75` | suite/test parallelism (`par_iter`; `-d` disables) — each test builds a fresh `Context`, so no engine state is shared                                         |
| GC test isolation          | `core/gc/src/test/mod.rs:56`         | one thread per test                                                                                                                                           |
| Test-only spawns           | atomics/symbol/message/date tests    | `thread::scope` / `thread::spawn` / `std::sync::mpsc` inside `#[test]` / `#[cfg(test)]` only                                                                  |

## Async executors and channels

| Item                       | File                                          | Note                                                                                                                |
| -------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| CLI `Executor`             | `cli/src/executor.rs:19,263`                  | smol `LocalExecutor` job pump; blocks on idle loop — headless harnesses must NOT reuse it (use `SimpleJobExecutor`) |
| `futures::mpsc::unbounded` | `core/runtime/src/message/senders.rs:43`      | message-port plumbing                                                                                               |
| `async_channel`            | CLI REPL (`main.rs:567`)                      | line transport only                                                                                                 |
| `tokio`/`smol` examples    | `examples/src/bin/{tokio,smol}_event_loop.rs` | examples only, not engine code                                                                                      |

## P6 verdict: Loom scope

Loom (`RUSTFLAGS="--cfg loom"`) is designed for code with shared-memory
concurrency. Applicability per surface:

- Thread-local engine/GC/context state: **out of scope** (no shared memory).
- `SharedArrayBuffer` atomics + `Atomics` ops: candidate **only** for the
  ordering-sensitive copy/wait paths (`utils.rs` batched copies,
  `futex.rs` notify/wait) — and only if a harness can be written without the
  full engine; otherwise covered by Miri + the atomics stress tests.
- Futex `Mutex`+`Condvar` map: **marginal** — mutex-guarded, no lock-free
  protocol; standard stress tests suffice.
- Harness threads (rayon, agents, REPL): **out of scope** — no shared engine
  state; each unit owns its `Context`.

Expected outcome: Loom is either narrowly applied to one futex/atomic harness
or documented as out of scope with this inventory as evidence. No
Loom-everywhere theater.
