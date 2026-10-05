# boa_engine-fuzz

This directory contains fuzzers which can be used to automatically identify faults present in Boa. The original three
are [grammar-aware](https://www.fuzzingbook.org/html/Grammars.html) (based on
[Arbitrary](https://docs.rs/arbitrary/latest/arbitrary/)) and coverage-guided; the P4.3 adversarial targets add
raw-byte and host-boundary harnesses. Shared harness code lives in [`src/`](src/): generation in
[`common.rs`](src/common.rs), parser-oracle domain normalization in [`canonicalize.rs`](src/canonicalize.rs), the
name-universe check in [`universe.rs`](src/universe.rs), semantic oracles in [`semantic.rs`](src/semantic.rs), and
host-boundary harnesses in [`adversarial.rs`](src/adversarial.rs).
Harness logic is unit-tested with plain `cargo test` (stable toolchain, no libFuzzer needed).

You can run any fuzzer you wish with the following command (replacing `your-fuzzer` with a fuzzer available in
fuzz_targets, e.g. `parser-idempotency`):

```bash
cargo fuzz run -s none your-fuzzer
```

Note that you may wish to use a different sanitizer option (`-s`) according to what kind of issue you're looking for.
Refer to the [cargo-fuzz book](https://rust-fuzz.github.io/book/cargo-fuzz.html) for details on how to select a
sanitizer and other flags.

## Parser Fuzzer

The parser fuzzer, located in [parser-idempotency.rs](./fuzz_targets/parser-idempotency.rs), identifies
correctness issues in both the parser and the AST-to-source conversion process (e.g., via `to_interned_string`) by
searching for inputs which are not idempotent over parsing and conversion back to source. It does this by doing the
following:

1. Generate an arbitrary AST
2. Convert that AST to source code with `to_interned_string`; we'll call this the "original source"
3. Parse the original source into an AST; we'll call this the "first AST"
   - Arbitrary ASTs aren't guaranteed to be parseable; to avoid errors caused by this, we discard errors here.
4. Convert the first AST to source code with `to_interned_string`; we'll call this the "first source"
5. Parse the first source into an AST; we'll call this the "second AST"
   - Since the original source was parseable, the first source must be parseable; emit any errors parsing produces.
6. Assert the print fixpoint (first source equals second source; a third pass catches oscillation) and the
   name-universe subset property (every name in the reparsed ASTs already existed before parsing, modulo the
   spec-rule `"#x"` private-field synthesis).
   - A fixpoint failure indicates the printer lost information or the parser misread the printed form; an
     unknown name indicates the printer emitted a name the parser materialized (e.g. a `NaN` literal morphing
     into the `NaN` identifier). Span differences are expected (the printer normalizes whitespace) and are
     not compared.

In this way, this fuzzer can identify correctness issues present in the parser.

The parser target also runs domain canonicalization ([`canonicalize.rs`](src/canonicalize.rs)) before
printing: `Arbitrary` derivation can emit parser-unreachable states (non-finite float literals,
mismatched template arrays, out-of-context `yield`/`await`, empty binding lists) whose prints
reparse with materialized names. Canonicalization normalizes those states; the oracle then checks
a print fixpoint (three passes), parse-error determinism, and the name-universe subset property
([`universe.rs`](src/universe.rs)).

## Bytecompiler Fuzzer

The bytecompiler fuzzer, located in [bytecompiler-implied.rs](./fuzz_targets/bytecompiler-implied.rs), identifies cases
which cause an assertion failure in the bytecompiler. These crashes can cause denial of service issues and may block the
discovery of crash cases in the VM fuzzer. It also asserts compile determinism: the same source compiled twice in fresh
contexts must succeed/fail identically with byte-identical disassembly (see [`semantic.rs`](src/semantic.rs)).

## VM Fuzzer

The VM fuzzer, located in [vm-implied.rs](./fuzz_targets/vm-implied.rs), identifies crash cases in the VM. It does so by
generating an arbitrary AST, converting it to source code (to remove invalid inputs), then executing that source code.
Beyond crashes, it runs semantic oracles (see [`semantic.rs`](src/semantic.rs)): determinism double-eval,
eval-vs-eval-of-reprint idempotency, the metamorphic hook (identity until P5), and — when `BOA_FUZZ_ORACLE`
points at a script-goal shell — completion-class differential against the pinned P3 oracle (completion
vs threw + error class; fuel exhaustion, oracle timeouts, and unclassifiable pairs skip rather than
assert). Known open-bug divergence shapes are filtered so the loop reaches new bugs; each filter entry
names its regression-DB id and a unit test fails if the bug lands without retiring the entry.

To ensure that the VM does not attempt to execute an infinite loop, Boa is restricted to a finite number of instructions
before the VM is terminated. If a program takes more than a second or so to execute, it likely indicates an issue in the
VM (as we expect the fuzzer to execute only a certain amount of instructions, which should take significantly less
time).

## Adversarial Targets (P4.3)

Five host-boundary harnesses (shared code in [`adversarial.rs`](src/adversarial.rs)) drive `boa_runtime` entry
points and host-adjacent APIs with adversarial inputs. The byte-level targets consume `seeds/*.js` literally
as programs (see [`sync-seeds.sh`](./sync-seeds.sh)), so every regression reproducer is also a directed seed.

- [`source-bytes.rs`](./fuzz_targets/source-bytes.rs): arbitrary bytes must parse deterministically and never
  panic the parser. Found bug #22 (parser stack overflow on deep nesting; fixed with a stack-aware guard).
- [`module-specifier.rs`](./fuzz_targets/module-specifier.rs): adversarial (specifier, base, referrer) triples
  must resolve deterministically through `boa_engine::module::resolve_module_specifier`.
- [`url-parse.rs`](./fuzz_targets/url-parse.rs): the native `Url` constructor must parse deterministically, and
  the JS-level `URL` / `URLSearchParams` constructors must evaluate deterministically in the host context.
- [`fetch-response.rs`](./fuzz_targets/fetch-response.rs): adversarial (body, status, statusText) triples must
  follow the fetch "initialize a response" validation order exactly (status range → statusText → null-body
  rule), encoded as executable oracle rules, and evaluate deterministically.
- [`cli-exec.rs`](./fuzz_targets/cli-exec.rs): arbitrary bytes run as a script in the full CLI host context
  (console/fetch/URL/…, offline `ErrorFetcher` backend, null console logger, fuelled) must evaluate
  deterministically and never panic or hang. Found bug #13's crash (proxy-mediated prototype cycle;
  fixed with stack-aware walk guards).

## GC Stress (P4.4)

`boa_gc` exposes a thread-local stress knob: `gc_set_stress(Some(n))` collects every `n`
allocations (bypassing the 1 MB threshold and adaptive growth) until cleared, with the
panic-safe RAII [`StressGuard`](https://docs.rs/boa_gc/latest/boa_gc/struct.StressGuard.html)
restoring the previous setting. Three consumers:

- `vm-implied` and `cli-exec` run their second eval leg under stress (default: every 100
  allocations, overridable per-run with `BOA_FUZZ_GC_STRESS_EVERY`): any divergence from the
  normal leg is a missing GC root or a collection-timing assumption. Programs that can
  legitimately observe collection timing (`WeakRef`, `FinalizationRegistry`, `WeakMap`,
  `WeakSet` — substring pre-filter, best-effort) keep a plain second leg instead, as do
  allocation-churn outliers (normal leg over 100k allocations — stressed cost is
  superlinear in churn, so uncapped stress would let pathological mutants starve the
  fleet's time budget).
- `bytecompiler-implied` compiles its second leg under stress with no filter (compilation
  runs no user code, so nothing can observe timing).
- Every fuzz-built context runs [`stress_point`](src/semantic.rs) (a forced collection at a
  hostile pre-eval point) on every eval, stressed or not.

The same knob drives `boa_tester run --gc-stress[=N]` (default 100): each Test262 test sets
the cadence on its own worker thread (or inherits it through the `run-single` child
protocol under `--timeout`), and the scheduled full-corpus stress run in `test262_full.yml`
is gated with `compare --fail-on=regression` against the normal run. The stress job uses an
extended `--timeout 300` (see the workflow comment for why); a new timeout there is still
signal, exactly as a new failure is.

## Sanitizer Matrix (P4.5)

`-s none` is the baseline on every schedule (PR smoke, nightly evolution, weekly
coverage). On top of it, the unsafe-adjacent targets run weekly under sanitizers
(`fuzz-sanitizer` job in `fuzz-nightly.yml`, Sundays, 30 min per cell):

- `vm-implied` + `cli-exec` under `-s address` (spatial/temporal memory errors —
  the class that matters most next to 590 `unsafe` sites),
- `vm-implied` + `source-bytes` under `-s memory` (uninitialized reads).

Locally (pinned nightly required):

```sh
cd tests/fuzz
RUSTUP_TOOLCHAIN=nightly-2026-10-01 cargo fuzz run --dev -s address vm-implied -- -max_total_time=300
RUSTUP_TOOLCHAIN=nightly-2026-10-01 cargo fuzz run --dev -s memory source-bytes -- -max_total_time=300
```

Deliberate exclusions, documented in the workflow: UBSan has no cargo-fuzz
spelling (0.13.2 offers `address`, `leak`, `memory`, `thread`, `none`), so
its integer/bounds classes ride on dev-profile `overflow-checks` +
`debug-assertions` (enabled on all schedules, abort the worker the same
way); standalone `-s leak` runs are redundant because ASan already runs
the leak checker at its defaults — and the defaults are proven clean (a
200-run ASan loop exits 0 with zero leak reports), so a future leak
report is signal, not GC noise.

Boundary: this phase owns sanitizer *fuzz* runs only. Sanitizer-instrumented
`cargo test` runs for unsafe-adjacent crates belong to P6 (unsafe audit), which
owns the unsafe inventory and its gates.
