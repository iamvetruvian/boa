# Implementation Plan — Project-1 Step 0: Eliminate Correctness and Architectural Instability in Boa

## Context and scope (read this first)

**What this plan is about, and why.** The larger program (`discussion.md`) is:
take Boa, an experimental Rust JavaScript engine, and develop it into a
production browser engine via two projects — **Project-1** (make Boa a fully
ECMAScript-conformant JavaScript engine) and **Project-2** (make Boa
technologically comparable to V8: tiered JIT, feedback, deoptimization,
JIT-aware GC). Both projects build on the existing interpreter pipeline, so an
unstable or incorrect interpreter would multiply uncertainty into every later
stage ("buggy VM → JIT → optimized buggy semantics → impossible debugging
nightmare"). This plan therefore implements the discussion's prerequisite: make
the interpreter correct and architecturally stable *first*, and — more
importantly — build the permanent assurance system (harness, oracles, fuzz
fleet, gates) that *proves* it stays that way while all later work proceeds.

**Which part of `discussion.md` this plan addresses, exactly.**

- The step-zero mandate, lines 892–936 (`# 0. First: eliminate correctness and
  architectural instability`): the interpreter must become the semantic
  reference implementation against which every later tier is tested.
- The Claim B aspiration, lines 1791–1795: "extremely high confidence that
  there are no important undiscovered bugs in the tested scope" — explicitly
  *not* Claim C ("no bug exists anywhere"), which testing cannot establish.
- The revised roadmap's **PHASE 0 through PHASE 6**, lines 3257–3311 (pin and
  inventory → Test262/spec tests → differential testing → fuzzing →
  bug-fixing with permanent regression tests → invariants/Miri/sanitizers/
  proofs → interpreter as "high-confidence semantic reference"), with the six
  corrections documented in the grill verdict below (notably: extend Boa's
  existing tester/Miri/fuzz/CI infrastructure rather than rebuilding it).
- The readiness gates A–G concept, lines 3013–3083, adapted into the P7 gate
  battery (trend-based rather than absolute where upstream-controlled).

**What this plan does NOT address.** The discussion's steps 1–11 (VM fast
paths, GC optimization, feedback vectors, tiering, baseline/mid/top-tier JITs,
structured IR, deoptimization, snapshots), the parallel WebAssembly and
debugger tracks, Project-2 as a whole, and the surrounding browser — all are
downstream. See Non-goals.

**The immediate next goal after this plan is implemented.** Per the
discussion's revised roadmap, **PHASE 7 (line 3312): begin the baseline JIT,
differentially checked against the interpreter at every step** — i.e., start
Project-2 execution work on top of a trusted semantic reference, while
conformance keeps ratcheting upward under the P2 trend gate and the P7 battery
keeps running permanently. Concretely: the exit state of this plan (pinned
baseline + green battery + empty triage queues + interpreter-as-reference
rule, per P7.4) is the entry state for baseline-JIT work; no JIT, GC, or
optimizer work starts before the step-0 exit review signs off Success Criteria
1–10.

## Goal

Bring Boa's existing interpreter pipeline (parser, AST, bytecompiler, bytecode VM,
builtins, GC, runtime/host bindings) to a state where we can truthfully claim:

> "We have extremely high confidence that there are no important undiscovered bugs
> in the tested scope, and no change in this program regresses any previously
> established behavior. Only progress, no regression."

Concretely, when this plan is fully executed, Boa must have: a pinned,
reproducible test baseline; a hardened Test262 harness with no silent
skips/crashes; a permanent regression database; differential testing against
independent engines; a running fuzz fleet (parser, bytecompiler, VM, GC-stress);
an audited `unsafe` inventory with Miri + sanitizer coverage; bounded proofs of
critical kernels; multi-dimensional coverage with mutation-tested suites; and a
formal readiness gate that must stay green before any Project-2 (JIT/GC) work
begins. No new JIT tiers and no GC redesign are in scope; only correctness,
stability, and the assurance infrastructure that proves them.

Execution model: AI agents write all code (implementation + tests + harnesses).
Human/agent effort concentrates on triage decisions (differential mismatches vs
the spec), gate reviews, and audit sign-offs. Testing rigor — not code volume —
is the bottleneck, and this plan is sequenced so every phase ends in a
mechanical gate.

## Grill verdict on `discussion.md` (researched 2026-10-01)

The discussion was verified claim-by-claim against the repo at
`/home/vetruvian/Desktop/boa` and against inspected authoritative sources.
Verdict: **the discussion's factual picture of Boa is accurate, its core
correctness strategy is correct, but its step-0 approach needs six corrections
before it becomes an executable plan.** There are no fatal hallucinations and no
missing prerequisite project; the corrections below are refinements of
sequencing, gating, and build-vs-extend decisions, all incorporated into the
phases that follow.

### Boa-state claims: confirmed, with evidence

| # | Claim in discussion | Verdict | Evidence |
|---|---------------------|---------|----------|
| 1 | Boa is at v0.22.0, released late Aug 2026 | CONFIRMED (1-day drift: repo says 2026-08-27, discussion says Aug 28) | `Cargo.toml:29`, `CHANGELOG.md:3` |
| 2 | ~95.6% Test262 conformance | SUBSTANTIALLY CONFIRMED (independent snapshots agree at 95.4–95.6%; repo computes the figure at runtime, it is not pinned in-tree) | `tests/tester` computes pass rate at runtime; `README.md:20` says ">90%"; corroborated by independent public snapshots (web-search snippets, not used as authoritative evidence) |
| 3 | Self-described "experimental" engine | CONFIRMED | `README.md:20` |
| 4 | Pipeline: parser → AST → bytecompiler → bytecode → register VM; `CodeBlock` holds bytecode/constants/bindings/exception info/ICs/registers | CONFIRMED | `core/engine/src/{context,bytecompiler,vm,codeblock}` + `docs/vm.md` |
| 5 | Bytecode optimizer + polymorphic inline caches in 0.22 | CONFIRMED | `CHANGELOG.md:29-30,128-140,173-189` |
| 6 | NaN-boxed `JsValue` | CONFIRMED (default; legacy enum behind `jsvalue-enum`) | `core/engine/src/value/inner.rs:1-12` |
| 7 | `boa_gc` mark-sweep collector with `Trace`/`Finalize` | CONFIRMED (thread-local, 1 MB default threshold — nowhere near V8's generational/concurrent design, as the discussion states) | `core/gc/src/lib.rs:1-5,44-70` |
| 8 | Builtins inventory (Object…Temporal, modules, eval, `FinalizationRegistry`, `RegExp.escape`, iterator helpers, `Error.prototype.stack`) | CONFIRMED | `core/engine/src/builtins/*`, `CHANGELOG.md:7-9,45-65` |
| 9 | ICU4X-backed `Intl` | CONFIRMED | root `Cargo.toml` `icu_*` deps; `core/icu_provider` |
| 10 | `boa_runtime` / `boa_wintertc` (TC55) Web-API layer | CONFIRMED (`boa_runtime` re-exports `boa_wintertc`; the "migration" framing is directionally right) | `core/wintertc/src/lib.rs:1-14`, `core/runtime/src/*` |
| 11 | No production JIT; Cranelift prototype: 109/196 opcodes, `jit` flag, copy-and-patch + feedback-vector gaps, GC at 10–16% of benchmark time | CONFIRMED nearly verbatim | `https://github.com/boa-dev/boa/discussions/4487` (inspected; exact quotes found, including "Currently 109 of Boa's 196 opcodes are supported" and "Profiling shows GC at 10-16% of V8 benchmark time") |
| 12 | Tester records panics; `run`/`compare` workflow; `test262_config.toml` ignore lists; `test262.yml` CI | CONFIRMED | `CONTRIBUTING.md:67-118`, `tests/tester`, `test262_config.toml`, `.github/workflows/test262.yml` |
| 13 | CI runs fmt/clippy/tests/coverage/Miri/fuzz-build/semver | CONFIRMED | `.github/workflows/rust.yml` (coverage :156, tests :197, Miri :316-349, fuzz build-only :351-385) |
| 14 | Existing fuzz targets (parser-idempotency, bytecompiler-implied, vm-implied), grammar-aware via `Arbitrary`, coverage-guided; VM fuzzer is crash-only, finds no logic errors | CONFIRMED — **and the discussion never mentions this infrastructure** | `tests/fuzz/README.md`, `tests/fuzz/fuzz_targets/*` (excluded from default workspace in root `Cargo.toml`) |
| 15 | Panic-removal work (`EngineError::Panic`, `js_expect`) in 0.22 | CONFIRMED | `CHANGELOG.md:28,40-43,53,171-203` |
| 16 | Test262: 50,000+ files; TC39 states coverage is not complete and tests may contain omissions/errors | CONFIRMED verbatim | `https://raw.githubusercontent.com/tc39/test262/main/README.md` (inspected) |
| 17 | Fuzzilli: FuzzIL-based generation, syntactic-correctness-by-construction, semantic-validity goal, REPRL execution | CONFIRMED | `https://raw.githubusercontent.com/googleprojectzero/fuzzilli/main/Docs/HowFuzzilliWorks.md` (inspected) |
| 18 | Miri detects UB classes but tests only one execution; passing ≠ sound | CONFIRMED | `https://raw.githubusercontent.com/rust-lang/miri/master/README.md` (inspected) |
| 19 | Kani: bit-precise model checker for Rust, safety + correctness harnesses | CONFIRMED | `https://raw.githubusercontent.com/model-checking/kani/main/README.md` (inspected) |
| 20 | Loom: permutes concurrent executions under C11 (with documented unsound/incomplete corners) | CONFIRMED | `https://raw.githubusercontent.com/tokio-rs/loom/master/README.md` (inspected) |
| 21 | cargo-fuzz/libFuzzer: coverage-guided fuzzing, `cmin`/`tmin`/`coverage` | CONFIRMED | `https://raw.githubusercontent.com/rust-fuzz/cargo-fuzz/main/README.md`, `https://llvm.org/docs/LibFuzzer.html` (inspected) |
| 22 | ASan/UBSan instrumented detection | CONFIRMED (official Clang docs; kept as tool references) | `https://clang.llvm.org/docs/AddressSanitizer.html`, `https://clang.llvm.org/docs/UndefinedBehaviorSanitizer.html` |
| 23 | WPT gives cross-browser confidence; WebKit requires tests with fixes | CONFIRMED | `https://web-platform-tests.org/`, `https://webkit.org/testing/` (inspected) |
| 24 | V8 is a C++/generational-GC engine; DOM comes from the embedder, not V8 | CONFIRMED | `https://v8.dev/docs` (inspected) |
| 25 | Rust `unsafe` transfers safety responsibility to the programmer (soundness obligation) | CONFIRMED | `https://doc.rust-lang.org/reference/behavior-considered-undefined.html` (inspected) |

### Explicitly unverifiable / out-of-scope items (honest gaps, none load-bearing)

- Exact open-issue examples (register forwarding, parser bugs, overflow bugs):
  issue-tracker contents were not enumerated; the plan does not depend on any
  specific issue, because Phase P0/P2 regenerates the defect list mechanically
  from Test262 + fuzzing + differential testing.
- A clean feature-by-feature list of the remaining ~4.4% Test262 gap: correctly
  stated by the discussion itself as unavailable in one place; Phase P2 produces
  it as an artifact (`docs/conformance-gap.md` + machine-readable matrix).
- Debugger/snapshot/sandbox/V8-tier internals: background context for Project-2,
  not step-0 material; not re-verified beyond items 24–25.

### Six corrections to the discussion's step-0 approach (all adopted in this plan)

1. **Extend, don't rebuild.** The discussion presents Miri, fuzzing, coverage,
   and Test262-comparison CI as things to create. They exist: a Miri CI job
   (`rust.yml:316-349`, running tests filtered by `miri`), three cargo-fuzz
   targets that are only *built* in CI but never *run* (`rust.yml:351-385`,
   `tests/fuzz/*`), a coverage job (`rust.yml:156`), and
   `boa_tester run|compare` (`CONTRIBUTING.md:67-118`). The plan extends each:
   run fuzzers on schedules with budgets, broaden the Miri filter, add sanitizer
   + stress-GC configs, and gate on `compare`.
2. **"100% of applicable Test262" must not be a hard gate for downstream work.**
   As a literal gate it blocks all progress on criteria outside our control
   (spec churn, test bugs, unsupported-proposal tests). Replace with: a pinned
   Test262 commit + a fully audited ignore list (every entry justified, linked,
   expiring) + a monotonic trend gate (pass count never decreases, ignore list
   only shrinks except by reviewed exception). Absolute conformance keeps rising
   as a *metric*; the *gate* is no-regression.
3. **Differential testing must start with one oracle, not three.** Standing up
   Boa↔V8↔SpiderMonkey↔JSC on day one triples harness/normalizer/triage work
   before any bug is found. Phase P3 starts with a single pinned oracle engine
   plus spec-anchored triage, and adds oracles only after the pipeline (runner,
   normalizer, minimizer, spec-verdict queue) is proven.
4. **Raw bytecode fuzzing needs a validity model first.** Generating arbitrary
   bytecode before documenting what `ByteCompiler::finish`/`CodeBlock`
   guarantee produces false positives that burn triage budget and erode trust in
   the fuzzer. Phase P5 first writes the validity model, then fuzzes
   valid-only generation (robustness) and invalid-input rejection (only for
   surfaces that actually accept untrusted bytecode, if any).
5. **Formal methods and mutation testing are misordered in the discussion.**
   Kani/Loom/mutation belong *after* the choke-point inventory and *after* the
   suites they measure exist: Kani harnesses target inventoried kernels
   (P6); Loom applies only if P0 finds real concurrent code (`boa_gc` is
   currently thread-local — `core/gc/src/lib.rs:44-51` — so Loom may be
   entirely out of scope); mutation testing measures suite strength, so it runs
   last (P7), not alongside initial test-writing.
6. **Missing operational machinery.** The discussion omits: toolchain/Test262
   version pins and lockfiles; crash/timeout/skip accounting in the tester
   (panics must fail loudly and be counted, never vanish from the denominator);
   a `cmin`/`tmin` corpus pipeline; performance baselines (fixes must not
   silently regress performance — criterion benches exist under `benches/`);
   a panic-trend gate; an `unsafe`-inventory gate; a spec-coverage feature
   matrix; and a host-boundary (`boa_runtime`/`boa_wintertc`/fetch) test
   strategy including WPT. All are explicit phase deliverables below.

### What we would do differently (net assessment)

Nothing structural: no alternative sequencing beats
inventory → harden harness → trend-gated conformance → differential →
fuzzing → validity models → unsafe/formal → coverage/mutation/gates, and no
prerequisite project is missing. The differences are the six corrections above
plus one emphasis shift: the discussion treats "high-confidence interpreter" as
the *output* of fixing bugs; this plan treats the *assurance system* (harness,
federation of oracles, fuzz fleet, gates) as the primary deliverable, with bug
fixes as its continuously-verified byproduct. That is what makes "only progress,
no regression" mechanically enforceable rather than aspirational.

## Success Criteria

Step 0 is complete when **all** of the following hold simultaneously on the
pinned toolchain and pinned Test262 commit (pins defined in P0). Each criterion
names the artifact or gate that proves it; Phase P7 wires them into one
readiness dashboard that must stay green.

1. **Reproducible baseline.** Toolchain, Test262 commit, oracle-engine builds,
   and dependency lockfile are pinned; two clean runs of the full gate battery
   on the same source commit produce identical verdicts (modulo explicitly
   allow-listed nondeterminism such as timing). Artifact: `docs/baseline.md` +
   lockfiles + CI config.
2. **No-silent-anything Test262 harness.** Every Test262 test in the pinned
   corpus is exactly one of: pass, spec-triaged fail with linked justification,
   or audited-ignore with justification + owner + expiry. Crashes, panics,
   timeouts, harness errors, and skips are counted categories that fail the gate
   unless individually triaged. The pass count is monotonic across commits
   (enforced by `boa_tester compare` in CI); the ignore list only shrinks
   except by reviewed exception. Artifact: hardened `tests/tester` + trend
   history.
3. **Permanent regression database.** Every defect found by any means (Test262,
   differential, fuzzing, review, issue tracker) becomes: minimal reproducer +
   unit test at the responsible layer + integration/Test262-style test where
   applicable + fuzz-corpus seed + a database entry (symptom, root cause, spec
   reference, fix commit). Re-running the database is a single command and part
   of every gate run. Artifact: `tests/regression/*` + `docs/regressions.md`
   (or a machine-readable equivalent).
4. **Differential agreement with an independent oracle.** A pinned oracle engine
   and Boa agree on the full differential corpus (Test262 programs +
   regression DB + generated corpus); every mismatch is triaged to a spec
   section with a verdict of Boa-bug / oracle-quirk / spec-ambiguity (with
   upstream Test262 issue where warranted). Zero untriaged mismatches.
   Artifact: differential harness + triage queue at zero.
5. **Continuously-running fuzz fleet with zero untriaged crashes.** Parser,
   bytecompiler, VM (extended to differential/semantic oracles, not crash-only),
   and GC-stress targets run on schedules with time/coverage budgets; every
   crash is minimized (`cmin`/`tmin`), deduplicated, entered into the regression
   DB, and fixed or triaged. Coverage of the fleet is tracked and non-decreasing.
6. **Memory-safety assurance.** Complete `unsafe` inventory (every site
   justified with a `SAFETY` argument, owner, and tests); Miri runs the unsafe
   core green; ASan/UBSan-instrumented fuzz + test runs green; zero known UB,
   zero unexplained panics in tested scope. `unsafe` inventory changes require
   audit sign-off.
7. **Proven critical kernels.** Bounded proofs (Kani) or exhaustive argument +
   tests exist for the inventoried kernels at minimum: `JsValue` tag
   encode/decode round-trip, GC rooting/tracing obligations for representative
   object graphs, and the bytecode validity model. Loom models exist for any
   concurrent code found in P0 (expected: none — then this criterion is
   satisfied by the documented negative result).
8. **Measured, mutation-adequate suites.** Line/branch coverage (existing
   coverage job, extended), opcode/operand coverage matrix, and semantic-feature
   matrix are published per commit and non-decreasing; mutation testing of the
   suites (semantic operators: comparison flips, barrier removal, lookup
   skipping, operand perturbation) achieves the adopted kill threshold, and
   every surviving mutant is either killed by a new test or justified in
   writing.
9. **No performance backsliding.** Criterion benches (`benches/`) plus a
   fixed smoke set run per commit with statistical comparison; fixes that
   regress beyond the adopted threshold require explicit sign-off. (Step 0 does
   not optimize; it forbids silent slowdowns.)
10. **Readiness gate green.** The P7 gate battery (conformance trend,
    differential, fuzz-budget, sanitizer, Miri, coverage, mutation-sample,
    perf-smoke, panic-trend) passes on the release candidate commit, reviewed
    and signed off. Downstream Project-1/Project-2 work may then begin — and
    this battery keeps running permanently.

## Non-goals (explicitly out of step 0)

- New JIT tiers, bytecode→IR→native work, deoptimization, tiering counters,
  feedback vectors beyond what testing needs (Project-2).
- GC redesign, generational/concurrent collection, allocation fast paths as
  features (GC *testing/stress* is in scope; GC *reimplementation* is not).
- WebAssembly engine or JS↔Wasm integration; new Web APIs; snapshot/startup
  systems; inspector/debugger features beyond what failure diagnosis needs.
- Absolute "100% Test262" as a blocking gate (see correction 2); absolute
  "zero bugs exist" claims of any kind (unprovable by testing — the
  discussion's Claim C — and not the target; the target is the aspiration in
  Goal, evidenced by criteria 1–10).

## Approach

### The assurance pyramid (what we build)

```text
                    ┌───────────────────────────┐
                    │ P7 readiness gate battery │  ← must stay green forever
                    └──────────────┬────────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              │                    │                    │
   ┌──────────▼─────────┐ ┌────────▼────────┐ ┌────────▼────────┐
   │ P4 fuzz fleet +    │ │ P3 differential │ │ P2 Test262 trend │
   │ P5 validity models │ │ vs oracle(s)    │ │ + gap closure    │
   └──────────┬─────────┘ └────────┬────────┘ └────────┬────────┘
              │                    │                    │
              └────────────────────┼────────────────────┘
                                   │
                    ┌──────────────▼───────────────┐
                    │ P1 hardened harness +        │
                    │ regression DB + CI gates     │
                    └──────────────┬───────────────┘
                                   │
                    ┌──────────────▼───────────────┐
                    │ P0 pins + inventory +        │
                    │ baselines (unsafe, choke     │
                    │ points, perf, conformance)   │
                    └──────────────────────────────┘
   P6 (unsafe audit, Miri, sanitizers, Kani, Loom-if-any) cross-cuts P0–P5
   and lands as gates in P7.
```

The dependency order is deliberately boring: nothing that finds bugs runs
before the machinery that records, minimizes, deduplicates, and
regression-tests them exists (P1 before P2–P5); nothing that judges
correctness runs before the baseline it compares against is pinned (P0
before everything).

### Key decisions (and rejected alternatives)

1. **Extend Boa's harness; do not build a parallel one.** `boa_tester
   run|compare`, `test262_config.toml`, `test262.yml`, the Miri/coverage/fuzz
   CI jobs, and the three fuzz targets are the foundation. Rejected: a new
   from-scratch runner (duplicates years of edge-case handling in flag parsing,
   negative tests, async/expected-failure semantics).
2. **Trend gates, not absolute gates.** Pin Test262; gate on monotonic
   pass-count and shrink-only ignore lists via `compare`. Rejected: "100% or
   blocked" (blocks on upstream spec/test churn) and "score-only" (hides
   denominator games).
3. **One oracle first.** P3 proves the differential pipeline against a single
   pinned oracle engine with spec-anchored triage; more oracles are added by
   repeating a proven recipe. Rejected: three-oracle day one (triples
   normalizer/triage work pre-value) and majority-vote verdicts (engines share
   bugs and spec misreadings; the spec, via Test262 + cited sections, is the
   judge).
4. **Crash-only fuzzing is insufficient; every fuzzer graduates to a semantic
   oracle.** The existing `vm-implied` target finds only crashes
   (`tests/fuzz/README.md:47-51`). P4 keeps crash detection but adds
   differential/semantic oracles (oracle engine, metamorphic equivalence,
   idempotency round-trips) so logic errors are found, not just panics.
5. **Validity model before bytecode fuzzing** (correction 4). Rejected: raw
   bytecode mutation first (false-positive triage swamp).
6. **Fix loop discipline: spec-cited, minimal, layered.** Every fix carries:
   the spec algorithm/section it implements, a minimal JS reproducer, a unit
   test at the responsible Rust layer, and (where meaningful) a Test262-style
   test + fuzz seed. Fixes without all four do not land. This is how agent
   velocity converts into assurance instead of churn.
7. **Unsafe is guilty until proven innocent.** Complete inventory with `SAFETY`
   justifications, Miri + sanitizers over the unsafe core, Kani for kernels,
   audit sign-off on inventory deltas. Rejected: "Rust is safe, skip it"
   (~770 `unsafe`-mentioning lines across `core/*`, concentrated in
   `boa_engine`, `boa_gc`, `boa_string`; the GC, value representation, and
   string interner are all `unsafe`-adjacent choke points).
8. **Loom is conditional.** If P0's concurrency inventory finds no real
   concurrent code (expected: `boa_gc` is thread-local), Loom is documented
   as out of scope rather than force-fit. Rejected: Loom-everywhere theater.
9. **Mutation testing last.** It measures suite strength, so it runs on mature
   suites (P7) with engine-specific operators, not generic line mutants only.
10. **Performance is a guardrail, not a goal.** Criterion baselines + smoke
    benches block silent slowdowns from fixes; no optimization work is
    scheduled. Rejected: "correctness first, perf later with no data" (loses
    the ability to attribute regressions to specific fixes).

### AI-agent execution contract (applies to every phase)

- Agents implement code, tests, harnesses, docs, and CI changes in small,
  reviewable units; every unit states which gate(s) it must pass.
- No unit lands unless the full fast gate set passes (unit tests, fmt, clippy,
  affected Test262 subset via `compare`, regression DB).
- Triage decisions (Boa-bug vs oracle-quirk vs spec-ambiguity; mutant
  kill/justify; ignore-list entries) are recorded with spec citations and are
  the only steps allowed to be slow.
- Anything flaky is quarantined immediately with an issue + expiry, and counts
  against the phase until fixed or formally owned; flakiness never becomes
  background noise.
- As-built deviation notes (append-only): during implementation, if anything
  must be done differently than the phase section lays out, never edit the
  existing plan text. Instead append a dated note to that phase's section
  stating exactly what was done differently and why, with evidence
  (file paths, commands, test names). Notes accumulate per phase; the
  original plan stays intact so later readers can see both the intent and
  what reality required.

### Universal regression rule (applies after every change and every phase)

1. **After every landed unit:** the full fast gate set passes — unit tests,
   fmt, clippy, regression DB, and the affected Test262 subset judged by
   `compare` against baseline (P1.5). No exceptions.
2. **After every phase:** the *entire* applicable battery passes, not just the
   phase's own checks — all prior phases' gates are re-run (prior Test262
   subsets go green again, regression DB grows and stays green, differential
   corpus stays at zero untriaged, fuzz smoke stays clean, sanitizer/Miri
   subsets stay green). Phase exit criteria are not met until this holds.
3. **Continuously:** the full battery (P7) runs on schedule and on every
   release-candidate commit; trend metrics (pass count, crash count, coverage,
   perf) are monotonic or explicitly signed off.
4. **If any regression occurs, the response playbook is, in order:**
   a. **Stop the line** — nothing further lands on red; the regressing unit is
   identified (small units + `compare` diffs make this mechanical).
   b. **Default to revert**, then re-land with the fix; forward-fixing on red
   is allowed only when the root cause is understood and the fix is smaller
   than the revert.
   c. **Never green by weakening** — no deleting/editing tests to fit new
   behavior, no new ignore entries to hide failures (P1.3 lint enforces this);
   behavior changes require a spec citation and review.
   d. **Enter the regression DB** (P1.1) — reproducer, layered tests, fuzz
   seed — so this exact regression is impossible forever.
   e. **Close the detection gap** — ask why no earlier layer caught it, and add
   the missing test/oracle/layer; a regression that teaches nothing is a
   process bug.
   f. **Quarantine is a last resort** — owner + expiry + counts against the
   phase (P1.6); flaky or timing-sensitive regressions do not become noise.

## Steps

### P0 — Pins, inventory, baselines (everything else stands on this)

**Goal.** Make every later measurement reproducible and map the codebase so
effort goes to choke points first. No behavior fixes here except those required
to make the harness deterministic.

**Work items.**

0.1. **Pin the world.** Record and enforce: Rust toolchain (`rust-toolchain.toml`
with pinned nightly for Miri/fuzz/sanitizer jobs + pinned stable for the rest),
Test262 commit (document exactly how the corpus is obtained today, then pin the
commit and verify the hash in CI), dependency `Cargo.lock` (already in-tree;
add a CI check that it is untouched by routine runs), and the oracle-engine
build(s) selected in P3 (pin when chosen; P0 defines the pinning mechanism, a
`docs/baseline.md` manifest + hash verification script).
0.2. **Conformance snapshot.** Run the full pinned Test262 corpus twice via
`cargo run --release --bin boa_tester -- run -o ./test-results-baseline`
(`CONTRIBUTING.md:97`) and store both result files as the immutable baseline;
any divergence between the two runs is itself a P0 bug (nondeterminism) to fix
or allow-list with justification.
0.3. **Static inventory of the engine.** Produce `docs/architecture-inventory.md`
mapping every crate and major module (parser, AST, bytecompiler, VM + each
opcode family, values, objects/property lookup/prototype chain, environments,
functions/closures, GC + rooting, strings/interner, RegExp, Intl/ICU data,
modules, job queue, `boa_runtime`/`boa_wintertc`/fetch host boundary, FFI,
CLI), each classified by: semantic criticality, memory-safety criticality,
concurrency, `unsafe` usage, external deps, current test coverage, and spec
coverage. Include the dependency graph (what feeds what) and the list of
single points of catastrophic failure (`JsValue` representation, GC rooting,
property lookup, bytecode dispatch, environment lifetime).
0.4. **Unsafe inventory (machine-readable).** Enumerate every `unsafe` block,
`unsafe fn`, and `unsafe impl` under `core/*` (starting point: ~770
`unsafe`-mentioning lines, concentrated in `boa_engine`, `boa_gc`,
`boa_string`) into `docs/unsafe-inventory.md` (or TOML/JSON): location,
reason, invariant relied upon, `SAFETY` justification status, covering tests,
owner. Anything without a justification is a P6 work item by construction.
0.5. **Concurrency inventory.** Enumerate threads, atomics, locks, channels, and
`Send`/`Sync` impls across the workspace; expected result is "effectively
single-threaded with thread-local GC" (`core/gc/src/lib.rs:44-51`) plus
explicitly listed exceptions (e.g., harness-level parallelism such as the
`rayon` workspace dependency, async runtimes in host code). This inventory
decides Loom's scope in P6.
0.6. **Performance baseline.** Run the full criterion suite (`benches/`) plus a
fixed JS smoke set on the pinned toolchain, store statistics as
`perf/baseline.json`; define the comparison procedure and regression threshold
used by every later phase. Also record Test262 full-run wall time (it bounds
how often the full gate can run).
0.7. **Host-boundary map.** Document what `boa_runtime` vs `boa_wintertc`
provide (module lists verified in-tree), what the CLI injects (`$boa`,
`$262`), and which surfaces accept untrusted input (JS source, module
specifiers, fetch responses, CLI flags, serialized data). This bounds the
adversarial-input testing in P4.

#### P0 implementation notes (grounded in current code)

**0.1 Pin the world.** Test262 is NOT a submodule/file dep: `tests/tester/src/main.rs:300` `clone_test262()` does `git clone https://github.com/tc39/test262` into `./test262` (`main.rs:181`) then `git reset --hard <commit>` (`main.rs:278`); pin lives in `test262_config.toml:1` (`commit="d86b2294..."`), overridable by `--test262-commit`/`--test262-path` (`main.rs:122`). No `rust-toolchain.toml` and no `.gitmodules` exist (observed absent at root). CI installs unpinned `stable` (`.github/workflows/test262.yml:28`) and compares PR output vs `boa-dev/data` (`test262.yml:55`). `Cargo.lock:1` is v4, in-tree. Pinning points: add `rust-toolchain.toml` (pinned stable + pinned nightly for Miri/fuzz/sanitizer jobs), new `docs/baseline.md` manifest {toolchain, test262 commit, oracle builds}, and a hash-verify step in `test262.yml:43` + `rust.yml` that asserts `git -C test262 rev-parse HEAD == test262_config.toml:1` and `git diff --exit-code Cargo.lock`. Keep `clone_test262` as the fetcher; verify-after-clone is less code than replacing it.
**0.2 Conformance snapshot.** Baseline cmds (`CONTRIBUTING.md:72`): `cargo run --release --bin boa_tester -- run -o ./test-results-baseline` twice; subset repro `run -vv -d -s test/language/types/number` (`CONTRIBUTING.md:91`). Outputs are `latest.json`/`results.json`/`features.json` (`tests/tester/src/results.rs:72`); only 4 counters exist: `Statistics{total,passed,ignored,panic}` (`main.rs:546`) + per-edition `VersionedStats es5..es17` (`main.rs:581`). Parallel by default via rayon (`exec/mod.rs:43`); determinism check = diff the two `latest.json` files (note: `get_test262_commit` reads `test262/.git/refs/heads/main` at `results.rs:162`, so a `--test262-path` without `.git` breaks `write_json` — pinning must keep the git dir). `compare` prints diffs only, no gating exit code (`results.rs:172`) — P1 extends it; P0 just stores both files immutably. Timeouts do not exist (`exec/mod.rs:283` `// TODO: timeout`).
**0.3 Static inventory.** Workspace: `Cargo.toml:1` members `core/*,ffi/*,tests/*,tools/*,examples,cli,utils/*,benches`, excluding `tests/fuzz,tests/src,tests/wpt` (`Cargo.toml:21`); `core/*` = ast,engine,gc,icu_provider,interner,macros,parser,runtime,string,wintertc; `utils/*` = tag_ptr,small_btree; v0.22.0/rust-1.91.0 (`Cargo.toml:27`). Engine pipeline modules: `core/engine/src/{bytecompiler,vm,optimizer,builtins,value,object,property,environments,module,context,realm.rs,job.rs}`; single-point-failure files to classify first: `value/*` (NaN-box vs `jsvalue-enum`: `core/engine/Cargo.toml:26`), `core/gc/src/lib.rs:44` (thread-local `BOA_GC`, 1MB threshold at `lib.rs:64`), property lookup/object shapes, `vm` dispatch. Inventory template row: `crate|module|feeds-into|semantic-criticality|unsafe-adjacent|threads/locks|Test262-vs-unit coverage|spec section`. Choke-point shortlist: `JsValue` codec, GC rooting (`Trace`/`Finalize`), property/shape lookup, `ByteCompiler::finish`→`CodeBlock` validity, module loader (`exec/mod.rs:666` shows `SimpleModuleLoader` + realm interplay).
**0.4 Unsafe inventory (machine-readable).** Enumerate every `unsafe` block, `unsafe fn`, and `unsafe impl` under `core/*` (starting point: ~770 `unsafe`-mentioning lines, concentrated in `boa_engine`, `boa_gc`, `boa_string`; verify by search, don't hand-count). Anchor entries on known sites: `exec/mod.rs:627` documents the `NativeFunction::from_closure` SAFETY pattern; `test262.rs:232` same for agent threads. Schema: `location|kind|invariant|SAFETY-status|covering-test|owner`. Extend in P6 with Miri/sanitizer columns; P0 only needs complete locations + status=`unjustified` default.
**0.5 Concurrency inventory.** Expected "effectively single-threaded": GC is `thread_local!` (`core/gc/src/lib.rs:44`); real threads exist only at harness level: rayon suite parallelism (`exec/mod.rs:43`, dep `Cargo.toml:98`), `$262.agent.start` spawns `std::thread` workers joined via `WorkerHandles::join_all` (`core/runtime/src/test262.rs:53`), `bus::Bus` broadcast (`test262.rs:228`), CLI REPL thread + `async_channel` (`cli/src/main.rs:815`) and `Executor` jobs (`cli/src/executor.rs`). Grep targets: `thread::spawn|Arc|Mutex|RwLock|Atomic|Send|Sync|tokio|smol` across `core/*,cli,examples`. If only the above surface, record Loom-out-of-scope with this evidence.
**0.6 Performance baseline.** `benches/Cargo.toml:22` single criterion bench `scripts` (`harness=false`); `benches/benches/scripts.rs:50` walks `benches/scripts/**/*.js` (basic,closures,intl,json,properties,prototypes,strings,v8-benches), group per relative path, `v8-benches` get `sample_size(10)` (`scripts.rs:77`); each script must define global `main()` called in `b.iter` (`scripts.rs:39`); setup parses+evaluates with optimizer OFF and `ConsoleExtension(NullLogger)` (`scripts.rs:22`). Baseline cmds: `cargo bench -p boa_benches [-- <regex>]` (regex filter at `scripts.rs:53`) and store criterion JSON + a new `perf/baseline.json` rollup; record Test262 full-run wall time from 0.2. Engine also has `[[bench]] full` (`core/engine/Cargo.toml:218`). Gotcha: benches build engine with `intl_bundled` (`benches/Cargo.toml:8`) — pin the same feature set in the comparison procedure or ICU data skews results.
**0.7 Host-boundary map.** `boa_runtime::register` (`core/runtime/src/lib.rs:172`) always installs Base64/Timeout/Encoding/Microtask/StructuredClone + `url`/`process`/`abort` by feature, then caller extensions via `RuntimeExtension` (`core/runtime/src/extensions.rs:10`, tuple impls to 12 at `extensions.rs:173`). `boa_runtime` re-exports TC55 APIs from `boa_wintertc` (`lib.rs:124`; timers aliased `interval` at `lib.rs:143`); own modules: `text,message,url,fetch,abort,process,test262`. `boa_wintertc::register` (`core/wintertc/src/lib.rs:63`) installs console/timers/encoding/microtask/clone/base64/abort + `url`/`fetch` by feature; modules at `wintertc/src/lib.rs:39`. CLI injects: always `console`(+fetch w/ `fetch` feature) via `add_runtime` (`cli/src/main.rs:830`); `--debug-object` → `$boa` (`main.rs:588`, `cli/src/debug/mod.rs:70` registers function/object/shape/optimizer/gc/realm/limits/string); `--test262-object` → `$262` + `print=console.log` (`main.rs:592`). `$262` API: createRealm/detachArrayBuffer/evalScript/gc/global/agent (`test262.rs:92`); agent start/broadcast/getReport/sleep/monotonicNow (`test262.rs:226`). Tester per-test context (`exec/mod.rs:522`): module loader rooted at test's parent dir, `can_block` from flags, `print()` async-signal fn (`exec/mod.rs:624`), `$262`, optional console, harness `assert.js/sta.js/doneprintHandle.js` + includes. Untrusted-input surfaces: JS source/module specifiers (loader root `cli/src/main.rs:169` `-r`, default `.`), fetch responses (`BlockingReqwestFetcher`, `cli/Cargo.toml:46`), CLI flags (`-e/FILE/stdin`, `main.rs:612`), `$262.evalScript` string eval. Fuzz/embedding entry points: `examples/src/bin/` (28 bins: `loadstring.rs,loadfile.rs,modules.rs,modulehandler.rs,tokio_event_loop.rs,smol_event_loop.rs,runtime_limits.rs` + js* builtin demos) with engine features `annex-b,float16,xsum,temporal` (`examples/Cargo.toml:13`); engine feature flags that change behavior under test: `default=float16,xsum,temporal`, `intl/intl_bundled,annex-b,experimental,js,flowgraph,trace,fuzz,deser,either,jsvalue-enum,system-time-zone,native-backtrace,embedded_lz4` (`core/engine/Cargo.toml:14`); tester defaults add `intl_bundled,experimental,annex-b` (`tests/tester/Cargo.toml:32`), CLI adds `deser,flowgraph,trace` + defaults incl. `native-backtrace,fast-allocator,fetch` (`cli/Cargo.toml:15`).
**Gotchas.** (1) `Ignored::contains_test` is substring match (`main.rs:83`) — audit entries can over-ignore. (2) `Ignored` features splits on `.` (`main.rs:98`) — `Intl` ignores all `Intl.*`. (3) Panics are caught per test (`exec/mod.rs:275`) and counted, but worker panics re-panic into the same path (`exec/mod.rs:345`). (4) No per-test timeout (`exec/mod.rs:283`). (5) `compare` never fails (`results.rs:172`) — trend gate is P1 work. (6) `get_test262_commit` assumes `.git/refs/heads/main` (`results.rs:162`). (7) `test262.yml:28` toolchain unpinned; `boa_tester` default features differ from CLI/benches — baseline must record the exact feature matrix.

**Validation / gate.**

- `cargo run --release --bin boa_tester -- run -o <out>` twice → byte-identical
  verdict files (or allow-listed nondeterminism with justification).
- `docs/baseline.md`, `docs/architecture-inventory.md`,
  `docs/unsafe-inventory.*`, concurrency inventory, `perf/baseline.json` exist
  and are reviewed.
- CI fails if lockfile/toolchain/Test262-commit verification fails.
- **Exit criteria:** all pins enforced in CI; inventories complete and
  reviewed; baselines stored; zero unexplained nondeterminism.

### P1 — Regression infrastructure + tester hardening (the machinery every later phase feeds)

**Goal.** Make it impossible for a found bug to silently return and impossible
for a failing/crashing/skipped test to silently disappear. No new bug-finding
yet — only the recording, minimizing, and gating machinery.

**Work items.**

1.1. **Regression database.** Create `tests/regression/` with a fixed entry
schema: id, minimal JS reproducer, responsible Rust layer + unit test path,
Test262-style test (where applicable), fuzz-corpus seed path, symptom, root
cause, spec citation, fix commit, finder (phase/tool). One command
(`cargo test -p boa_regression` or a documented `cargo run` equivalent) runs
the whole DB. Seed it by importing the existing issue-tracker bugs that have
reproducers (triage: reproduce-first; unreproducible entries stay open, never
silently dropped).
1.2. **Tester hardening: counted outcomes.** Extend `tests/tester` so every
test outcome is one of a closed, counted set: pass / fail-triaged /
fail-untriaged / crash / panic / timeout / harness-error / skip-triaged /
skip-untriaged. `compare` gains `--fail-on untriaged,crash,timeout,skip` style
strictness (exact flags to be designed against the current `compare`
implementation): any untriaged/crash/timeout/new-skip fails the run. Timeouts
get per-test budgets; hangs are killed and counted, never left to stall CI.
1.3. **Ignore-list audit format.** Every `test262_config.toml` ignore entry
must carry: reason, spec/test link, owner, expiry or re-review commit, and
the P2 work item that removes it. Add a CI check (`tester check-config` or a
lint script) rejecting unjustified entries, and a report command listing
ignore entries by age/area. No new ignores without review from this point on.
1.4. **Panic trend + crash accounting.** Panics/crashes become first-class
metrics: per-run counts by crate/module, stored alongside `latest.json`, with
a trend gate (counts only decrease; any new crash signature fails the gate
until triaged into the regression DB). Extend the existing panic logging in
the tester rather than replacing it.
1.5. **CI gating on `compare`.** Wire the fast path into `.github/workflows`:
every PR runs unit tests + fmt + clippy + the regression DB + affected
Test262 subsets with `compare` against the P0 baseline (no-regress verdict);
the full corpus + full battery runs on a schedule and on release-candidate
commits. Define "affected subset" selection (by touched crate/module mapping
from the P0 inventory) and prove it catches seeded regressions (deliberately
revert one fixed bug per area in a dry run and confirm the gate goes red).
1.6. **Flakiness protocol.** Any nondeterministic failure is quarantined the
same day with owner + expiry; quarantine list is public in-repo and itself
gated (no growth without sign-off). Deterministic reruns are the norm per P0;
this item handles what P0 missed.

#### P1 implementation notes (grounded in current code)

Crate: `boa_tester` (`tests/tester/Cargo.toml:2`, `publish=false`), bin `boa_tester`.
Modules: `main.rs` (CLI/config/types/schema), `exec/mod.rs` (runner), `read.rs`
(suite/test/YAML reader), `results.rs` (JSON writer + compare), `edition.rs`
(feature→edition map + `SpecEdition`).

**`run` subcommand + flags** (`tests/tester/src/main.rs:116-164`).
Flags: `-v/--verbose` count (`main.rs:118-119`); `--test262-path` x `--test262-commit`
(`main.rs:122-131`); `-s/--suite` default `"test"` (`main.rs:133-135`);
`-O/--optimize` (`main.rs:137-139`); `-o/--output` dir (`main.rs:141-143`);
`-d/--disable_parallelism` (`main.rs:145-147`); `-c/--config` default
`test262_config.toml` (`main.rs:149-151`); `--edition` (`SpecEdition`, default
`ESNext`, `main.rs:153-155`, `edition.rs:360-361`); `--versioned`; `--console`.
Config load: TOML→`Config` (`main.rs:201-207`); commit resolution
`--test262-commit` > config > fetch-latest (`main.rs:209-212`); clone/fetch/reset
via git CLI (`main.rs:300-385`).

**Exec: outcome taxonomy** (`tests/tester/src/exec/mod.rs:275-519`).
`TestOutcomeResult::{Passed,Ignored,Failed,Panic}` (`main.rs:794-804`); failed =
`total-passed-ignored` derived, not stored (`main.rs:475`, `results.rs:208-210`).
`Test::run` (`exec/mod.rs:166-195`): MODULE|RAW run once; STRICT|NO_STRICT runs
non-strict first, short-circuits on failure (`exec/mod.rs:177-185`) — one
`TestResult` per test, strictness not recorded. Negative phases Parse/Resolution/
Runtime handled inline (`exec/mod.rs:352-501`); `is_error_type` checks native
kind or ctor name (`exec/mod.rs:598-621`). Missing outcomes: no timeout, crash
(abort/OOM), harness-error, skip/triaged distinctions — P1.2 adds variants here
plus `Statistics` counters (`main.rs:546-556`), `Add/AddAssign` (`main.rs:558-578`),
aggregation (`exec/mod.rs:100-126`), and print arms (`exec/mod.rs:212-227,
main.rs:469-486,491-509`). P1 owns this taxonomy; P2.5 (WPT) and P3 (differential)
reuse these shared outcome types rather than inventing parallel ones.

**Panic capture** (`exec/mod.rs:275,504-508`): `catch_unwind` per `run_once`;
payload discarded (`String::new()`), path to stderr only. Worker panics
re-panicked (`exec/mod.rs:345,492`). P1.4: capture payload/backtrace, per-crate
attribution, persist beside `latest.json`.

**Timeouts: none — and the mechanism must be sound.** `// TODO: timeout`
(`exec/mod.rs:283`); only guard is the 60-min job timeout (`test262.yml:20`).
P1.2 (P1 owns the tester timeout mechanism; P3 reuses it): per-test wall-clock
budget enforced by running each test in a **subprocess** that is killed on
expiry — explicitly NOT "scoped thread + kill", which is unsound in Rust
(threads cannot be killed; scoped threads only join). Where subprocess isolation
is too coarse, compose with in-engine fuel (`RuntimeLimits` +
`instructions_remaining`, see P3/P4 notes) as a second layer. Killed runs must
still emit a `Timeout` outcome plus partial logs, never vanish.

**Parallelism**: rayon `par_iter` over suites and tests (`exec/mod.rs:43-85`);
serial via `-d`. Edition pre-filter `test.edition <= max_edition`
(`exec/mod.rs:76,82`).

**Results schema** (`tests/tester/src/results.rs:13-70`).
`latest.json` = `ResultInfo{c,u,r}` (`results.rs:15-22`); `SuiteResult{n,a,av,s,t,f}`
(`main.rs:761-778`); `TestResult{n,v,r}` (`main.rs:783-792`, `result_text`
`#[serde(skip)]`); outcomes `O/I/F/P` (`main.rs:795-804`); stats `t/o/i/p`
(`main.rs:547-556`); versioned `es5..es17` (`main.rs:581-596`, backfill deser
`main.rs:598-662`). `write_json` (`results.rs:84-159`): branches to
`GITHUB_REF` subdir, `refs/pull*`→`pull/` (`results.rs:90-101`); appends
`ReducedResultInfo` to `results.json`, `FeaturesInfo` to `features.json`.
`get_test262_commit` reads `.git/refs/heads/main` (`results.rs:162-168`) —
breaks on detached HEAD; P0 pin must keep a `main` ref or this fn must change.

**`compare`** (`results.rs:170-412`): accepts files or dirs (dir→`latest.json`,
`results.rs:173-184`); prints table + fixed/broken/new-panics/panic-fixes
(`compute_result_diff`, `results.rs:434-485`); **always exits 0**
(`results.rs:411`). P1.2/P1.5: add `--fail-on untriaged,crash,timeout,skip`
+ nonzero exit on breach (P1 owns `compare` strictness; P7 consumes the flag,
it adds no second one). Gotchas: `Ignored→Failed` silently dropped
(`results.rs:459`); diff iterates base only, new-only tests/suites invisible
(`results.rs:443,471`); text-mode diffs print `base-new`, markdown `new-base`
(`results.rs:362-376` vs `:255-284`).

**Ignore-list parsing** (`main.rs:48-109`, `read.rs:156-227`).
`Ignored{tests,features,flags}` (`main.rs:71-78`); `contains_test` = **substring**
match (`main.rs:87-89`); `contains_feature` exact + pre-dot prefix
(`main.rs:94-104`); flags bitflags custom deser (`main.rs:944-995`).
`read_suite`: dir-level ignore propagates to subtree (`read.rs:166,208-217`);
non-`.js` and `*FIXTURE` skipped silently (`read.rs:189-200`); unknown features
fail the whole run via `Test::new` (`main.rs:826-833`, `edition.rs:379`).
P1.3: change `tests`/`features` values to `{pattern,reason,link,owner,expiry,
work_item}` tables, keep substring semantics documented, add `tester
check-config` lint + age/area report; P1.6 quarantine list reuses same type.

**`test262_config.toml`** (`test262_config.toml:1-102`): `commit` pin (`:1`);
`[ignored] flags=[]` (`:5`), `features=[...]` (`:7-60`, bare strings + URL
comments), `tests=[...]` (`:62-102`, path prefixes incl. OOM/panic/ICU4X cases).
P1.3 migrates each to audited entries; no code reads comments — lint must enforce
machine-readable fields.

**`test262.yml`** (`.github/workflows/test262.yml:1-84`): on PR→`main|releases/**`
(`:3-7`), `contents:read` (`:9-10`), per-PR concurrency cancel (`:12-14`), one
job `run_test262` (`:17`), `cargo run --release --bin boa_tester -- run -v -o
../results/test262` (`:43-48`), `compare base pr -m` vs `boa-dev/data` main
(`:50-75`), uploads `comment.md+pr_number.txt` artifact (`:79-84`); comment
posted by `test262_comment.yml` on `workflow_run` completed (`:1-8`, gated on
success `:17`). P1.5: add unit/fmt/clippy/regression-DB + affected-subset
`compare --fail-on` jobs to PR path; full corpus+battery on schedule/release
commits (new workflow or extend `nightly_build.yml`).

**CONTRIBUTING commands** (`CONTRIBUTING.md:67-119`): full run
`cargo run --release --bin boa_tester -- run -v 2> error.log` (`:72`, panics→
stderr); `-vv` names, `-vvv` outputs (`:78-80`); subset `-s <rel-path>`
(`:82-85`); `-vv -d -s …` readable small runs (`:91`); `-o` save (`:97`);
`compare <base> <new>` files or dirs (`:105-119`).

**`-s/--suite` mechanics** (`main.rs:133-135,416-439`): path relative to test262
root; ends `.js` → single test, stdout only, **no JSON/stats/compare output**
(`main.rs:416-433`); else directory subtree via `read_suite` + `SuiteResult::run`
(`main.rs:435-451`). P1.5 affected-subset gating: map touched crates→suite
prefixes (dir-level `-s` runs, each with `-o` for `compare`); prove with seeded
reverts per area (P1 gate drill). Gotcha: single-file mode unusable for gating;
also `-s` + `--edition` interact (edition filter applies after load).

**P1.1 regression DB**: new `tests/regression/` crate (`cargo test -p
boa_regression`); entry schema per plan; seed from issue tracker
reproduce-first. Reuses `compare` strict mode + `-s` subsets for the fast gate.
P1.1 additionally defines the shared JSON-store conventions (stable test-id,
closed outcome-enum subset, spec-ref, timestamps, tool version) that the P2.1
gap matrix and P3.4 triage queue schemas must reuse, so all three stores stay
mutually queryable.

**Extension-point summary**: counted outcomes→`main.rs:794-804,546-578` +
`exec/mod.rs:100-126,166-227,504-518`; strict compare→`results.rs:170-220,411-485`
+ `Cli::Compare` (`main.rs:165-178,241-246`); config lint→new subcommand +
`main.rs:48-109`; crash accounting→`exec/mod.rs:504-508` + `write_json`
(`results.rs:84-159`); CI→`test262.yml` + `test262_comment.yml:17` gate.

**Validation / gate.**

- Seeded-regression drill: inject one known bug per major area (parser, VM,
  builtins, GC stress, host) → fast gate red on all five; remove → green.
- `compare` strict mode fails a run containing one crash, one timeout, and one
  new skip (synthetic fixtures).
- Ignore-list lint rejects an unjustified entry; accepts a fully-justified one.
- **Exit criteria:** regression DB runnable in one command; tester has no
  uncounted outcome; `compare`-gated CI live; seeded drill green-to-red-to-green
  demonstrated.

#### P1 as-built notes (appended 2026-10-02; append-only, original text above unchanged)

1. **Triaged/untriaged + skip distinctions are compare-time metadata, not
   outcome variants (P1.2).** The goal text lists fail-triaged /
   fail-untriaged / skip-triaged / skip-untriaged among "a closed,
   counted set" and the notes say "P1.2 adds variants here".
   Implemented instead: `TestOutcomeResult` gained only
   `Timeout`/`Crash`/`HarnessError` (joining
   Passed/Failed/Ignored/Panic); triaged state lives in the `--triage`
   JSON store and quarantine in `test262_quarantine.toml`, joined at
   compare time (`FailCondition::Untriaged`, quarantine exemptions in
   `evaluate_fail_conditions`, `tests/tester/src/results.rs:768`).
   Why: triage/quarantine are mutable workflow states — a test must be
   triageable without re-execution, and run artifacts (`latest.json`)
   stay immutable records of what happened. No uncounted outcome:
   every test still lands in exactly one stored variant, and
   `strict_compare_breaches_on_crash_timeout_and_new_skip` plus
   `quarantine_and_triage_exempt_breaches` prove the gating.
2. **Seeded drill used synthetic injections per area plus one real
   revert, not one real revert per area (P1.5).** Attempted literally:
   four regression-DB-seed reverts were tried (`9f5521bd`,
   `970ea933`, `a93e0b3f`, `5cee37eb`); three are Test262-invisible
   (message-only change; uncovered edges), so they cannot redden any
   subset gate. This itself proved the DB gate's complementary value:
   the `970ea933` revert fails `cargo test -p boa_regression`
   (`try-finally-pending-return: ... 43 !== 42`) while the subset gate
   stays green. Synthetic injections give deterministic per-area
   breach proof with exact-CI fidelity (mapped subsets from
   `scripts/affected-subsets.sh`, exact `run -v --timeout 60` +
   `compare --fail-on=regression` invocation); the `5cee37eb` revert
   (5 broken with-statement tests) is the plan-literal case. Evidence:
   `docs/p1-seeded-drill.md` (6/6 red, then green).
3. **Child-output pipe protocol hardened beyond the plan (P1.2).** The
   plan mandates subprocess isolation but does not specify the pipe
   protocol. The drill exposed a deadlock: children emitting more
   than the ~64 KiB OS pipe buffer (87 KiB completion-value dump from
   `test/staging/sm/regress/regress-567152`) blocked mid-write while
   the parent polled without draining, and were misreported as
   `Timeout` — including on the clean tree. Fix: the child truncates
   outcome text to 4 KiB before serializing
   (`run_single`, `tests/tester/src/main.rs`) and the parent drains
   both pipes on reader threads (`run_child`,
   `tests/tester/src/exec/mod.rs`). Covered by the hermetic
   `large_child_output_completes_without_timeout` e2e
   (`tests/tester/tests/isolation.rs`). This preserves the plan's
   requirement ("hangs are killed and counted") rather than changing
   it.
4. **Crash-signature gating arms when both sides have `crashes.json`;
   P0 run-1 predates it (P1.4).** Per-test crash transitions gate
   against run-1 today; the signature-level breach (same test,
   changed crash — wired into `FailCondition::Crash`/`Regression`
   during implementation, tested by
   `changed_crash_signature_breaches_on_same_outcome`) fires once a
   `crashes.json`-bearing baseline exists. Missing files skip with a
   printed note, never fail. Signature exemption uses quarantine
   substring matching, since the id-based triage store cannot address
   signatures.
5. **P1.6 "no growth without sign-off" = PR review + `check-config`
   lint in CI.** The quarantine file is public in-repo
   (`test262_quarantine.toml`, 0 entries); every entry must carry
   owner/expiry/justification or the `check-config` CI step
   (`.github/workflows/test262.yml`) fails; additions land only via
   reviewed PRs. No separate bot/CODEOWNERS mechanism was added.
6. **Within-latitude choices, recorded for clarity (not deviations):**
   `--fail-on` vocabulary (`regression` shorthand covering
   broken/new-abnormal/new-skip; plan left exact flags to design);
   new `test262_full.yml` scheduled workflow (plan allowed "new
   workflow or extend `nightly_build.yml`"); affected-subset mapping
   with full-corpus fallback for value/object/environments/gc/string
   changes; crash signature format `kind@crate: first-line`;
   `crashes.json` always written. The DB seeded with 4 entries;
   continued seeding happens per-fix via the universal regression
   rule (4d), not by back-importing the whole tracker in P1.

### P2 — Test262 trend gate + systematic gap closure (conformance becomes a ratchet)

**Goal.** Turn the ~95% conformance figure into a monotonic ratchet and
systematically close the gap area by area, with every fix carrying the full
four-artifact discipline (spec citation, minimal reproducer, layered tests,
fuzz seed).

**Work items.**

2.1. **Conformance-gap matrix.** From the P0 baseline + audited ignore list,
generate `docs/conformance-gap.md` and a machine-readable matrix: every
failing/ignored test mapped to spec area (grammar production, builtin,
abstract operation), suspected root-cause layer, and owning P2 work item.
This is the feature-by-feature list the discussion correctly notes does not
exist anywhere today — P2 creates and maintains it.
2.2. **Fix loop per area, choke points first.** Order areas by the P0
inventory (values/objects/property lookup/environments/VM dispatch first;
isolated builtins later). For each defect: reproduce minimally → cite the
spec algorithm → fix at the responsible layer → add unit + Test262-style
tests → add fuzz seed → enter regression DB → run affected Test262 subset
with `compare` (must show strictly-fewer failures, zero new ones) → land.
2.3. **Spec-driven supplementary tests.** Where Test262 is thin (TC39's own
caveat: coverage is broad but explicitly not complete), write targeted tests
from the spec text for the area under repair — especially interaction seams
Test262 under-covers (Proxy × accessors × prototype mutation; async ×
microtask ordering; WeakRef/finalization timing boundaries; module
namespaces × circular imports). Each cites its spec section.
2.4. **Upstream feedback.** Tests that look wrong (not merely failing) get
minimal reproducers and are filed upstream to tc39/test262 with a local
audited-ignore + expiry while pending; verdicts on return update the matrix.
2.5. **Host/API conformance.** `boa_runtime`/`boa_wintertc` behavior is
covered by unit/integration tests per API plus applicable WPT suites
(WPT exists precisely to give implementations cross-browser confidence —
`https://web-platform-tests.org/`); WPT results get the same counted-outcome
treatment as Test262 (P1 machinery reused, separate corpus pin).

#### P2 implementation notes (grounded in current code)

**1. Builtins organization (where fixes land).** One module per object under
`core/engine/src/builtins/` (~35 mods, `mod.rs:3-55`; feature-gated `escape`,
`intl`, `temporal` at `mod.rs:46-55`). Pattern per object, sampled from String:
`pub(crate) struct String` + `impl IntrinsicObject for String { fn init(realm) }`
(`builtins/string/mod.rs:60-65`) wiring every member through `BuiltInBuilder`
(`.method/.static_method/.property`, `string/mod.rs:83-110`); each op carries a
`[spec]: https://tc39.es/...` doc link plus numbered `// N.` step comments
mirroring the algorithm (`string/mod.rs:235-271`, `string_create`); unit tests
live in a sibling `#[cfg(test)] mod tests` (`string/mod.rs:51-52`). Fix loop per
area: reproduce via `boa_tester run -s <subpath>` -> cite spec section -> edit
the builtin op keeping step comments in sync -> extend sibling `tests.rs` ->
`compare` before/after.

**2. `test262_config.toml`: small, unaudited, substring-matched.** Pin
`commit = "d86b229…"` (`test262_config.toml:1`); `[ignored]` holds `flags = []`,
~19 `features` (`:7-60`: 7 unimplemented + 11 pending proposals + `caller`), and
~28 `tests` paths (`:62-101`), several with bare `# TODO` comments and no owner/
expiry. Matching is loose: test ignores are substring matches
(`tests/tester/src/main.rs:83-90`), feature ignores match `foo` for `foo.bar`
(`main.rs:94-104`). P2.1 input: this file as-is; P1.3 (which precedes P2) adds
the audit schema — P2 must not silently grow the list (additions need
reason+owner+expiry even before the lint exists).

**3. Tester mechanics to reuse (no new runner).** Binary `boa_tester`
(`tests/tester/Cargo.toml:1-2`); CLI `run|compare` (`main.rs:114-179`):
`-s/--suite` runs one subpath (`main.rs:133-135`,
`CONTRIBUTING.md:82-91`), `-o` writes JSON (`main.rs:142-143`), `-c` selects the
TOML (`main.rs:150-151`). `run` auto-clones/pins test262 to `./test262`
(`main.rs:300-385`). Results schema: `Statistics{total,passed,ignored,panic}`
(`main.rs:546-556`), `TestOutcomeResult::{Passed,Ignored,Failed,Panic}`
(`main.rs:794-804`), nested `SuiteResult` (`main.rs:761-778`) serialized with
short keys (`n/a/av/s/t/f/r`, `results.rs:13-35,761-804`) into `latest.json`
(full) + `results.json` (reduced history) + `features.json`
(`results.rs:72-159`). `compare base new` diffs counts and lists
fixed/broken/new-panics/panic-fixes (`results.rs:170-220,414-485`); note
`Ignored->Failed` is deliberately not reported as broken (`results.rs:459`) —
un-ignoring without fixing is invisible to `compare`, so the gap matrix must
track ignore removals separately. CI today only *comments* PR conformance
(`.github/workflows/test262.yml:43-78`, base from `boa-dev/data`); `compare`
always exits 0 (`results.rs:411`), so P2's trend gate needs a new strict/fail
flag (P1.5 owns it; P2 consumes it).

**4. Gap-matrix generation (P2.1).** Inputs: P0 baseline `latest.json`
(RunResult tree with per-test `r` + path) + `test262_config.toml` + live
`test262/` metadata (`test.features`, `test.flags`, `esid` parsed in
`main.rs:806-847`). Approach: a small tool (extend `boa_tester` with a
`report` subcommand next to `run|compare`, `main.rs:114-179`) that walks the
`SuiteResult` tree, joins each `F`/`P`/`I` leaf with its `features` set
(already collected per suite, `main.rs:775-777`, `features.json`) and its
directory prefix (`test/built-ins/<Builtin>/…` -> builtin area; `test/language/…`
-> parser/bytecompiler/VM by `esid`/feature), and emits
`docs/conformance-gap.md` + a machine-readable JSON mapping
test -> {spec area, ignore reason or fail/panic, suspected layer, P2 work item}.
The JSON schema must follow the shared P1.1 store conventions (stable test-id,
outcome enum, spec-ref, timestamps). Regenerate on every full run; diff against
the previous matrix to prove monotonicity.

**5. Fix-loop mechanics per area (P2.2-2.4).** Recent conformance-fix shape
(`git log --oneline`, Sep 2026): `fix(string)` touched `core/string/src/str.rs`
+ `core/string/src/tests.rs` (a93e0b3f) — note the StringToNumber kernel lives
in `boa_string`, not `builtins/string`; `fix(engine)` try/finally touched
`bytecompiler/*` + `core/engine/src/tests/control_flow/mod.rs` (970ea933);
`fix(engine)` with-statement touched bytecompiler+vm+environments plus an
`insta-bytecode` snapshot (5cee37eb). So: choke-point fixes (values, property
lookup, environments, dispatch) land in `core/engine/src/{bytecompiler,vm,
environments,object}` with tests in `core/engine/src/tests/...`; isolated
builtin fixes land in `builtins/<obj>/mod.rs` + sibling `tests.rs`, except
string/number kernels which live in `core/string`. Bytecode-shape changes must
update `tests/insta-bytecode` snapshots. Every fix: `run -s <affected>` +
`compare` must show strictly-fewer failures, zero new `F`/`P`; upstream-suspect
tests get a minimal reproducer filed to tc39/test262 + audited-ignore with
expiry (P2.4). Spec-seam tests (P2.3) go in `core/engine/src/tests/` with
`// https://tc39.es/...#sec-...` citations, following the existing step-comment
idiom.

**6. WPT current state + integration points (P2.5).** `tests/wpt` (`boa_wpt`
crate) runs WPT *today* as plain `cargo test -p boa_wpt`: `build.rs` shallow-
clones `web-platform-tests/wpt` at `rev` from `test_wpt_config.toml:1` into
`../../tests_wpt` and exports `WPT_ROOT` (`build.rs:12-83`); `lib.rs` builds a
`Context` with `boa_runtime` console+fetch extensions, `self`/`location`
globals, and `testharness.js` (`lib.rs:159-202`); each `#[rstest]` case evals
one `.any.js` file with a 10 s watchdog (`lib.rs:279-372`). Active suites:
`console`, `encoding` (api-* + one ctor file), `url` (3 excludes), `timers`
(`lib.rs:375-447`); `fetch` is `#[ignore]` (`lib.rs:424`). Gotchas: (a) no CI
workflow references WPT at all (checked `.github/workflows/`); (b) no counted
outcomes — `result_callback__` does `assert_eq!(status, Pass)` so the first
failing subtest panics the whole test binary (`lib.rs:205-218`) and failures
are invisible as data; (c) rev pin has no hash verification and no ignore/audit
format (`test_wpt_config.toml` is one line). Integration: add a counted-outcome
collector reusing the P1.2 outcome types (depend on them; do not duplicate the
taxonomy) + JSON writer mirroring `results.rs:84-159`, port the
`test262_config.toml` ignore schema (subset: tests + reason/owner/expiry) to
`test_wpt_config.toml`, then add a scheduled CI job running
`cargo test -p boa_wpt` and gating on zero-untriaged exactly like the
Test262 trend gate.

**Validation / gate.**

- After each area: affected-subset `compare` shows failures strictly
  decreasing, passes non-decreasing, zero new untriaged outcomes.
- Weekly full-corpus run: global pass count monotonic; ignore list shrinks or
  holds with justification; panic/crash counts non-increasing.
- Every landed fix references its regression-DB entry; audit script verifies
  the four artifacts exist (reproducer, unit test, Test262-style test or
  written justification, fuzz seed).
- **Exit criteria:** gap matrix complete and current; all P2-planned areas
  closed or explicitly deferred with justification; trend gates green for the
  trailing window (e.g., 50 consecutive full runs with zero untriaged
  outcomes); WPT host suites integrated.

#### P2 as-built notes (appended 2026-10-02; append-only, original text above unchanged)

1. **2.2 scoping: machinery complete, fix loop proven on 2 areas, rest
   triaged in the matrix.** Full gap closure (493 failed + 1648 ignored)
   is the months-long project-1 work, not one foundation session. P2
   delivered the ratchet machinery (`report` + `--check` trend gate,
   `docs/conformance-gap.{json,md}`, `docs/gap-areas.toml`, four-artifact
   audit) and proved the fix loop end-to-end on `builtin:Promise`
   (Promise.try final-algorithm rewrite, 2F fixed) and
   `language:identifier-resolution` (capture-time resolvability in
   SetNameByLocator, 1F fixed), each with reproducer-first proof, spec
   citation, unit test, DB entry, fuzz seed, and strictly-fewer/zero-new
   subset/full compares. Big clusters were triaged, not closed:
   `intl:NumberFormat` (119F) clusters on spec revisions + upstream ICU4X
   notation gaps (a workstream, recorded in the area notes). All 53 gap
   areas are `open`/`deferred`/`closed` with justification in
   `gap-areas.toml` — the matrix IS the exit criteria's deferral record.
   Result: 51437 -> 51440 passed, 493 -> 490 failed.
2. **Trailing window is CI-owned.** "50 consecutive full runs" is
   time-based and cannot be produced in-session; demonstrated 2
   consecutive green full runs plus the refresh workflow
   (CONTRIBUTING "Conformance trend gate"). The scheduled
   `test262-full` job (now with `report --check`) produces the window
   going forward.
3. **Area mapping is path-prefix; esid/feature refinement deferred.**
   The plan sketches `test/language/... -> parser/bytecompiler/VM by
   esid/feature`. Implemented deterministic path-prefix areas
   (`builtin:<B>`, `language:<seg>`, `intl:<seg>`, `staging:<seg>`,
   `annexB:<seg>`); features/esid recorded per entry for human triage.
   Why: esids are free-text spec titles, unreliable for automation.
4. **Triage curated per area, not per test.** The plan says the matrix
   maps "each failing/ignored test" to layer/work-item. The tool maps
   each gap test to area + features + edition + flags + ignore
   attribution automatically; layer/work-item/status/notes are curated
   per AREA in `docs/gap-areas.toml` and joined at render time. Why:
   per-test curation at ~2100 entries does not scale and would be wiped
   by regeneration; area-level is the maintainable granularity.
5. **WPT plan-reality skews.** (a) No `execute_async_test_file` /
   `cli_wpt` exist — all suites run synchronously through
   `execute_test_file`. (b) `tests/wpt` is workspace-EXCLUDED (own
   lockfile, edition 2021, no workspace lints); WPT commands run with
   `tests/wpt` as CWD. (c) The wintertc abort/encoding/url/fetch/clone/
   events modules are no-op stubs (migration TODOs) with nothing to
   unit-test; real behavior lives in `boa_runtime` (already tested) and
   the WPT suites. The missing unit tests were added where behavior
   lives instead: 9 `JsValueStore` round-trip tests in
   `core/wintertc/src/store/tests.rs`. Wintertc stub migration itself
   is engine work, out of P2 scope.
6. **Fuzz-seed artifact defined.** Raw-JS corpus staged under
   `tests/fuzz/seeds/` (P4 wires execution); `fuzz_seed` is now
   required and file-must-exist; the 4 P1 seeds were backfilled. The
   four-artifact audit is `scripts/audit-fix.sh <id>` (checklist) over
   the strict `cargo test -p boa_regression` schema (enforcement).
7. **Upstream (2.4): no filings arose.** All triaged gaps classified as
   engine bugs, upstream blocks (ICU4X), pending proposals, or correct
   ignores — no wrong-looking tests encountered. The process (prepare
   reproducer + spec citation in-loop; human files; link + expiry on
   the ignore entry) is documented in CONTRIBUTING "Upstream test
   feedback".
8. **Spec-seam tests (2.3): not needed for the closed areas.**
   `test/built-ins/Promise/try` (15 tests) and the
   `identifier-resolution` dir cover their areas; the seams are pinned
   by the new unit tests + DB repros instead.
9. **WPT gate design details.** Ignored files are SKIPPED (never
   executed) and recorded with their matched pattern; `wpt-report`
   breaches on stale patterns. Records append to `$WPT_OUT` under a
   process lock. Harness-*setup* failures (missing testharness.js,
   unregistrable callbacks) still panic by design — only verdicts are
   counted. Compile-time rstest excludes with TODOs were migrated to
   audited config ignores; `idlharness` (non-test helpers) and the
   `fetch` `#[ignore]` (needs a web server) stay as-is.
10. **Within-latitude choices (not deviations):** matrix committed at
    `docs/conformance-gap.{json,md}`; counts-based `--check`
    (suite-root + total comparability guard); new scheduled `wpt.yml`
    workflow; crash-signature column carried per abnormal entry when
    `crashes.json` exists; shared taxonomy extracted to the new
    `tests/outcomes` (`boa_outcomes`) crate per 2.5's depend-don't-
    duplicate rule.

### P3 — Differential testing v1: one oracle, proven pipeline, spec-anchored triage

**Goal.** Build the single highest-value bug-finding layer: run the same
programs on Boa and one pinned independent oracle engine, normalize observable
behavior, and triage every mismatch against the spec. Prove the pipeline
small, then expand oracles by repetition.

**Work items.**

3.1. **Oracle selection + pinning.** Evaluate candidate independent engines
runnable headless in CI (typical candidates to evaluate include V8's `d8`,
SpiderMonkey's `jsshell`, and JavaScriptCore's `jsc`) against: build/pin
cost, startup speed (differential runs millions of programs), and output
normalizability. Select ONE for v1; pin the exact build (hash + build flags
in `docs/baseline.md`); document the runner contract so adding oracles later
is mechanical. (V8 background: `https://v8.dev/docs`.)
3.2. **Runner + normalizer.** Build `tools/differential/` (or extend the
tester if cleaner): executes each program under Boa and the oracle with
identical timeouts/resource caps; captures return value, thrown value/type,
`console` output, and a canonicalized observable-state dump (global bindings,
property enumeration order where deterministic); normalizes known-benign
differences (error message text, stack-trace formats, timing/identity values)
via an explicit, reviewed normalization table — never by silently ignoring
fields.
3.3. **Seed corpora.** v1 corpus = full pinned Test262 programs + regression
DB reproducers + a generated corpus (P4 generators feed back here once they
exist; until then, a curated adversarial set covering the P0 choke points and
P2 interaction seams).
3.4. **Triage queue with spec verdicts.** Every mismatch enters a queue with:
minimized program (see P4 `tmin` pipeline), both outputs, suspected area, and
— before closure — a verdict of Boa-bug (→ regression DB + fix loop),
oracle-quirk (pinned-oracle-specific, documented, re-checked on oracle
upgrades), or spec-ambiguity (filed upstream where appropriate, audited
exception with expiry). Majority vote is never a verdict; the spec section is.
3.5. **Oracle expansion (after v1 proves out).** Add a second oracle by
repeating 3.1–3.4; promote cross-oracle disagreements involving Boa to the
same triage queue. More oracles follow the same recipe on a schedule, not all
at once.

#### P3 implementation notes (grounded in current code)

Build as a new `tools/differential/` crate reusing `boa_engine` + `boa_runtime`
in-process for the Boa side and a subprocess for the oracle.

**1. Boa-side headless driver — reuse `Context::eval`, not the CLI**
- Drive Boa in-process: `Context::eval(Source)` at `core/engine/src/context/mod.rs:205`
  (`Script::parse` + `evaluate`). `boa_engine` re-exports `Source` + prelude at
  `core/engine/src/lib.rs:146`, `core/engine/src/lib.rs:149`, so the harness depends only
  on `boa_engine` (+ `boa_runtime` for console) — same as `examples/src/bin/loadstring.rs:12`.
- Feed programs via `Source::from_bytes` (`core/parser/src/source/mod.rs:39`) for generated
  corpora, `Source::from_filepath` (`core/parser/src/source/mod.rs:87`) for Test262 files,
  `Source::from_reader` (`core/parser/src/source/mod.rs:115`) for composed harness+test input.
- Mimic the tester's context setup, `Test::create_context` at
  `tests/tester/src/exec/mod.rs:522`: `Context::builder()` (`tests/tester/src/exec/mod.rs:534`),
  `SimpleModuleLoader` per test dir (`tests/tester/src/exec/mod.rs:530`), `can_block` flag
  (`tests/tester/src/exec/mod.rs:536`), `print()` capture fn (`tests/tester/src/exec/mod.rs:624`),
  `$262` registration (`tests/tester/src/exec/mod.rs:546`), optional `Console`
  (`tests/tester/src/exec/mod.rs:549`). Reuse pattern `parse_module_and_register`
  (`tests/tester/src/exec/mod.rs:666`) for module-mode programs.
- Job draining: call `context.run_jobs()` (`core/engine/src/context/mod.rs:499`) after eval,
  exactly as the tester does (`tests/tester/src/exec/mod.rs:327`). Keep the default
  `SimpleJobExecutor` (`core/engine/src/job.rs:870`, `core/engine/src/job.rs:897`) — do NOT
  reuse the CLI's smol `Executor` (`cli/src/executor.rs:258`), which blocks on an idle-event
  loop and never terminates headless. Prefer `IdleJobExecutor`/fixed clock for determinism
  (tester precedent: `FixedClock::from_millis`, `core/engine/src/context/time.rs:217`).
- Strictness: `context.strict(bool)` (`core/engine/src/context/mod.rs:487`); tester runs each
  test in both modes unless flagged (`tests/tester/src/exec/mod.rs:166`).

**2. Oracle side + runner contract (P3.1, P3.2)**
- Oracle = pinned headless binary (`d8`/`jsshell`/`jsc`) invoked as a subprocess with the
  program on stdin/file; compare against the same per-case timeout (P1.2 mechanism, see §4).
  Runner contract (for P3.5 second-oracle repetition): `run_oracle(binary, program, timeout)
  -> OracleOutcome { exit_code, stdout, stderr }`; record binary hash + flags in
  `docs/baseline.md`.
- Boa CLI is a usable fallback oracle-style interface but not the primary path: file eval at
  `cli/src/main.rs:449`, dispatch at `cli/src/main.rs:612`; exit code is 0 on success, nonzero
  via `color_eyre::Result` (`cli/src/main.rs:551`) on parse/eval/job errors
  (`cli/src/main.rs:426`, `cli/src/main.rs:530`). Note it prints the completion value only when
  non-`undefined` (`cli/src/main.rs:524`) — a diff harness must not rely on that filtering.

**3. Observable-state capture points (P3.2)**
- Return value: `JsValue::display()` (`core/engine/src/value/mod.rs:964`) for canonical text;
  `JsValue::to_string(context)` (`core/engine/src/value/mod.rs:974`) for `String(x)` semantics.
- Thrown errors — classify, don't string-compare: `JsError::try_native`
  (`core/engine/src/error/mod.rs:557`) → kind+message (tester precedent
  `tests/tester/src/exec/mod.rs:598` checks only `kind` vs expected `ErrorType`);
  `as_opaque` (`core/engine/src/error/mod.rs:675`), `as_engine`
  (`core/engine/src/error/mod.rs:718`), `into_erased` (`core/engine/src/error/mod.rs:755`).
  `Display for JsError` (`core/engine/src/error/mod.rs:856`) appends a backtrace — strip or
  normalize it, never compare raw.
- Console capture: implement `Logger` (`core/wintertc/src/console/mod.rs:57`, `log` at
  `core/wintertc/src/console/mod.rs:91`) backed by an `Rc<RefCell<Vec<String>>>` buffer and
  install via `Console::register_with_logger` (`core/wintertc/src/console/mod.rs:340`) /
  `init_with_logger` (`core/wintertc/src/console/mod.rs:356`); `NullLogger`
  (`core/wintertc/src/console/mod.rs:169`) is the no-op precedent.
- Async completion: reuse the tester's `print("Test262:AsyncTestComplete")` protocol
  (`tests/tester/src/exec/mod.rs:639`) plus `run_jobs` error capture (`tests/tester/src/exec/mod.rs:327`).
- Canonical state dump: eval a fixed epilogue (`Object.keys(globalThis)` sorted,
  JSON of named bindings) through the same `Context` — same technique as the unit-test
  `AssertWithOp`/`InspectContext` hooks (`core/engine/src/lib.rs:238`, `core/engine/src/lib.rs:225`).

**4. Timeout / resource-cap mechanisms (all exist — compose them; P1 owns the tester mechanism, P3 reuses it)**
- `RuntimeLimits` (`core/engine/src/vm/runtime_limits.rs:3`; defaults `core/engine/src/vm/runtime_limits.rs:17`):
  `set_loop_iteration_limit` (`core/engine/src/vm/runtime_limits.rs:47`),
  `set_recursion_limit` (`core/engine/src/vm/runtime_limits.rs:94`),
  `set_stack_size_limit` (`core/engine/src/vm/runtime_limits.rs:81`),
  `set_backtrace_limit` (`core/engine/src/vm/runtime_limits.rs:68`), via
  `runtime_limits_mut` (`core/engine/src/context/mod.rs:601`). Usage precedent:
  `examples/src/bin/runtime_limits.rs:12`, `examples/src/bin/runtime_limits.rs:75`.
  Violations surface as `RuntimeLimitError` (`core/engine/src/error/mod.rs:307`) inside
  `EngineError` (`core/engine/src/error/mod.rs:363`) — uncatchable from JS, must be a
  distinct diff outcome, not "Boa threw".
- Instruction budget (hang-proofing): `ContextBuilder::instructions_remaining`
  (`core/engine/src/context/mod.rs:1188`) + enforcement (`core/engine/src/vm/mod.rs:781`)
  raising `NoInstructionsRemain` — but `fuzz`-feature-gated
  (`core/engine/src/context/mod.rs:103`). Either enable `fuzz` for the diff crate or run
  each case in a worker process with a wall-clock kill. Note the tester has an
  explicit `TODO: timeout` gap (`tests/tester/src/exec/mod.rs:283`) — P1 fills it, P3 reuses
  the P1.2 mechanism and adds no second one.
- Panic isolation: wrap every Boa-side run in `catch_unwind` like the tester
  (`tests/tester/src/exec/mod.rs:275`) → `Panic` outcome (`tests/tester/src/main.rs:795`).

**5. Normalizer (P3.2) — explicit table, reuse `is_error_type` shape**
- Compare (value-display | error-kind | console-lines | state-dump); normalize via a reviewed
  table: error *message* text ignored (kind compared, cf. `tests/tester/src/exec/mod.rs:598`),
  stack traces dropped, `console.time`/identity/`Date.now` patterns redacted, `undefined`
  completion vs empty stdout unified. Every rule needs a fixture proving it never masks a
  real mismatch (P3 gate: 100-fixture drill).

**6. Corpora, verdicts, triage queue (P3.3, P3.4)**
- v1 corpus = pinned Test262 programs + `tests/regression/*` reproducers (P1.1) + curated
  adversarial set over P0 choke points; P4 generators feed back later.
- Outcome taxonomy: define a differential-crate-local outcome type (e.g.
  `DiffOutcome::{Agree, Mismatch{boa, oracle, normalized_diff}, …}`) following the P1.2
  closed-taxonomy pattern — do NOT extend the tester's `TestOutcomeResult`
  (`tests/tester/src/main.rs:795`), which stays closed to tester outcomes. Persist runs in
  the `SuiteResult`/`TestResult` JSON shape (`tests/tester/src/main.rs:762`,
  `tests/tester/src/main.rs:783`) via `write_json` (`tests/tester/src/results.rs:84`);
  queue diffing reuses `compare_results` (`tests/tester/src/results.rs:172`). Verdict enum:
  `BoaBug | OracleQuirk | SpecAmbiguity`, each carrying spec section + minimized program
  (P4 `tmin`) before closure. Triage-queue JSON follows the shared P1.1 store conventions.

**7. Exact touch points**
- NEW `tools/differential/` crate: `boa_side.rs` (Context setup/run), `oracle.rs` (subprocess
  runner), `capture.rs` (Logger impl + state dump), `normalize.rs` (table), `queue.rs`
  (verdict store on `TestResult` JSON), `main.rs` (`run`/`triage` subcommands mirroring
  tester CLI `Run`/`Compare`, `tests/tester/src/main.rs:116`, `tests/tester/src/main.rs:166`).
- EXTEND (minimal): expose tester's `create_context`/`is_error_type` for reuse or copy the
  ~70 lines deliberately; reuse the P1.2 per-case timeout mechanism (P1 owns it).

**8. Gotchas**
- `Display for JsError` includes backtrace (`core/engine/src/error/mod.rs:856`) — raw string
  compare always diverges; compare kind+message only.
- `RuntimeLimit`/`NoInstructionsRemain` are engine errors, not JS throws — map to
  timeout/divergence outcomes, never to "both threw".
- `SimpleJobExecutor` bails on first error and uses virtual-clock jobs
  (`core/engine/src/job.rs:863`); still call `run_jobs` after *every* eval or promises leak
  across cases — use a fresh `Context` per case (tester does: `tests/tester/src/exec/mod.rs:534`).
- CLI `Executor` never idles-out headless (`cli/src/executor.rs:262`) — do not reuse for diff.
- `instructions_remaining` requires the `fuzz` feature; without it there is no in-engine
  hang stop — wall-clock kill is mandatory.
- Determinism: pin `FixedClock` (`core/engine/src/context/time.rs:212`), disable `can_block`
  for CPU-only corpora, force GC between cases; record oracle binary hash + flags.

**Validation / gate.**

- Pipeline drill: 100 hand-built mismatch fixtures (50 real historical Boa
  bugs reverted, 50 synthetic oracle-side quirks) → runner flags all 100,
  normalizer produces zero false benign-masking on a held-out set, minimizer
  shrinks each to under the adopted line budget.
- v1 corpus run completes in CI with zero untriaged mismatches; triage queue
  depth is a published metric with a decreasing trend.
- Oracle upgrade drill: bump the pinned oracle, re-run, and confirm the only
  new mismatches are re-triaged oracle-quirks or newly-exposed Boa bugs.
- **Exit criteria:** v1 oracle pinned + green at zero untriaged; triage
  throughput exceeds mismatch discovery rate for the trailing window;
  second-oracle recipe demonstrated at least once (even if kept nightly-only).

#### P3 as-built notes (appended 2026-10-02; append-only, original text above unchanged)

1. **Oracle: jsshell ESR `JavaScript-C128.14.0`; Node reserved for the
   second-oracle recipe, Deno dropped.** Measured startup ~10ms, cleanest
   normalizable output, zero-build pin via Mozilla SHA256SUMS
   (`scripts/fetch-oracle.sh` verifies zip SHA `dfe276d0...` and binary SHA
   `9cdfbda6...`; idempotent, re-verified in-session). Pin enforced in
   `docs/baseline.md` + `verify-baseline.sh:check_oracle_pin`; the runner
   hard-refuses version-skewed binaries and foreign verdict files (e2e).
2. **Supervisor lives in the differential crate.** The tester is a binary
   crate (`pub(crate)` internals), so reuse meant copy; the P1.2 mechanics
   are identical (per-case subprocess, wall-clock kill, bounded captures).
   Two supervisor bugs found by scale are fixed here, not inherited: (a)
   the drain stopped at the 64KiB cap and SIGPIPEd chatty children — it
   now drains to EOF while capturing the head (unit test with 200KiB);
   (b) staged probe files were never deleted (915MB orphaned from one
   killed 5.9k-case run) — both staging sites now remove their unique
   file on every path (verified zero remnants, identical results).
3. **Taxonomy: crate-local `DiffOutcome`, shared `TestOutcomeResult`
   untouched.** Per plan: comparison outcomes live in `boa_differential`;
   the tester's taxonomy stays closed. Queue diffing reuses the P1.1
   store conventions. One robustness rule added: a Boa child that exits
   0 but prints no parseable outcome line is a case-level HarnessError,
   never a run abort (a 128KiB-print probe used to kill the whole run).
4. **Minimizer is line+block (`line-reduce-v2`), not `tmin`.** P4 owns
   `tmin`; the queue schema already versions the producer per entry. v1
   (pure single-line deletion) stalled on brace-paired lines — 4 CI
   bodies at 28-32 lines over the 25-line body budget — because removing
   either half of a pair breaks the parse. v2 alternates line deletion
   with predicate-gated balanced-delimiter range deletion (JS-aware
   scanner: strings/comments/templates skipped, `${}` re-enters code;
   regex literals best-effort, gated by the predicate) to a joint
   fixpoint. The 4 now minimize to 3/3/24/4 lines; every run since
   (including 2718 fresh Array entries) is fully within budget,
   including a 3-line crash repro (`new Array(4294967295)`). The budget
   is report-level (`report_over_budget`); the gate stays
   zero-untriaged — a future irreducible-above-25 repro must force
   minimizer work, not silently pass, and there is deliberately no
   waiver hatch yet.
5. **Drill deviation: no literal 100-fixture artifact (50 reverted +
   50 synthetic).** The 50-reverted half needs 50 buggy engine variants;
   its intent is covered more strongly: (a) regression-DB repros run in
   every differential corpus, so a reverted fix flips them to mismatches
   — a continuous reverted-bug tripwire; (b) 374 live real divergences
   flagged on current engines (flagging proven on reality, not
   fixtures); (c) the ~30-file adversarial corpus + e2e must-flag case
   cover the synthetic-quirk half; (d) no-false-masking is pinned by
   R-rule unit fixtures, the e2e strict-comparison case, and 374
   unmasked live flags; (e) budget proven live on 3000+ entries. The e2e
   suite proves the loop instead: flag → breach(1) → verdict → clean(0)
   → export → preload.
6. **Verdicts are spec-anchored; majority vote is never a verdict.**
   Signatures are id+outcomes+fields; `oracle-quirk` requires a note,
   `boa-bug` a spec URL, `spec-ambiguity` spec + expiry. In-session
   adjudications that prove the point: (a) jsshell evaluates
   keyed-destructuring targets before the computed key **only under
   indirect eval** (raw script passes; Boa and V8 pass both ways);
   (b) sloppy-eval instantiation order differs on ALL THREE engines and
   Boa is the spec-exact one (functions-before-vars per
   EvalDeclarationInstantiation; micro-probe node `[fnA,midVar,fnB]`,
   jsshell `[midVar,fnA,fnB]`, Boa `[fnA,fnB,midVar]`);
   (c) jsshell evaluates `if (false) {}` (no else) to an EMPTY completion
   (inheriting the prior statement-list value into the eval result)
   while the spec returns undefined (sec-if-statement;
   `eval("1; if (false) {}")` → jsshell `1`, V8/Boa `undefined`).
   Committed verdicts live in `corpora/verdicts.toml` (3073: 374 CI +
   2699 Array), preloaded by signature; the CI run preloads its 374 and
   gates clean, and Array-slice preload is proven live (11/11).
7. **Scale: v1 (CI) corpus green; Array 6.4x probe fully triaged;
   full weekly run needs sharding (calibrated).** Final Array run
   (5869 cases + 126 excluded, post-fix engine, final driver):
   3170 agree / 2683 mismatch / 16 one-sided; all 2699 queued entries
   triaged in-session (2682 oracle-quirk + 16 boa-bug + 1
   spec-ambiguity) from ~10 anchored root causes — triage throughput
   exceeds discovery by two orders of magnitude, and every verdict
   committed (3073 total) with live preload proof. Minimizer budget
   100% across all runs. Extrapolation warning (measured, not guessed):
   5.9k cases take ~50 min on 5 cores (minimization dominates), so the
   ~60k-case weekly corpus projects to 4-8h — likely breaching the
   job's 300-min timeout AND the 6h hosted-runner cap. The
   `differential-full` job keeps its plan-literal gate, but P3.5 should
   shard it by suite (matrix over per-slice corpora, config-only) or
   add a survey mode that defers minimization; until then its first red
   is infra backlog (loud, not silent), not breakage. Boundary
   documented, not fixed: >64KiB of program console output truncates
   the Boa child's JSON line / pushes oracle markers past the
   supervisor cap, queueing a one-sided entry (never silent, never
   aborting); symmetric console caps are P3.5 work.
8. **Second-oracle recipe demonstrated (Node v26.10.0).** Destructuring
   probe under `--oracle-kind node`: 13 agree, 2 mismatch, 0 one-sided
   (vs 2/13/0 under jsshell — the eval-order quirk family vanishes),
   both triaged `oracle-quirk` with spec citation in scratch (V8's
   source-order eval instantiation, Boa exact), gate clean. Node
   verdicts stay out of the jsshell-pinned committed file.
9. **Oracle-upgrade drill demonstrated.** Reset on a scratch queue copy:
   13 pre-triaged → 13 untriaged, `--check` exits 1; fresh run with
   `--verdicts` re-triage → clean, exit 0. The stale-verdict hint now
   only fires for verdicts whose cases this run EXECUTED (a disjoint
   sub-corpus run used to advise pruning verdicts it never ran —
   destructive advice, fixed).
10. **CI wiring validated without a runner.** `cargo test --release`
    rebuilds `./target/release/boa_differential` (proved by
    delete-and-rebuild); workflow YAML parses; run flags mirror the
    green local CI run; `verify-baseline.sh` is green except the known
    pre-commit Cargo.lock FAIL (new crates' deps; clears on commit).
11. **P3's catches: two engine fixes landed, one crash class filed.**
    (a) Global `var` key-order hash scramble → source-order fix
    (`bytecompiler/declarations.rs`), full loop, Test262 steady.
    (b) Engine exposed a `TypedArray` global the spec does not list (SS
    18; V8/jsshell agree: `undefined`) → removed the
    `global_binding::<BuiltinTypedArray>` line; test262's
    testTypedArray.js harness (2080 includers asserting the global) is
    served by runner-side injection (`var TypedArray =
    Object.getPrototypeOf(Uint8Array)`) in the tester AND the
    differential corpus, like other runners. Full loop: repro-first
    proof (stash), engine unit test, DB entry `typedarray-no-global`,
    fuzz seed, green audit, full Test262 steady at 51440 passed
    (96.01%, compare exit 0, zero-new).
    (c) `String(new Array(4294967295))` SIGABRTs (64GiB
    `Vec::with_capacity(2*len-1)` in `Array.prototype.join`) → filed
    as OPEN `huge-sparse-join-abort` with minimized repro + analysis; a
    length-accounting guard was prototyped and REVERTED (the 4GiB result
    fits Boa's 2^32-1 max, so the killer is O(length) iteration, not
    the cap — the real fix is sparse-awareness, engine work).
    Differential verdicts: 16 crash entries → `boa-bug` + regression
    link; they reopen automatically when the fix lands (signature
    flip). Follow-ups recorded in the entry: same O(length)
    materialization in `toLocaleString`/`sort_indexed_properties`
    (sort spins, then OOMs — measured).
12. **Driver is now poison-safe with capped rendering.** The Array probe
    found 107+6 unobservable cases in two mechanisms: (a) tests defining
    indexed properties on `Array.prototype` (e.g. non-writable `"0"`)
    broke the epilogue's own push/sort on BOTH sides via prototype-chain
    `Set` lookup — fixed with null-prototype working containers
    (converted back to a real array for the marker via immune
    CreateDataProperty slice), proven by an e2e case plus 107 (F,F) →
    observed at scale; (b) megabyte state renderings SIGPIPEd both
    children past the 64KiB capture — fixed with a 512-char
    `__diff_trunc` (+ length suffix). Independently, every intrinsic the
    epilogue needs is captured into `var`s before the program runs
    (invoked only via captured `Reflect.apply` — no method lookups, no
    `.call`); the old driver provably emits no markers under method
    replacement (exit 3, reproduced). Committed adversarial guards
    `driver-poison.js` + `driver-bigstate.js` (both agree ×2 variants)
    keep every CI run proving it. Masking window documented: values
    identical in the first 512 chars with equal length but differing
    later compare equal (CI counts identical pre/post: 374/374).
    Known hole, deferred: a program defining `Object.prototype.toJSON`
    would still break both markers symmetrically (queued one-sided,
    never silent) — zero observed cases.
13. **Exclusion accounting, not silent skips.** CI run: 3 excluded
    (`engine-specific` x2, `matrix-tracked` x1); Array probe: 126
    (`async` x90, `host-262` x16, `time-random` x20), each counted with
    a machine-readable reason; `engine-specific` repros stay green in
    the DB but run nowhere differential.
14. **Host-global collisions verdict, not excluded.** 48 Array entries
    carry a Boa-only `isConstructor` key (jsshell pre-defines a native
    shell global of that name, so the harness binding filters from
    oracle state) and 11 an oracle-only `TypedArray` key (pre-fix Boa
    global; gone post-fix) — both sides bind the same function and
    pass; the diff is pure observation-filter asymmetry. Verdicts carry
    precise notes (`oracle-quirk` for the shell builtin; the TypedArray
    entries collapse post-fix); the signatures reopen them if either
    shell changes its globals. A corpus-side collision filter was
    considered and rejected: value-change detection fails on name-only
    function rendering, and static declaration scanning is fragile —
    precise verdicts are the maintainable instrument.
15. **Runner fix: `[[CanBlock]]` is now true on the Boa side.**
    Shard-1 triage surfaced 14 reversed entries (Boa throws
    `TypeError: agent cannot be suspended`, oracle completes), all
    `Atomics.wait` with `CanBlockIsTrue` flags — but direct probes
    showed Boa returns `timed-out` correctly. Root cause: the
    differential context hardcoded `.can_block(false)`
    (`boa_side.rs`), while the tester sets true unless
    `CanBlockIsFalse` (`exec/mod.rs`) and the pinned oracle's main
    thread blocks (verified: finite waits complete; infinite waits
    hang). Fix mirrors the tester: `.can_block(true)` plus a corpus
    exclusion for the now-unprovidable `CanBlockIsFalse` flag (reason
    `canblock-false`; 2 files). Tests: corpus unit test
    `canblock_flags_route` + e2e `canblock_true_agrees_and_false_excludes`
    (47 unit + 4 e2e green). Proof rerun of the `Atomics/wait` slice:
    all 14 agree, 2 excluded `canblock-false`, remaining 10 mismatches
    are the known eval-order state shapes. The 14+4 stale shard-1
    entries carry no verdict by design (engine was correct; runner
    fixed; rerun-proven) and vanish on the next run.
16. **Oracle quirk: stale indirect-eval completion (Rule 7, 1088
    entries).** Shard-1's `undefined → function:*` value cluster is a
    SpiderMonkey completion-slot leak, minimized to `var o = {}; o.b
    = function () {};` + `if (false) { throw 1; }` (jsshell: the
    function; Boa/node: undefined). Full matrix: value-producing
    statements and try/catch write the slot, but
    if-false-without-else, `var`, bare blocks, `;`, and empty loops
    never update it, so eval leaks the last value instead of the
    spec's undefined (PerformEval maps empty to undefined;
    IfStatement-without-else yields undefined). Boa follows the spec;
    three-way probes + per-case node adjudication over exact
    corpus-reconstructed programs agree with Boa on all 1088 (zero
    Boa-bug candidates, zero timeouts). Bulk verdicts split pure
    value (750) vs value + known sloppy-eval order companion (338);
    full-state combos stay hand-triaged. Untriaged shard-1: 3203 →
    2115. Shards language/intl402 have since completed.
17. **Full-corpus run finished (85,904 cases); shard-1 triage 2115 → 85
    via rules 8–27.** All five shards completed with zero failures
    (builtins 43159, language 34099, intl402 5232, staging 2220, annexB
    1194). Shard-1 bulk verdicts, every count asserted exact: cross-area
    ctor-format+Float16 620, pure isConstructor collision 295, RegExp
    inline-modifiers 70+70, new-Unicode-scripts 22+22, RegExp.escape 38,
    UCD/emoji/folding tail 278 (every case node-adjudicated: 250 probes
    + 10 negated + 8 full RGI files + folding, all V8-with-Boa),
    Symbol-primitive guards 46 (jsshell spec bug), missing-API direct
    203 + second-order 226 (upsert, float16, isError, base64/hex,
    rawJSON, try, pause, f16round, keyed, sumPrecise, waitAsync 68,
    toTemporalInstant 10), Promise resolve-precedence 16, TypedArray
    [[Set]] 12, JSON reviver 10, iterator toStringTag 8,
    defineProperty order 4, freeze-RAB 2, ThrowTypeError identity 8,
    native-syntax shape 2. Throw-combo state companions were all
    classified (102 groups: post-throw unassignment, short-circuited
    counters, pre-write targets, ctor-format, shell-global filters)
    behind a frozen reviewed-pair guard. Notable: Boa is ahead of V8
    too on sumPrecise, allKeyed/allSettledKeyed, unique-per-realm
    %ThrowTypeError%, and trailing-garbage leniency (V8 strict where
    the test expects leniency — verdict rests on jsshell's missing API
    regardless). Mismatch triage found ZERO engine bugs — Boa correct
    in every adjudicated case; the run's only engine bugs remain the 2
    one-sided crash roots (join-abort, cyclic-proto), filed + regressed.
    Results durably snapshotted in `differential-runs/` (raw 2.2 GB +
    verified 5 MB zstd tarball + SHA256SUMS + triage workflow backup;
    raw gitignored, tarball committable) after a reboot proved /tmp
    mortal. Remaining for P3.5-complete: shard-1 tail 67 (both-ok
    state/value + 5 empty-field) + shards 2–5 (~17.8k, bulk rules adapt
    per shard) + verdicts export + gates.
18. **P3.5 COMPLETE (2026-10-03): driver hardening, tail verdicts,
    node adjudication, 42,726 exported verdicts, P4 unblocked.**
    (a) Driver hardening: IIFE-wrapped harness + toJSON-immune
    pre-serialized `__diff_q` quoter (`compose.rs`; lone surrogates →
    U+FFFD) after toJSON-poison + frozen-global silenced both sides;
    50 unit + 5 e2e green; byte-identity proof on loopbefore/loopafter
    reruns. (b) Time-scan widened (`corpus.rs` + unit test
    `time_scans_catch_parenless_and_smuggled_date`) after parenless
    `new Date;` and `Array.from.call(Date, …)` evasions found.
    (c) Fixed-driver reruns: ucd16 (406 agree/129 mismatch), forof
    (846/592), smregexp (98/74), biff (52/50), dfix (2/1).
    (d) Mega-bulk `bulk-language.py` applied: 3536 verdicts + 3 stale
    removals + 1 manual combo verdict; all 5 shard gates clean (zero
    untriaged): language 12554, builtins 24494, staging 923, annexB
    479, intl402 4276. (e) Node/V8 adjudication rerun, 27146 cases
    (20032 agree/6985 mismatch/129 one-sided): verdicts stand; family
    audit found expected agreements plus second-oracle surprises
    folded into 73 notes via `audit-updates.py` (T4 double-eval,
    with-proxy V8 fails, PUTVALUE V8 fails, TA_F16 SharedTA shape,
    SuppressedError membership, STALE50 V8-stale,
    EVAL17/INTL28 V8 fails). (f) Export + merge:
    `tools/differential/corpora/verdicts.toml` now holds 42,726
    verdicts. (g) Engine bugs 15 → 19 (try-catch-completion,
    class-tostring-native, regexp-double-escape,
    atomics-wait-finite-timeout filed with fail-verified repros).
    (h) fmt/clippy/test green (differential zero warnings; the two
    `boa_engine` iterator_helper clippy notes are pre-existing —
    working tree untouched there). `verify-baseline.sh`: all pass
    except Cargo.lock-dirty, which is the legitimate new-crate
    (boa_differential/boa_outcomes/boa_regression) lock addition,
    uncommitted per no-commit-without-asking — expected, not a
    regression. P3 exit criteria met: proven pipeline, spec-anchored
    triage, empty untriaged queue. Deviation from plan text: none in
    method — only scale (a second-oracle audit and driver hardening
    the plan did not foresee, both recorded here).

### P4 — Fuzz fleet: run what exists, graduate oracles, stress the GC, instrument everything

**Goal.** Turn the three build-only fuzz targets into a scheduled fleet with
time/coverage budgets, semantic oracles, and a crash→minimize→dedupe→regress
pipeline; add GC-stress and sanitizer configurations. Fuzzing runs forever
after this phase; the phase is done when the fleet is self-sustaining with an
empty untriaged queue.

**Work items.**

4.1. **Run the existing three targets on schedules.** Promote
`parser-idempotency`, `bytecompiler-implied`, `vm-implied`
(`tests/fuzz/fuzz_targets/*`) from build-only (`rust.yml:351-385`) to
scheduled runs: short-budget runs per PR (smoke: minutes, fixed seeds +
regression seeds must pass), long-budget runs nightly/weekly (hours, full
corpus evolution). Standard cargo-fuzz workflow applies throughout
(`cargo fuzz run|cmin|tmin|coverage`; see
`https://raw.githubusercontent.com/rust-fuzz/cargo-fuzz/main/README.md` and
`https://llvm.org/docs/LibFuzzer.html`).
4.2. **Graduate `vm-implied` to semantic oracles.** Keep crash detection; add:
(a) differential mode against the P3 oracle on generated programs,
(b) metamorphic-equivalence mode (see P5 transforms) comparing Boa-vs-Boa,
(c) idempotency round-trips (parse→print→parse→execute equality) reusing the
parser target's machinery. Crash-only can no longer be the sole signal for VM
correctness.
4.3. **Adversarial-input surfaces (host boundary).** Add/extend targets for
every untrusted-input surface from the P0 host map: JS source bytes (parser
robustness: never panic/hang/corrupt on arbitrary bytes), module specifiers,
URL parsing (`boa_runtime`/`boa_wintertc` url modules), fetch response
handling, CLI argument/host-object injection paths. Properties: no panic, no
hang (instruction/time budgets as in the existing VM target), no UB under
sanitizers.
4.4. **GC-stress configuration.** Add a stress mode (feature flag and/or
runtime knob): minimal GC threshold / collect-on-every-N-allocations, forced
collections at hostile points (inside calls, property access, exception
unwind), weak-ref/finalization-heavy generated programs (millions of
allocations, deep graphs, cycles, ephemerons). Run the Test262 + regression
corpus under stress on a schedule; any divergence vs normal mode is a bug.
4.5. **Sanitizer matrix.** Add CI/fuzz configs: ASan+UBSan-instrumented fuzz
runs (`cargo fuzz run -s address,undefined …`) and sanitizer-instrumented
test runs for the unsafe-adjacent crates. Guidance: the existing `-s` option
(`tests/fuzz/README.md:15`) and the Clang sanitizer docs
(`https://clang.llvm.org/docs/AddressSanitizer.html`,
`https://clang.llvm.org/docs/UndefinedBehaviorSanitizer.html`).
4.6. **Corpus pipeline.** Every crash: `tmin`-minimized reproducer,
signature-deduplicated, entered into the regression DB with finder="fuzz";
every fix's seed merged back via `cmin`; corpus coverage (`cargo fuzz
coverage`) tracked per target and non-decreasing. Seeds include all of P2's
fix reproducers and P3's mismatch minimizations.
4.7. **Fuzzilli evaluation (semantic generation at scale).** Evaluate
Fuzzilli (FuzzIL-based generation with syntactic-correctness-by-construction
and REPRL execution —
`https://raw.githubusercontent.com/googleprojectzero/fuzzilli/main/Docs/HowFuzzilliWorks.md`)
as the semantic program generator feeding P3/P4 oracles: build the Boa
REPRL-style adapter, run a bounded trial, and adopt if its yield-per-triage-hour
beats the in-house `Arbitrary`-based generators at comparable coverage;
otherwise keep in-house generation and re-evaluate on a schedule. The decision
and its evidence are recorded either way.

#### P4 implementation notes (grounded in current code)

Current fleet (all paths repo-relative to `/home/vetruvian/Desktop/boa`).
Three `[[bin]]` targets in `tests/fuzz/Cargo.toml:26-42`: `parser-idempotency`,
`vm-implied`, `bytecompiler-implied`. Deps: `libfuzzer-sys 0.4.13`
(`tests/fuzz/Cargo.toml:11`), `arbitrary 1.4.2` (`tests/fuzz/Cargo.toml:13`),
path deps `boa_ast`+`arbitrary`, `boa_engine`+`fuzz`, `boa_interner`+`arbitrary`,
`boa_parser` (`tests/fuzz/Cargo.toml:14-17`). Own workspace
(`tests/fuzz/Cargo.toml:20-21`) + root exclude (`Cargo.toml:21-25`, "weird
things on Windows" `Cargo.toml:22`): all fuzz commands run from `tests/fuzz/`.
CI only builds: `cargo fuzz build -s none --dev` (`rust.yml:351-385`, cmd
`.github/workflows/rust.yml:385`) on stable (`.github/workflows/rust.yml:371-374`).

Generation strategy (`tests/fuzz/fuzz_targets/common.rs`, all read).
`FuzzData { interner, ast: StatementList }` (`common.rs:14-17`); `Arbitrary`
impl seeds interner with `a..=h` (`common.rs:21-25`), derives AST via
`StatementList::arbitrary` (`common.rs:27`; per-node derives e.g.
`core/interner/src/sym.rs:14`), then `FuzzReplacer: VisitorMut` remaps every
`Sym` into the 8-sym universe via `node.get() % len` (`common.rs:45-48`).
`FuzzSource` = `to_interned_string` of that AST (`common.rs:77-86`); its
`Debug` prints the source (`common.rs:88-92`) — reuse for crash artifacts.

Per-target behavior, budgets, asserts (all three targets read line by line).
- `parser-idempotency.rs:15-64`: AST→source→parse→source→parse; discards
  first-parse errors (`parser-idempotency.rs:23`), `expect`s second parse
  (`parser-idempotency.rs:40-42`), asserts interner length unchanged across
  both passes (`parser-idempotency.rs:27-35`, `parser-idempotency.rs:45-53`)
  and AST equality (`parser-idempotency.rs:54-61`). No execution budget
  (parse-only). Gotcha: `do_fuzz` always returns `Ok`, so `Corpus::Reject`
  (`parser-idempotency.rs:68-74`) is dead — libFuzzer keeps everything.
- `bytecompiler-implied.rs:11-27`: `Context::builder().interner(..)
  .instructions_remaining(0)` (`bytecompiler-implied.rs:12-16`), then
  `Script::parse` (`bytecompiler-implied.rs:17-21`) + `codeblock`
  (`bytecompiler-implied.rs:22`). Budget 0 = compile-only by construction:
  any eval would instantly throw `NoInstructionsRemain`
  (`core/engine/src/vm/mod.rs:784-787`). Note `Script::parse` also runs the
  optimizer (`core/engine/src/script.rs:100-102`), so this target already
  covers optimizer crashes.
- `vm-implied.rs:11-22`: budget `1 << 16` (`vm-implied.rs:12-16`), `ctx.eval`
  (`vm-implied.rs:17`), result discarded (`vm-implied.rs:20-22`). Crash-only
  by design (`tests/fuzz/README.md:47-51`); infinite loops become
  deterministic `Throw` via the fuel check in `execute_one`
  (`core/engine/src/vm/mod.rs:777-790`, decrement `core/engine/src/vm/mod.rs:789`).
Fuel plumbing (gated on `fuzz`): field (`core/engine/src/context/mod.rs:102-104`),
builder setter (`core/engine/src/context/mod.rs:1186-1189`), `build` wiring
(`core/engine/src/context/mod.rs:1254-1255`); feature =
`["boa_ast/arbitrary", "boa_interner/arbitrary"]` (`core/engine/Cargo.toml:62`).

Scheduling design (P4.1). Keep build job as-is; add: (a) per-PR smoke —
`cargo fuzz run --dev -s none <target> -- -max_total_time=300`, fixed
`-seed=` set plus every `tests/regression` seed as corpus input, fail on any
crash/timeout; (b) nightly 4–8h full-corpus evolution per target with new-seed
writeback; (c) weekly long runs + `cargo fuzz coverage`. Gotcha: `cargo fuzz
run` needs nightly (cargo-fuzz book) but CI pins stable
(`.github/workflows/rust.yml:371-374`) — P0 must add a nightly pin for fuzz
jobs. Corpus pipeline (P4.6): `cargo fuzz cmin/tmin/coverage <target>`;
`FuzzSource`'s `Debug` (`common.rs:88-92`) gives the minimizer-ready source.

Semantic-oracle extension points per target (P4.2; keep crash detection).
- parser: already an idempotency oracle; extend `do_fuzz`
  (`parser-idempotency.rs:15`) with error-determinism (same bytes → same
  error kind/position twice) and a third parse→print fixpoint check.
- bytecompiler: extend `do_fuzz` (`bytecompiler-implied.rs:11`): compile
  twice (fresh `Context`), compare success/failure + disassembly equality;
  once P5 lands, assert `verify(&CodeBlock)` on every produced block (staged:
  land the hook call-site in P4, activate when P5 delivers `verify`); add a
  normal-vs-GC-stress compile-equality mode (same `codeblock` or same error).
- vm: extend `do_fuzz` (`vm-implied.rs:11`): (a) differential — run
  `FuzzSource.source` under Boa and the pinned P3 oracle, compare
  completion value/error class after normalization; (b) metamorphic mode —
  staged: land the harness hook in P4, plug in P5 transforms once P5 delivers
  them; (c) Boa-vs-Boa — normal vs GC-stress vs sanitizer builds must agree
  (cheapest, no oracle needed); (d) idempotency — `eval(src)` vs
  `eval(print(parse(src)))` equality, reusing `parser-idempotency.rs:16-25`
  machinery. All modes reuse `Script::parse`/`codeblock`/`evaluate`
  (`core/engine/src/script.rs:87-91`, `core/engine/src/script.rs:121`,
  `core/engine/src/script.rs:176-183`) and the `1<<16` fuel budget
  (`vm-implied.rs:14`).

GC-stress knob design, exact location (P4.4). `GcConfig { threshold,
used_space_percentage }` is private (`core/gc/src/lib.rs:53-59`), default
1MB/70% (`core/gc/src/lib.rs:64-72`) with `TODO: Add a configure later`
(`core/gc/src/lib.rs:63`). Trigger + adaptive growth live in `manage_state`
(`core/gc/src/lib.rs:188-201`); `Collector::collect` runs when
`bytes_allocated > threshold` (`core/gc/src/lib.rs:189-190`); existing pub
hook `force_collect` (`core/gc/src/lib.rs:533-542`). Design: (1) immediate,
no API change — call `force_collect` at hostile points inside fuzzers
(between parse/compile/eval, N times per input) and in a Test262-under-stress
runner, diffing normal vs stress outcomes; (2) permanent knob — add
`pub fn gc_set_stress(...)` (or env-gated cfg) next to `force_collect`
(`core/gc/src/lib.rs:533`), storing a flag in `BoaGc`
(`core/gc/src/lib.rs:81-87`) that `manage_state` (`core/gc/src/lib.rs:188`)
honors as collect-every-N-allocs and that disables adaptive growth
(`core/gc/src/lib.rs:194-199`). Gotchas: `BOA_GC` is thread-local
(`core/gc/src/lib.rs:44-51`), so the knob is per-thread/global, NOT
per-`Context` — do not plumb it through `ContextBuilder` (cf. fuel setter
`core/engine/src/context/mod.rs:1188`); the existing test isolation pattern
is thread-per-test (`core/gc/src/test/mod.rs:54-58`) — reuse for
stress/normal differential runs; adaptive growth fights a fixed tiny
threshold, so stress mode must bypass lines `core/gc/src/lib.rs:194-199`.

Adversarial surfaces (P4.3). `boa_wintertc` registers
abort/base64/clone/console/encoding/events/fetch/microtask/store/timers/url
(`core/wintertc/src/lib.rs:39-51`) via `register`
(`core/wintertc/src/lib.rs:62-82`); `url`/`fetch` are feature-gated
(`core/wintertc/src/lib.rs:45-46`, `core/wintertc/src/lib.rs:50-51`).
New targets: `url-parse` (arbitrary bytes → URL parser, no-panic/no-hang),
`fetch-response` (arbitrary bytes as Response body), module-specifier fuzz
via `Module::parse`-equivalent path, CLI-args/host-injection paths; all reuse
`FuzzSource`-style byte→string generation plus the fuel budget
(`core/engine/src/vm/mod.rs:784-789`). Pre-coding probe required: the exact
`url`/`fetch` pub entry functions were not inspected — confirm them before
coding the targets.

Sanitizer matrix (P4.5; P4 owns fuzz-instrumented runs, P6 owns sanitizer
`cargo test` runs). Baseline `-s none` is the documented default
(`tests/fuzz/README.md:11-15`, CI `rust.yml:385`). Matrix: `-s none`
(coverage/smoke, fastest), `-s address` (memory errors), `-s
address,undefined` (full matrix per plan), each as
`cargo fuzz run -s <set> <target> -- -max_total_time=<budget>`.
UBSan on Rust finds mostly FFI/miscompile-class issues; ASan is the load-
bearing column for the `unsafe`-adjacent GC/value/string code.

Fuzzilli trial (P4.7). Build a REPRL-style adapter: Fuzzilli-generated JS →
`Script::parse`+`evaluate` (`core/engine/src/script.rs:87-91`,
`core/engine/src/script.rs:176-183`) under the `1<<16` fuel cap, with the
P4.2 value/error comparator as the oracle; adopt iff yield-per-triage-hour
beats `Arbitrary` generation (`common.rs:19-61`) at comparable coverage.

Key gotchas recap: `Corpus::Reject` dead in parser target
(`parser-idempotency.rs:68-74`); bytecompiler budget 0 forbids eval
(`bytecompiler-implied.rs:14` + `core/engine/src/vm/mod.rs:784-787`);
only `Sym`s are remapped — arbitrary string literals are an open TODO
(`common.rs:36`); GC knob must be thread-local-global, never builder state
(`core/gc/src/lib.rs:44-51`); fuzz runs need nightly, CI has stable
(`.github/workflows/rust.yml:371-374`).

**Validation / gate.**

- Seeded-crash drill: reintroduce five historical crash bugs (distinct
  signatures) → fleet rediscovers all five within the smoke budget; minimizes
  each; dedupes correctly.
- Oracle-graduate drill: revert three historical *logic* bugs → semantic modes
  (not crash detection) flag all three.
- A full scheduled cycle completes with zero untriaged crashes and
  non-decreasing per-target coverage.
- **Exit criteria:** fleet scheduled + green; crash pipeline (minimize→dedupe
  →regress→merge-back) demonstrated end-to-end; GC-stress + sanitizer configs
  running; Fuzzilli adopt/reject decision recorded with evidence.

#### P4 as-built notes (appended 2026-10-03; append-only, original text above unchanged)

P4.4 is implemented and proven below. P4.5–P4.7 and the P4 gates are still
pending; their notes will extend this section.

1. Knob shape (as planned): `gc_set_stress(Option<NonZeroU64>)` next to
   `force_collect` (`core/gc/src/lib.rs`), thread-local, bypassing the byte
   threshold and adaptive growth while set. Plus a panic-safe RAII
   `StressGuard` (nested uses restore correctly) and introspection
   (`gc_stress_every`, `gc_collections`, `gc_total_allocs`). The plan's
   gotcha is honored: nothing is plumbed through `ContextBuilder`; every
   consumer sets the knob on its own thread (per-test guards cover rayon
   workers).
2. `stress_point` is an unconditional `force_collect`, not stress-mode-only
   (deviation). Rationale: a hostile-point collection before every fuzz
   eval is nearly free and shakes out missing roots on every input, while
   the every-N cadence covers in-eval points. The determinism double-eval
   thus also runs one collection per leg in normal mode.
3. Stressed second legs in `vm-implied`, `bytecompiler-implied`, and
   `cli-exec`. Timing-observable programs (`WeakRef`,
   `FinalizationRegistry`, `WeakMap`, `WeakSet`) keep a plain second leg
   via a best-effort substring pre-filter (deviation: the plan assumed the
   a-h universe made the vm leg unconditionally sound, but un-remapped
   string literals can reach `globalThis["WeakRef"]`, so both eval targets
   filter; bytecompiler needs no filter — compilation runs no user code).
   A divergence on a *filtered* program is still triaged, never dismissed.
4. Churn gate `STRESS_MAX_ALLOCS = 100_000` + `should_stress` (addition, not
   in plan). A real libFuzzer smoke produced a 25-char fuel-saturated
   `while (import(this)) {}` (132k normal-leg allocs) running 38x slower
   stressed (12.3s vs 0.3s); uncapped, such mutants would starve the 300s
   PR smoke. Inputs over the cap skip only the stressed leg (determinism,
   crash, idempotency, oracle legs still run); the gate is deterministic
   (allocation counts, not wall time) and fail-safe unit-tested. Measured
   fix: 70s → 1.8s on the pathological artifact. The Test262 side needs no
   gate (bounded corpus; worst case is one slow test, see 6).
5. Tester `--gc-stress[=N]` (deviation: plan said "feature flag and/or
   runtime knob"; chose an optional-value CLI flag, default N=100, so
   schedules can tune without rebuilding). Each in-process test holds a
   `StressGuard`; under `--timeout` the cadence travels through the
   `run-single` child protocol (explicit value; `run-single` is internal).
   Zero is rejected with an explanatory error on both paths.
6. Calibration (release, CI-exact): full suite normal 51,440/0 in 113s vs
   stressed 51,440/0 in 203s (~1.8x), `compare --fail-on=regression` green.
   At the normal 60s budget the gate correctly breached once:
   `regress-1507322-deep-weakmap` (100k-deep ephemeron chain) needs ~58s
   stressed vs 0.44s normal with the *same passing outcome* — a budget
   breach, not an engine bug (proven via `run-single`: `{"outcome":"O"}`).
   The scheduled stress job therefore uses `--timeout 300` (5x headroom,
   documented in the workflow); N=1 engagement was proven separately (6x
   slowdown on a trivial test, so the knob provably bites end to end).
7. CI: `test262_full.yml` gains a full-corpus `--gc-stress` run plus a
   stress-vs-normal `compare --fail-on=regression` gate and a results
   artifact. Fuzz schedules are untouched — the stress legs are built into
   the targets, so smoke/nightly/weekly cover them with no workflow change.
8. Test collateral: 6 `boa_gc` unit tests (exact cadence, bypass,
   restore, guard incl. panic-unwind, alloc counter), 10 `boa_fuzz` lib
   tests (fixture + host-context + compile agreement, engagement proof,
   filter matrix, env override, gate matrix, fail-safe outlier pin,
   200-input generative soundness probe mirroring target leg selection),
   3 tester e2e (`compare`-gated equivalence on the mini corpus, both
   paths, zero rejection). Suites green: gc 36, fuzz lib 55, tester 39+8,
   engine lib 1128; fmt clean; clippy clean except 4 pre-existing warnings
   in files untouched by P4.4 (engine `iterator_helper`, fuzz `universe` /
   `canonicalize` — same as before this phase).

P4.5 is implemented and proven below (appended 2026-10-03). P4.6–P4.7 and
the P4 gates follow in later appends.

9. Sanitizer matrix (deviation: weekly ASan+MSan only, not the plan's
   broader sketch). `fuzz-sanitizer` job in
   `.github/workflows/fuzz-nightly.yml`: 4 cells (vm-implied+cli-exec
   under ASan, vm-implied+source-bytes under MSan), 1800s each, Sunday
   schedule + manual dispatch, artifacts uploaded on failure only.
   Target choice is unsafe-adjacency: the VM/eval and byte-handling
   surfaces where spatial/temporal/uninit bugs live. UBSan is
   deliberately excluded — cargo-fuzz 0.13.2 spells only
   address/leak/memory/thread/none, so there is no UBSan invocation to
   schedule; integer/bounds classes stay covered by dev-profile
   `overflow-checks` + `debug-assertions` on every schedule instead.
   Standalone `-s leak` is excluded as redundant: ASan runs its leak
   checker by default, proven clean on a 200-run loop, so any future
   leak report is signal, not GC noise. Rationale is documented
   in the workflow comment block and `tests/fuzz/README.md`.
10. Clean proofs before scheduling (a red first run must mean a new bug,
    not a dirty baseline): ASan 200-run loop exit 0, MSan 300-run loop
    exit 0, plus a 10-minute ASan hunt on vm-implied (1971 runs, 0
    crashes). The hunt's new-coverage finds accumulated in the working
    corpus (gitignored) and later surfaced as the P4.6 baseline-inflation
    lesson — scheduled runs upload evolved corpora as artifacts for the
    merge-back queue rather than baking them into any baseline.
11. Test collateral: none beyond the workflow + proofs — sanitizer
    coverage is a schedule, not an assertion, and the existing suites
    already run under dev-profile checks. `verify-baseline.sh` green
    (pre-commit Cargo.lock note only).

P4.6 is implemented and proven below (appended 2026-10-03). P4.7 and
the P4 gates follow in later appends.

12. Crash intake `pipeline/triage-crash.sh <target> <artifact>`
    (reproduce → dedupe → minimize → skeleton). Reproduces through the
    fleet binary first (fleet-visibility proof; stale artifacts exit 1);
    dedupes BEFORE the expensive leg (open-rule match = duplicate, exit
    0 with no skeleton; landed-rule match = possible regression, skeleton
    written anyway — fail safe); minimizes via `cargo fuzz tmin` under
    the same sanitizer; hangs skip tmin (each probe would spin to the
    timeout — manual bisection instead) and always get a skeleton (bare
    `exit=124` rules are forbidden: two hangs never auto-dedupe).
    `known-signatures.txt` holds 3 rules; the parser stack-overflow rule
    doubles as the class rule since all native overflows print one
    message. Demonstrated end to end on a live crasher.
13. Seed bank + merge-back on-ramps. `pipeline/export-diff-seeds.py`
    pulled 7 `diff-*.js` seeds from `differential-runs/*/triage-queue.json`
    (harness-prelude stripping, dedupe, `--dry-run`); `examples/render.rs`
    renders structured-target inputs back to JS for triage readability.
    `sync-seeds.sh` copies `seeds/*.js` to every corpus as `repro-*.input`
    (byte targets run them literally; structured targets take byte
    diversity). Bank: 29 seeds.
14. Coverage ratchet `pipeline/coverage-gate.py` + 8-target
    `coverage-baseline.toml` (regions, corrected bank-true values after
    the gate-time accumulation fix below: parser 1850, vm 8324,
    bytecompiler 8107, source-bytes 8121, module-specifier 5799,
    url-parse 14970, fetch-response 15225, cli-exec 24546; check-mode
    green 8/8, exit 0). Three hard-won corrections are baked in:
    (a) bank-true baselines — unmerged hunt finds inflated vm-implied,
    so baselines are measured over restored bank-only corpora (finds
    preserved aside for the merge-back queue, never baked in, never
    lost). A dead end is recorded honestly: the incident-era vm value
    8339 was first misread as depleted-corpus low and "ratcheted" to
    23555 — but 23555 was itself tainted (see the accumulation fix in
    the gate notes), and 8339 turned out to be ≈ the true bank value
    all along. (b) Breach retry — coverage wobbles
    run to run (observed −15/−2, each `= baseline` on re-measure), so a
    first below-baseline reading re-measures once; the gate fails only
    on a reproduced decrease. The retry proved itself on its first full
    run (two tentative breaches, both noise). (c) `cmin` mutates in
    place AND hash-renames: filenames never survive minimization (track
    content, not names), concurrent cargo-fuzz commands corrupt the
    shared `target/` + `corpus/` (one at a time locally; the gate
    runs serially and carries a restore-guard against the observed
    cmin-emptied-corpus failure), and check mode is therefore NOT
    read-only — documented in `pipeline/README.md` along with the rest.
15. Test collateral: the gate is self-proving (green 8/8 run + observed
    retry saves); triage was demonstrated on a live artifact. Durable
    unit tests stay with the suites that own the behavior (no new test
    files: the pipeline is scripts + committed state, verified by
    execution).

P4.7 is implemented and proven below (appended 2026-10-03), with the
adopt/reject decision the P4 exit criteria require. Only the P4 gates
remain; their notes will extend this section.

16. REPRL adapter `tools/fuzzilli` (`boa-reprl` binary): fresh `Context`
    per script, fuel-capped (65536), `fuzzilli` + `print` globals.
    Crash fidelity is the design center: `EngineError::Panic` surfaced
    as `JsError` and any Rust panic escaping `eval` both `abort()` —
    a Rust unwind would exit 101, which Fuzzilli reads as mere failure
    and the crash would be LOST. Verified by the `/tmp` self-test
    (HELO, complete→0x0, throw→0x100, print→fuzzout bytes, crash→SIGABRT
    green) and by Fuzzilli's own startup tests (PRINT + CRASH 0/1/2).
17. Coverage stub `cov.rs` (Rust port of upstream `coverage.c`, extended
    to multi-range: one guard array per codegen unit accumulates, reset
    numbers guards sequentially across all ranges). Build recipe in
    `build.sh` (validated): sancov-module pass + trace-pc-guard at
    level 3 (level 4 pulls `trace-pc-indir`, a second runtime symbol;
    the LEVEL flag is load-bearing — without it LLVM silently emits no
    guards), flags via `--config target.*.rustflags` with
    `-Ztarget-applies-to-host` + `target-applies-to-host=false` so host
    build-scripts stay uninstrumented, separate `target/sancov` dir,
    `debug-assertions` + `overflow-checks` on (Fuzzilli's "debug
    parameters"), and a BEHAVIORAL self-check (startup must print
    `[COV] registered guard range`, else the flags were silently
    dropped). 55,813 edges; Fuzzilli's `[Coverage] Initialized, 55814
    edges` (+1 is its own indexing) confirms the SHM handshake live.
18. Three protocol/infra traps, all verified against upstream sources
    (never assumed): (a) the exec magic is `exec`, not the `cexe` of
    older Fuzzilli/docs — every execution failed until the child was
    fixed to match `libreprl-posix.c` (the self-test passed throughout
    because it mirrored the same wrong magic: self-built oracle, caught
    only by the real parent). (b) `FUZZILLI_PRINT` must write `%s\n` to
    fd 103 (memfd whose shared offset is the length), not stdout —
    upstream QJS does `fdopen(103)`; until wired, Fuzzilli warned
    `Cannot receive FuzzIL output`. (c) `build.sh`'s original
    `| grep -q` self-check always false-alarmed under `set -o pipefail`:
    grep's early exit SIGPIPEs the binary, whose `println!` then panics
    with exit 101 — capture-then-grep now. Also recorded: Python's
    `Popen(pass_fds=+preexec dup2)` does NOT deliver the duped fds here
    (the close pass runs after preexec and closes them; the child then
    exits 1 on HELO write) — the self-test uses raw fork/exec.
19. Trial (Fuzzilli @ `281729b`, Swift 6.4, `boa` profile mirroring
    `qjs`, machine-local checkout under `~/.local`, documented as trial
    env, not a repo deliverable): 15 min, 1 job, 45,861 samples
    (~51 execs/s), 490 interesting, 63% valid, 20.91% of 55,814 edges
    with the corpus still growing at cutoff (last find 31s before end).
    Yield: SEVEN deterministic crashes. Four are sparse-array
    materialization OOMs (`memory allocation of 23–51G bytes failed`,
    3–4-line triggers) in the KNOWN-OPEN huge-sparse-join class — they
    dedupe to its open rule, which is the taxonomy working as designed.
    Three are NOVEL Boa bugs: (i) `("function").at("-9223372036854775807")`
    → negate-with-overflow panic at `builtins/string/mod.rs:541`
    (one-line reproducer; wraps silently without overflow checks);
    (ii) `class C5 extends BigUint64Array` + `using v6 = C5` in a
    constructor → index-OOB at `vm/opcode/define/mod.rs:126` (7 lines);
    (iii) a Promise/async-executor shape → `EnginePanic: cannot fail per
    spec` (caught only because the adapter aborts on engine panics —
    the CLI exits 1). Reproducers preserved under
    `pipeline/triage/fuzzilli-trial15/` (gitignored working state);
    fixes are out of P-4 scope and filed as findings, not fixes.
20. DECISION: ADOPT for periodic campaigns. Three novel bugs plus four
    class-confirms in 15 minutes on one job is overwhelming
    yield-per-triage-hour, and every crash minimized to a ≤14-line
    deterministic reproducer with signal + stderr captured. Scheduled-run
    integration (profile upstreaming, dual-schedule wiring, corpus sync)
    is follow-up work, not part of this trial. The OOM triggers are
    deliberately NOT staged as fleet seeds: full-scale allocation
    aborters would kill any run whose corpus they join — the existing
    small-scale huge-sparse seed remains the sentry shape for the class.

P4 gates are green below (appended 2026-10-03). P-4 is complete; the
exit-criteria verdict closes the section.

21. Oracle-graduate drill 3/3 (pre-built `drill/logic/run.sh`, run as
    found): sanity agreed Boa-vs-jsshell on all three probes, then the
    breaking patches were applied and all three diverged on the broken
    tree (tonumber `1` vs `NaN`; finally `43|43` vs `42|43`; typedarray
    `function|function` vs `undefined|function`), tree restored
    byte-identical (snapshot hash) and rebuilt green. No changes needed.
22. Seeded-crash drill 5/5 (`drill/crash-gate.sh` +
    `inventory-gate.toml`, built for the gate on the logic runner's
    snapshot+trap pattern): five historicals reintroduced at once —
    parser-nesting (WT bug #22 revert), padstart-OOM (419a8ed5),
    function-ctor (957e6f83), class-scope (46e92c75), dataview-assert
    (a6101fe1) — all rediscovered through fleet binaries with five
    distinct message+location signatures, 3x `tmin-ok` plus two
    hand-minimal boa-eval inputs (parser bisected to the exact 217-paren
    threshold; padstart is a single expression), tree restored
    byte-identical. Three forensics-grade lessons: (a) match the harness
    to the crash layer — parse-only source-bytes cannot re-trip
    compile/eval-time bugs (first attempt retired 4/5); (b) pids in
    panic lines make dedupe trivially pass, so the runner normalizes
    them out; (c) the parser break-patch transiently removes a Cargo
    dependency and cargo's lock rewrite-back is not byte-exact, so the
    wrapper snapshots `Cargo.lock` bytes (first attempt failed restore
    on exactly one lock line; diagnosed to the missing `stacker` edge).
    Excluded with reason: cyclic-proto (same SO message as parser,
    never distinct) and sloppy-this-assign (its todo!() arm is
    unreachable — the parser rejects `this =` first; incidentally, sloppy
    `this = 5` should be a legal no-op per spec, a latent finding).
    Cross-validation worth recording: the reverted function-ctor bug
    trips `define/mod.rs:126` (`must be declarative environment`) —
    the same file:line where the Fuzzilli trial's novel
    TypedArray-subclass crash trips (as index-OOB): two independent
    harnesses, one fragile site.
23. Full scheduled cycle + merge-back, demonstrated end to end. The
    PR-smoke command set ran all eight targets (300s/120s, seed 42):
    seven green, one find — a parser-universe oracle violation
    (`async *[yield]` computed key) that triaged cleanly
    (reproduce→dedupe-miss→tmin 74→37 bytes→skeleton), classified as
    harness-side (the generator emits a Yield node outside generator
    context; real-source round trips are stable both ways), and filed as
    an open DB entry with an open dedupe rule — zero untriaged. The
    dataview drill bug then walked the whole merge-back path:
    gate tmin → landed dedupe rule → corpus seed → regression-DB entry
    (landed) → responsible-layer unit test → `audit-fix.sh` green →
    coverage ratchet re-run, which genuinely gained +592 cli-exec
    regions from the new seeds. The same ratchet run caught the biggest
    P4.6 correction: `cargo fuzz coverage` never clears its profraw
    batches, so every measure had unioned all history (+13k phantom
    parser regions); the gate now clears per target per measure, and all
    eight baselines were re-cut bank-true (parser 1850, vm 8324,
    bytecompiler 8107, source-bytes 8121, module 5799, url 14970, fetch
    15225, cli-exec 24546), check-mode green 8/8. Two pipeline
    robustness fixes came out of the same work: triage fails fast on a
    missing artifact (previously a bad path triaged as a bogus crash)
    and anchors relative paths at the fuzz dir.
24. Gate test collateral: the deterministic
    `batched_forward_sub_batch_same_misalignment` unit test (offsets
    derived from the observed base, so any allocator layout trips;
    proven FAILED pre-fix at utils.rs:458 and green post-fix), one
    landed + one open DB entry, and the drill harness itself (5 break
    patches, gate inventory, crash-gate wrapper — all rerunnable).
    Recorded failure: the first unit-test shape (JS-level `getUint8(6)`
    on a fixed SAB) passed on the broken tree — the assert depends on
    heap layout, not just index, so behavior-level tests cannot pin it
    and it was replaced rather than kept. Suites, all observed fresh:
    engine lib 1129 (+1), regression DB 4/4 (negative-controlled: a
    sabotaged repro fails the run naming the entry), gc 36, fuzz-lib 55,
    tester 39+8; fmt clean; clippy zero in every file this phase
    touched (eval/reprl/main/utils to zero; cov.rs keeps only its
    pre-existing warnings — never edited here).
25. EXIT VERDICT — all P4 criteria met: fleet scheduled (smoke/nightly/
    sanitizer/coverage) + green (7/8 clean, 1/8 triaged-to-open, zero
    untriaged); crash pipeline demonstrated end-to-end on live finds
    (triage skeleton + open rule) and on the drill bug (full merge-back
    to seed + rule + DB + ratchet); GC-stress + sanitizer configs
    running per P4.4/P4.5; Fuzzilli ADOPTED with trial evidence (item
    20). P-4 done.
26. Post-P4 filing (2026-10-03): the three novel Fuzzilli trial bugs are now
    open regression-DB entries (27 total: 11 landed + 16 open), each with a
    minimized repro asserting the node-v26-oracle behavior, a non-tripping
    sentry seed (synced: 34 seeds/corpus), and a node/spec-cited root cause;
    `cargo test -p boa_regression` stays 4/4 green. Two corrections to item
    19: (a) the TypedArray-`using` crash rescoped — ANY `using` directly in
    function-body or script-top scope aborts (`function F(){using v=F}F();`
    suffices; nested blocks work), filed as `using-binding-index-oob`;
    (b) the `.at()` release behavior — the negate wraps but `get_expect`
    then trips its OOB expect, so release aborts too (by code reading), not
    "wraps silently". Filed: `string-at-negative-overflow` (INT64_MIN via
    f64 rounding of "-9223372036854775807", fix: compare without negating),
    `using-binding-index-oob` (top-scope env holds 0 slots for the binding),
    `await-recursion-engine-panic` (512-frame limit trip laundered by a
    `cannot fail per spec` js_expect; fix-loop chooses RangeError vs
    uncatchable propagation, entry records both). The sloppy `this = 5`
    latent nit from item 22 remains unfiled.
27. Step-0 follow-up (2026-10-06): two harness bugs found while rebasing onto
    upstream main, both fixed before the rebaseline. (a) Workspace feature
    unification: tools/fuzzilli (workspace member since P4.7) enables
    boa_engine/fuzz+verify-bytecode, and a bare `cargo --bin X` from the
    workspace root unifies those into the build — proven by diffing rustc
    `--cfg` lines (`-p` clean vs `--bin` fuzz-enabled). The resulting tester
    fails every test with NoInstructionsRemain (default budget 0). Fixed 10
    call sites (snapshot script, 5 workflows, 2 Makefile tasks) to `-p`-scoped
    form and added a `verify-baseline.sh` guard (proven both directions).
    Lesson: fuzz-enabling crates must stay out of default-member unification
    reach (tests/fuzz is excluded; tools/fuzzilli is not — tolerated now that
    all call sites are scoped and guarded). (b) Rayon worker stacks: the P4.3
    parser guard (256 KiB absolute red zone) plus rayon's 2 MiB default
    workers spuriously reject 32-deep nesting (test262 S13.2.1_A1_T1, needs
    ~3 MiB release / ~13 MiB debug by measurement). Sized the tester and
    differential pools to 32 MiB workers (stacks commit lazily). S13 green in
    both profiles afterwards; full suite deterministic at 51446/51446.

### P5 — Bytecode validity model, VM-level testing, metamorphic equivalence

**Goal.** Make the VM testable below the JS surface without false positives:
first write down what valid bytecode *is*, then generate valid-only inputs at
scale, and add metamorphic equivalence oracles that need no hand-written
expected outputs.

**Work items.**

5.1. **Validity model.** Document the invariants established by
`ByteCompiler::finish` (`core/engine/src/bytecompiler/mod.rs:2788`) and
required by the VM/`CodeBlock`: operand
encoding/decoding, register allocation bounds, jump-target well-formedness,
constant-table references, environment/scope indices, exception-table
coherence, IC slot correspondence. Deliverable: `docs/bytecode-validity.md`
plus machine-checkable assertions where practical (a `verify(&CodeBlock)`
routine used by tests/fuzzers; whether it also runs in production builds is a
separate Project-2 decision and defaults to no).
5.2. **Valid-only bytecode generator + VM differential.** Build a structured
generator producing *valid* bytecode/ASTs (extending the existing
`Arbitrary` machinery in `tests/fuzz/fuzz_targets/common.rs`): execute under
Boa vs the P3 oracle (via JS lifting where the unit is expressible, else
Boa-vs-Boa across configurations such as normal vs GC-stress vs instrumented
builds). Invalid inputs are tested only against surfaces that accept
untrusted bytecode, if any exist — otherwise invalid-input behavior is
explicitly out of contract and documented as such.
5.3. **Metamorphic transforms.** Implement semantics-preserving JS transforms
with proof sketches or spec citations each (constant folding of pure
expressions, dead-store elimination, loop unrolling by fixed factors,
expression reassociation only where the spec guarantees it, alpha-renaming,
statement reordering only where effect-free): run original-vs-transformed
under Boa and require identical observable behavior; run both under the
oracle to confirm the transform itself is sound. Counterexamples indict the
transform first, the engine second — both outcomes improve the system.
5.4. **Opcode/operand coverage matrix.** Instrument which opcodes, operand
forms, exception paths, and branch kinds the combined suites (Test262 +
regression + differential + fuzz) exercise; publish the matrix per commit.
Uncovered cells become targeted test-writing work items (not merely "more
fuzzing"): each cell gets a named test or a written justification.

#### P5 implementation notes (grounded in current code)

**Scope** (plan §P5): validity model of what `ByteCompiler::finish`
(`core/engine/src/bytecompiler/mod.rs:2788-2831`) establishes, valid-only
VM-level generation, metamorphic oracles, opcode matrix.

**1. Validity-model outline (`docs/bytecode-validity.md`), real field names**

`CodeBlock` (`core/engine/src/vm/code_block.rs:127-175`): `flags: Cell<CodeBlockFlags>`
(`core/engine/src/vm/code_block.rs:29-57`), `length`, `parameter_length`,
`register_count`, `this_mode: ThisMode`, `mapped_arguments_binding_indices`,
`bytecode: Bytecode`, `constants: ThinVec<Constant>`, `bindings: Box<[BindingLocator]>`,
`handlers: ThinVec<Handler>`, `ic: Box<[InlineCache]>`, `source_info`,
`global_lexs/global_fns/global_vars`, `debug_id`.
Invariants to document + check:
- Bytecode: `Bytecode.bytes: Box<[u8]>` (`core/engine/src/vm/opcode/mod.rs:184-188`)
  decodes fully via `Bytecode::next_instruction` (`core/engine/src/vm/opcode/mod.rs:490-509`);
  every `Address` operand (`core/engine/src/vm/opcode/mod.rs:190-232`) lands on an
  instruction boundary `< len`, never `DUMMY_ADDRESS = u32::MAX`
  (`core/engine/src/bytecompiler/mod.rs:597`); `JumpTable{index, addresses: ThinVec<Address>}`
  (`core/engine/src/vm/opcode/mod.rs:1715`) length matches patched table
  (`core/engine/src/vm/opcode/mod.rs:168-181`); no `Reserved1..61` opcodes
  (`core/engine/src/vm/opcode/mod.rs:2144-2266`; 195 real + 61 reserved = 256,
  `core/engine/src/vm/opcode/mod.rs:412`).
- Registers: every `RegisterOperand` (`core/engine/src/vm/opcode/mod.rs:234-279`)
  `< register_count`; `register_count = RegisterAllocator::finish()` = slots used
  (`core/engine/src/bytecompiler/register.rs:141-150`), sized into the frame at
  (`core/engine/src/vm/mod.rs:599-604`); persistent slot order must match `CallFrame`
  (undefined, promise triple, async-gen) (`core/engine/src/bytecompiler/mod.rs:625-656`).
- Constants: `Constant::{String,Function,BigInt,Scope}`
  (`core/engine/src/vm/code_block.rs:99-109`); each `IndexOperand`
  (`core/engine/src/vm/opcode/mod.rs:281-333`) in bounds AND of the type the opcode
  expects (`GetFunction`→`Function`, `StoreLiteral`→`String|BigInt`,
  `CallEval.scope_index`/`PushScope`→`Scope`), else `constant_string/function/scope`
  panic (`core/engine/src/vm/code_block.rs:319-353`); recurse into nested `Function`.
- Bindings: `BindingKind::{Global,Stack}` indices `< bindings.len()`
  (`core/engine/src/bytecompiler/mod.rs:578-582,777-801`); `Local(Some(i))` moves use
  `i < register_count` (`core/engine/src/bytecompiler/mod.rs:1004-1019`);
  `BindingLocator{name, scope 0|1|n+2, binding_index, unique_scope_id}`
  (`core/ast/src/scope.rs:594-610`) scope must match `BindingLocatorScope`
  (`core/ast/src/scope.rs:699-708`).
- Handlers: `Handler{start, end, environment_count}`
  (`core/engine/src/vm/code_block.rs:80-85`); `start <= end <= len`
  (cf. assert in `patch_handler`, `core/engine/src/bytecompiler/jump_control.rs:442-457`),
  `end != DUMMY`, `environment_count <= current_open_environments_count`
  (`core/engine/src/bytecompiler/mod.rs:533`); match `find_handler` rev-search +
  `[start,end)` (`core/engine/src/vm/code_block.rs:94-96,305-311`).
- IC: each `ic_index` (`GetNameGlobal`, `Get/SetPropertyByName*`) `< ic.len()`;
  `InlineCache{name, entries: GcRefCell<ArrayVec<_,PIC_CAPACITY=4>>, megamorphic}`
  (`core/engine/src/vm/inline_cache/mod.rs:16-39`) pushed 1:1 at emit sites
  (`core/engine/src/bytecompiler/mod.rs:933-940,1031-1058,1068-1074`); name matches
  the access site. Check bounds + name only; entries are runtime-populated.
- Globals: `global_lexs/global_vars` → `Constant::String`; `GlobalFunctionBinding{name_index,
  function_index}` → `String` + `Function` (`core/engine/src/vm/code_block.rs:112-119`).
- Tail: block ends with compiler-appended `Return`
  (`core/engine/src/bytecompiler/mod.rs:2793`); async blocks have patched handler
  (`core/engine/src/bytecompiler/mod.rs:2790-2792`); `mapped_arguments_binding_indices`
  nonempty only if `emitted_mapped_arguments_object_opcode`
  (`core/engine/src/bytecompiler/mod.rs:2797-2801`).

**2. `verify()` placement**

- New `core/engine/src/vm/verify.rs`: `pub(crate) fn verify(&CodeBlock) ->
  Result<(), VerifyError>` (new error enum, precise per-invariant variants + pc).
  Reuse `InstructionIterator` (`core/engine/src/vm/opcode/mod.rs:515-555`) for the
  decode walk and `instruction_operands`/Display (`core/engine/src/vm/code_block.rs:955-984`)
  for error context. Recurse `Constant::Function`.
- Wire-in (tests/fuzz only, never production default): call from a `#[cfg(any(test,
  feature="verify-bytecode"))]` hook at end of `finish`
  (`core/engine/src/bytecompiler/mod.rs:2807-2830`); Test262-acceptance test walks the
  pinned corpus compiling each file and asserting `verify` ok; invalid-input unit tests
  craft bad jumps/registers/truncated operands (operand decode asserts short reads,
  `core/engine/src/vm/opcode/args.rs:41-66`, so `verify` must pre-check lengths itself
  and return `Err`, never panic). Fuzzers call `verify` after `parsed.codeblock(&mut ctx)`
  (pattern at `tests/fuzz/fuzz_targets/bytecompiler-implied.rs:17-26`).

**3. Generator-extension design (extend, don't rebuild)**

- Extend `tests/fuzz/fuzz_targets/common.rs`: `FuzzData{interner, ast: StatementList}`
  + `Arbitrary` impl with `Sym`-remap via `VisitorMut`
  (`tests/fuzz/fuzz_targets/common.rs:14-61`) and `FuzzSource` lifting via
  `ast.to_interned_string(&interner)` (`tests/fuzz/fuzz_targets/common.rs:77-86`).
  `StatementList: Arbitrary` (`core/ast/src/statement_list.rs:214-223`) + ~100
  `#[cfg_attr(feature="arbitrary", derive(Arbitrary))]` AST nodes (e.g.
  `core/ast/src/expression/mod.rs:69`, `core/ast/src/statement/mod.rs:43`) already exist.
- Valid-by-construction path (primary): mutate AST with `VisitorMut`
  (`core/ast/src/visitor.rs:450-509`), lift with `ToInternedString`
  (`core/interner/src/lib.rs:607-620`, blanket impl for `ToIndentedString`), re-parse
  gate exactly like `parser-idempotency.rs:15-64` / `bytecompiler-implied.rs:17-26`
  (`Corpus::Reject` on parse failure), then `Script::parse` → `codeblock` →
  `verify` → execute under `Context::builder().instructions_remaining(1<<16)`
  (`tests/fuzz/fuzz_targets/vm-implied.rs:11-18`).
- Direct bytecode path (secondary, for non-JS-reachable shapes): build on
  `BytecodeEmitter::new/emit_*/patch_jump/patch_jump_table/next_opcode_location`
  (`core/engine/src/vm/opcode/mod.rs:136-182`), following the emit-then-patch
  `DUMMY_ADDRESS` protocol used by `jump_if_true/jump_if_false/patch_jump*`
  (`core/engine/src/bytecompiler/mod.rs:1147-1155,1227-1229,1441-1449`); hand-assemble
  `CodeBlock` via `CodeBlock::new` (`core/engine/src/vm/code_block.rs:181-208`) +
  field fill, then `verify` before execute.
- Differentials: (a) Boa-vs-oracle on lifted JS (needs P3 runner); (b) Boa-vs-Boa
  across configs (normal vs GC-stress vs instrumented) reusing the `vm-implied`
  harness; invalid bytecode tested ONLY against surfaces accepting untrusted
  bytecode (none known — document as out-of-contract otherwise).

**4. Metamorphic transforms (grounded in `Expression`/`Statement`)**

`Expression` variants (`core/ast/src/expression/mod.rs:71-181`), `Statement`
(`core/ast/src/statement/mod.rs:45-107`); lift both sides via `to_interned_string`
dispatch (`core/ast/src/expression/mod.rs:188-223`), compare observable behavior.
- Const-fold pure `Binary`/`Unary` on `Literal` operands (int/float/string only;
  skip `+` on mixed, `Pow`, division edge cases); drop `Parenthesized` round-trip.
- `Conditional` with literal condition → taken branch; `x && true`→`x` (keep order).
- Dead-store elim: remove `Expression` statements provably side-effect-free
  (bare `Literal`/`Identifier` reads — identifiers CAN throw ReferenceError, so
  only `Literal` unless bound-known); `Statement::Empty` insert/delete anywhere.
- Alpha-rename `Identifier` consistently via `visit_sym_mut`-style pass
  (pattern at `tests/fuzz/fuzz_targets/common.rs:45-48`), avoiding capture.
- Loop unroll `WhileLoop`/`ForLoop` by 2–4 with fresh bindings; `Block` flatten of
  nested bare blocks; `&&`/`||` regrouping preserving evaluation order only.
- Soundness gate first: run all transforms Boa-vs-oracle on 10k programs
  (P5 validation gate); counterexample indicts transform first.

**5. Opcode/operand coverage matrix (P5 builds it; P7.1b gates on it — single implementation)**

- Instrument `Opcode::as_str` (`core/engine/src/vm/opcode/mod.rs:387-393`) keyed
  counters: static pass (`InstructionIterator` over every Test262-compiled block)
  + dynamic VM dispatch pass; dims = opcode × operand form
  (`Address`/`RegisterOperand`/`IndexOperand`/`u32`/`ThinVec`, encoding in
  `core/engine/src/vm/opcode/args.rs:68-75,120-228`) × branch taken/untaken for the
  `Jump*`/`LogicalAnd|Or`/`Coalesce`/`Case`/`TemplateLookup` family
  (`core/engine/src/vm/opcode/mod.rs:955-978,1612-1715,1824,2096`) × handler hit/miss
  (`find_handler`). Publish per commit; every JS-reachable opcode needs a named test.

**Gotchas**: lifted source often invalid (nameless functions etc.) — always parse-gate
(`tests/fuzz/fuzz_targets/parser-idempotency.rs:21-23`); `Register: Drop` panics on
leak (`core/engine/src/bytecompiler/register.rs:62-77`) — generator must `dealloc`
(`core/engine/src/bytecompiler/register.rs:122-139`); any surviving `DUMMY_ADDRESS`
is a compiler bug; `verify` must be total (no `read` asserts, no `constant_*`
panics); `finish(self)` consumes the compiler; `strict:false` forced in fuzz ASTs
(`core/ast/src/statement_list.rs:220`).

**Validation / gate.**

- `verify(&CodeBlock)` accepts 100% of compiler-produced blocks across the
  full Test262 corpus and rejects a crafted invalid set (bad jumps, bad
  register indices, truncated operands) with precise errors.
- Metamorphic drill: transforms validated against the oracle on 10k programs
  with zero transform-soundness failures before engine comparison begins.
- Opcode matrix: every opcode reachable from JS has at least one direct test;
  exception-path coverage per opcode family measured and non-decreasing.
- **Exit criteria:** validity model documented + machine-checked; VM-level
  differential running; metamorphic suite green; opcode matrix published with
  zero unjustified-empty cells reachable from JS.

#### P5 as-built notes (appended 2026-10-03; append-only, original text above unchanged)

1. P5.0 grounding (2026-10-03): all plan line numbers verified against
   current code (`finish`, `CodeBlock`, `Bytecode`/`Address`/operands,
   `InstructionIterator`, 195 real + 61 reserved opcodes, `DUMMY_ADDRESS`,
   constant/binding/IC consumers).
2. P5.1 (2026-10-03): `docs/bytecode-validity.md` (validity model §§0-9 +
   [V]/[D] table) + `core/engine/src/vm/verify.rs` (`verify()` +
   `VerifyError`, total + sound) + `CodeBlock::verify()` + `finish` hook
   (`#[cfg(any(test, feature="verify-bytecode"))]`) + `verify-bytecode`
   feature + `try_read`/`decode_checked`/`try_next_instruction`. 27 unit
   tests incl. Test262 corpus acceptance (dual strictness, fresh `Context`
   per pair, >60k compiles); engine lib 1156 green. Template pairing is
   checked at the jump target (not fall-through); `verify` omits
   environment depth/liveness, locator resolution, `environment_count`,
   mapped-args indices (dynamic; static env-depth dataflow is future work).
3. P5.2 (2026-10-03): `tests/fuzz/src/mutate.rs` (5 structured mutators, 9
   tests) + `semantic.rs::eval_outcome_unoptimized` + enforcing
   `verify_hook`; `vm-implied.rs` rewired (parse-gate + normal/GC-stress +
   optimizer on/off + reprint + metamorphic + structured mutant + oracle);
   `core/engine/src/vm/direct.rs` (8 direct bytecode differential tests);
   `verify-bytecode` enabled in `tests/fuzz` + `tools/fuzzilli`. 90s smoke:
   1216 runs clean.
4. P5.3 transforms (2026-10-03): `tests/fuzz/src/metamorphic.rs` with 4
   semantics-preserving transforms (proof sketches + spec citations in
   module docs): T1 `paren-add`, T2 `const-fold` (Int/Num +,-,*; Int
   &|^; unary `-`/`!`), T3 `cond-fold` (literal conditions; `if(false)`
   without `else` excluded — completion-sensitivity), T4 `regroup`
   (same-operator `&&`/`||`/`??`/`&`/`|`/`^` with paren look-through).
   Deliberately skipped with reason: paren-*dropping* (the AST printer
   never inserts precedence parens, so dropped parens mis-reparse and no
   span-insensitive AST equality exists to verify the lift), dead-store
   elimination / statement reordering (sealed `StatementList` +
   completion-sensitivity), loop unrolling (needs break/continue/effect
   analysis), alpha-renaming (needs scope resolution), string folds
   (printer does not escape quotes). The unit value battery caught two
   real soundness bugs pre-gate: Int `0 * -x` / `-(0)` folding to `+0`
   instead of `-0` (fixed with the signed-zero rule).
5. P5.3 generator + red-zone hardening (2026-10-03): `FuzzData` from noise
   yields empty/failing programs, so the gate rolls a hermetic template
   generator (`gate_template_program`: valid + terminating by
   construction — bounded loops, no recursion/eval/getters; `??`-mixing,
   assignment-level, numeric-`.prop`, and printer-alphabet rules
   documented). Pilot triage found the parser's 256 KB stack red-zone
   (bug-#22 hardening) tripping at ~10 nesting levels on 2 MB test
   threads (~212 on 8 MB main, both measured): paren-add variants
   straddled it constantly (32/200 false engine divergences). Fix:
   `MAX_DERIVED_NESTING = 256` (`semantic.rs`, variants past it dropped;
   templates ≤ ~12, corpus ≤ ~100) + 64 MB worker threads for the loop
   (`vm-implied`, `FuzzSource` stays `!Send` so only the source text
   crosses; panics `resume_unwind`) and the gate (scope threads cannot
   set stack size, so `Builder` + `Arc`).
6. P5.3 gate + first find (2026-10-03): `soundness_gate_10k` (ignored,
   `BOA_FUZZ_ORACLE=<node|jsshell>`, `BOA_GATE_PROGRAMS` pilot override,
   8 workers, fuel pre-screen, per-transform anti-vacuity floors,
   deterministic seeds, sort-by-seed failure reports) passed on 10k
   programs / 24,593 pairs with ZERO oracle disagreements — and 1 engine
   divergence, filed as `dce-dead-branch-completion-leak` (regression DB
   entry 28: 11 landed + 17 open): DCE replaces falsy `if`/`while`/`for`
   with `Statement::Empty`, leaking the previous completion instead of
   `undefined` (`0; if (false) { 1; }` → `0` under `-O`, node:
   `undefined`; same for `while`/`for`). Unoptimized contexts are
   correct; the Test262 runner optimizes only with `-O` (default off),
   so no suite run exercises DCE. JS repro is documentary (script
   completions unobservable from inside; `eval`/`Function` compile
   unoptimized); the planned Rust unit test is the guard.
7. P5.3 loop hardening (2026-10-03): `eval_outcome_with_optimizer` +
   `is_filed_dce_completion_leak` (signature + DCE-off mechanism
   confirmation; skips only the filed shape) wired into `check_legs`;
   the metamorphic leg re-compares unoptimized on divergence (variant
   optimizer leg by name, skip when both sides agree unoptimized);
   `check_legs` + `check_metamorphic_pair` moved to `semantic.rs` so the
   wiring is unit-pinned (`legs_silently_pass_filed_dce_shape`, plus
   fingerprint tests; all fail loud when the DCE fix lands). Fuzz lib 82
   green; regression DB 4/4; two 90s post-change smokes clean (669 +
   714 runs at ~7 exec/s).
8. P5.4a engine hooks (2026-10-03): `vm-coverage` feature +
   `core/engine/src/vm/coverage.rs` (process-global histogram, JSON-safe
   cell alphabet, `coverage_dump_json`, `emitted_cells` static scan,
   single `refinement_cell` grammar shared by the dynamic pre-hook and
   the static scan) + `Vm::coverage_current` + `execute_one` pre/post
   hooks + `handle_error` entry hook. Cell grammar: base opcode plus
   `|taken`/`|nottaken` (9 conditional jumps only), `|argv-N`/`|argv-many`
   (4 fixed-arity calls, cap 7), `|k-str`/`|k-bigint` (`StoreLiteral`
   only — every other constant use-site is model-fixed), `|err-T`
   (surfacing semantics: the error passing through `handle_error` while
   the op is current; `opaque`/`engine` for user/engine errors).
   Corrections to the plan sketch: the run loop lives in `vm/mod.rs`
   (`run`/`execute_one`), not flowgraph; errors flow through the
   `IntoCompletionRecord` choke, so the opcode is threaded via a
   `vm-coverage`-gated `Vm` field; unconditional `Jump` and `JumpTable`
   emit base only; pending-exception attribution is absorbed by the
   surfacing definition. 9 eval-driven unit tests (serial-guarded; the
   histogram is process-global); full engine suite green with the
   feature. `Opcode::as_str` already existed — no macro work needed.
9. P5.4a/b battery + matrix lib (2026-10-03): gate generator hoisted to
   `tests/fuzz/src/battery.rs` (`battery_program(seed)` minimal API;
   `BatteryRng` + helpers private) with determinism/density/termination
   tests; `tests/fuzz/src/matrix.rs` (`parse_cell`, `merge_matrix`,
   `merge_rows`, `BATTERY_VERSION`, 7 unit tests incl. fingerprint rows).
   The matrix driver lives in `tests/fuzz` as `examples/matrix.rs`
   (tests/fuzz is its own workspace, so a `tools/` crate could not share
   the battery). Fuzz lib 92 green; clippy zero-new both sides.
10. P5.4b matrix tool + feeds (2026-10-03): `examples/matrix.rs` runs
    battery + `--seeds` + `--files-from` + extras through verify +
    static + isolated-dynamic (32 MB worker per program, sized to match
    the main thread; counts
    proven identical to inline; file panics recorded with static kept,
    battery panics fail at exit) and writes per-program + merged JSON;
    `--merge` combines matrix/tester reports; `--static-only`,
    `--optimize` modes. `boa_tester --coverage-out` (dynamic-only,
    conflicts with `--timeout`, needs `--features coverage`) writes the
    suite-global histogram. Runs: 10k battery + 63 seeds unoptimized
    (17 s, 136 rows, 2 known file panics recorded, exit 0) and optimized
    (+exactly `StoreNan`, `StoreNegativeInfinity`, nothing lost);
    debug==release merged identical; Test262 static 48,378 parsed /
    5,494 skipped (negative + module-goal + fixtures; script-goal only,
    module cells go dynamic-only) → 218 rows; merged static 191/195
    base. Gap probes: `0/0` + `-1e999` (optimized) close the NaN pair;
    function-scoped `eval("var q…")` closes `DefEvalVar` (dynamic-only —
    runtime-compiled); `ImportMeta` awaits the Test262 dynamic run.
    Full Test262 `--coverage-out` run in progress at time of writing.
11. P5.4b Test262 dynamic campaign (2026-10-04): the single full-suite
    run was SIGKILLed after 1 h and a reboot wiped `/tmp`, so the feed
    was re-driven as 148 disjoint depth-3 chunks (`--coverage-out` each,
    300 s timeout, 8-way) plus two salvage passes (depth-4, then files
    at 60 s): 1439 chunk files, zero losses, zero double-counts. Coverage
    tax measures ~zero (Array subset 5.1 s vs 6.3 s baseline noise); the
    slow chunks are pre-existing engine performance pathologies
    (`built-ins/RegExp`, `decodeURI*`, `staging/sm/*` spin past any
    in-process budget), not hook overhead. All scratch moved to
    `target/matrix-scratch/` (reboot-proof, gitignored).
12. P5.4c ratchet + gap closure (2026-10-04): `tests/fuzz/matrix/`
    (`opcodes.txt` 195, `ratchet.json` 259 rows, `justifications.toml`
    0 zero + 93 deep, `test262-dynamic.json` 339 rows, `regen.py`,
    `README.md`) + `matrix --check` gate (version/defensive/new/stale/
    floors/justified-zero/stale-just/promote rules, 8 unit tests) +
    `tests/fuzz/probes/` (8 gap probes). Closure highlights: the tool's
    dynamic leg now drains the job queue (`eval_outcome_drained_*`, unit
    differential-pinned) for tester parity — undrained async resumptions
    were invisible, hiding `JumpIfNotEqual|nottaken`; `|argv` gaps closed
    by arity probes; fused/branch arms by emitter-mapped probes (chunk
    attribution found the async-resume source). Final: 195/195 base
    opcodes, closed universe 251 (250 deterministic + `ImportMeta`
    deep-only), zero malformed anywhere. `--check` proven both ways
    (tampered fresh fails 3 rules; full regen passes = determinism).
    P5 gates: fmt clean; clippy zero-new (engine/tester/fuzz); engine
    1173 + fuzz-lib 100 + regression 4/4 green; gate pilot 200 PASS
    post-hoist; `verify-baseline.sh` has 1 PRE-EXISTING failure (main
    `Cargo.lock` dirtied by P4-era workspace members — required for the
    build, fixable only by a commit, which the repo rule forbids;
    P5.4 adds zero main-lock entries and the fuzz lock is ignored).

### P6 — Unsafe audit, Miri expansion, sanitizers, Kani kernels, Loom-if-any

**Goal.** Rule out memory unsafety in the unsafe-adjacent core with
overlapping independent methods (audit + Miri + sanitizers + bounded proofs),
each of which is understood to be incomplete on its own — which is exactly why
all of them run.

**Work items.**

6.1. **Complete the unsafe audit.** For every P0-inventory site: write the
`SAFETY` justification (allocation, alignment, lifetime, aliasing, and which
safe API upholds the soundness obligation — the Rust Reference rule that
`unsafe` never excuses UB:
`https://doc.rust-lang.org/reference/behavior-considered-undefined.html`),
name the covering tests, assign an owner. Sites that cannot be justified are
refactored into safe code or justified narrow wrappers. Inventory deltas
require audit sign-off from here on (CI check comparing against
`docs/unsafe-inventory.*`, same mechanism as the ignore-list lint).
6.2. **Expand Miri.** Broaden today's narrow filter (`cargo miri test
--workspace --exclude boa_cli --exclude boa_examples miri`,
`rust.yml:348-349`) to the full unsafe core test set: value representation,
GC (allocation/tracing/weak/ephemeron paths), string/interner, object/property
machinery, IC paths. Keep the pinned nightly + `MIRIFLAGS` discipline from
`.cargo/config.toml` and P0. Miri is understood per its own docs to test one
execution among many
(`https://raw.githubusercontent.com/rust-lang/miri/master/README.md`) —
hence multiple seeds/configs on a schedule, plus sanitizers below.
6.3. **Sanitizer test runs.** Run the unsafe-core test suites under
ASan+UBSan on a schedule (nightly toolchain, `RUSTFLAGS="-Zsanitizer=..."`;
exact flags pinned in P0), in addition to the P4 fuzz-instrumented runs.
Failures become regression-DB entries like any crash.
6.4. **Kani kernels.** Write bounded proofs (harnesses) for, at minimum: (a)
`JsValue` NaN-box tag encode/decode round-trip and niche invariants
(`core/engine/src/value/*`); (b) GC rooting obligations — representative
allocation/reachability patterns never collect live objects nor corrupt live
structure (`core/gc/*`); (c) the P5 bytecode validity checker accepts exactly
the compiler's output language on bounded inputs. Kani usage follows its
book (harnesses + `kani::any()` all-values checking —
`https://raw.githubusercontent.com/model-checking/kani/main/README.md`).
Bounds, unwinding limits, and residual risks are documented per harness;
anything that exhausts resources is narrowed or replaced by a tested argument,
never silently dropped.
6.5. **Loom, conditional on P0.** If the concurrency inventory found real
concurrent code, model it under Loom (`RUSTFLAGS="--cfg loom"`, per
`https://raw.githubusercontent.com/tokio-rs/loom/master/README.md`),
including its documented C11-model gaps in the residual-risk notes. If not
(expected), record the negative result with the inventory reference and close
this item without theater.

#### P6 implementation notes (grounded in current code)

**6.1 Audit procedure.** Enumerate `unsafe` under `core/*`; per site record
allocation/alignment/lifetime/aliasing argument, soundness obligation,
covering tests, owner. Extend the in-tree lint denies (`core/string/src/
lib.rs:8-12`, also `builtins/array_buffer/mod.rs:10-11`,
`builtins/atomics/futex.rs:138-139`, `builtins/typed_array/element/mod.rs:1`)
to `core/engine/src/value/*` and `core/gc` so new unjustified `unsafe` fails
CI. Gate inventory deltas by diffing `docs/unsafe-inventory.*` (P1 lint pattern).

**Site taxonomy (all bodies opened).**
- *A. NaN-boxed `JsValue` codec* (`core/engine/src/value/inner.rs:4-12`
  selects nan-boxed vs `jsvalue-enum` legacy). Tag/untag pure fns at
  `core/engine/src/value/inner/nan_boxed.rs:143-309`; `tag_pointer`
  (`nan_boxed.rs:278-302`) masks to 48 bits, panics off-platform
  (`nan_boxed.rs:283-288`, TODO at `294-296`); const-asserts at `312-322`.
  `unsafe` surface: `as_*_unchecked` reconstruct `JsBigInt`/`JsObject`/
  `JsSymbol`/`JsString` via `ptr.with_addr` + `from_raw`
  (`nan_boxed.rs:655-663,684-692,713-721,742-750`), each with `# Safety: inner
  value must be valid`; callers guard on `is_*` tag checks with `SAFETY: must
  hold a valid, non-null` comments (`nan_boxed.rs:641-642,670-671,699-700,
  728-729`); `Clone` uses `mem::forget` after cloning the pointee
  (`nan_boxed.rs:371-382`); `Trace` marks only `Object`
  (`nan_boxed.rs:356-362`) — strings/symbols/bigints are `Rc`-based, not GC;
  audit must prove no `Gc`-owned payload hides behind those tags.
- *B. GC core* (`core/gc/src/lib.rs:44-51` thread-local `BOA_GC`; allocator
  `Box::into_raw` + `NonNull::new_unchecked` at `lib.rs:138,157,179`).
  `Collector::collect` (`lib.rs:225-278`) runs mark→finalize→mark→sweep;
  `unsafe fn finalize` (`lib.rs:422-438`) and `unsafe fn sweep`
  (`lib.rs:446-496`) document caller-validity contracts; `DropGuard`
  (`lib.rs:101-114`) + `finalizer_safe` (`lib.rs:119-121`) forbid deref during
  sweep. `GcBox` is `#[repr(C)]` header+vtable+value
  (`core/gc/src/internals/gc_box.rs:7-12`); erased dispatch via `VTable`
  fn-pointers (`core/gc/src/internals/vtable.rs:10-44,61-74`). `Gc::from_raw`
  (`core/gc/src/pointers/gc.rs:234-239`), `cast_unchecked`
  (`pointers/gc.rs:278-285`), `cast_ref_unchecked` (`pointers/gc.rs:294-297`)
  carry caller-validity docs; `Clone` inc-refs then `from_raw`
  (`pointers/gc.rs:346-356`); `Drop` routes through `Finalize`
  (`pointers/gc.rs:367-373`). `GcHeader` rooting = `non_root<ref`
  (`gc_header.rs:108-110`) with saturation cap (`gc_header.rs:40-63`) — the
  anti-UAF linchpin. `GcRefCell` (`cell.rs:95-98`): `Trace` skips `Writing`
  (`cell.rs:231-256`); `GcRef::cast`/`GcRefMut::cast` (`cell.rs:291,436`) are
  caller-cast contracts; stale `SAFETY` wording at `cell.rs:164` — fix in audit.
  `EphemeronBox` holds `UnsafeCell<Option<Data>>` with raw key pointer
  (`core/gc/src/internals/ephemeron_box.rs:7-15`); `value/key_ptr/key/set`
  (`ephemeron_box.rs:43-99`) all `# Safety: no live mutable refs`; key-liveness
  trace at `ephemeron_box.rs:139-164`.
- *C. `boa_string`* (`Cell<usize>` refcount, `#[repr(C)]` vtable-first
  layouts). `JsString::from_raw/from_ptr/slice_unchecked/as_inner`
  (`core/string/src/lib.rs:514,526,573,611`) with per-fn safety docs;
  `JsStringBuilder`: `alloc`/`realloc` (`core/string/src/builder.rs:89,163`),
  `data()` via `byte_add(DATA_OFFSET)` (`builder.rs:134-138`),
  `extend_from_slice_unchecked`/`push_unchecked`/`set_len`
  (`builder.rs:200,292,58`) with capacity/in-bounds contracts, `build_inner`
  writes header then `mem::forget`s the builder (`builder.rs:357-365`).
  `SequenceString::try_allocate` (`core/string/src/vtable/sequence.rs:70-117`)
  asserts `DATA_OFFSET`; vtable fns cast `NonNull<JsStringVTable>` back with
  `SAFETY: validated on construction` (`sequence.rs:130`, `slice.rs:61,73,
  92,106`); `SliceString::new` (`slice.rs:30-48`) extends a `JsStr` borrow to
  `'static` tied to `owned` clone — audit aliasing between `inner`/`owned`.
- *D. TypedArray/ArrayBuffer/shared memory.* `SliceRef::get_value` /
  `SliceRefMut::set_value` (`core/engine/src/builtins/array_buffer/utils.rs:
  131,330`) document size+alignment contracts; `Element::read/read_mut`
  (`core/engine/src/builtins/typed_array/element/mod.rs:246,256`) implemented
  by macro with debug len/align asserts (`element/mod.rs:305-337`); batched
  atomic copies + `memcpy`/`memmove` (`utils.rs:428,507,584,656,764,819`).
  `FutexWaiters`: `unsafe impl Send` justified by global-mutex confinement
  (`core/engine/src/builtins/atomics/futex.rs:245-248`); `add_waiter` builds
  `UnsafeRef` from `&FutexWaiter` (`futex.rs:269-277`). Smaller sites:
  `to_int_unchecked` after range check (`core/engine/src/builtins/string/
  mod.rs:338-342`).
- *E. `Trace` obligations.* `unsafe trait Trace` (`core/gc/src/trace.rs:78-96`)
  warns wrong impls cause UAF/UB; `Tracer::trace_until_empty` (`trace.rs:43-56`)
  requires queue-validity. Every `#[unsafe_ignore_trace]` /
  `#[boa_gc(unsafe_empty_trace)]` / `#[boa_gc(unsafe_no_drop)]` (builtins
  promise/intl/temporal/iterator files) must prove the skipped field holds no live `Gc`.

**Rooting API contract (auditors check against this).** `Gc::trace` only
enqueues (`pointers/gc.rs:333-335`); `trace_non_roots` incs the heap
non-root count (`pointers/gc.rs:337-339`); `mark_heap`
(`lib.rs:301-417`) roots on `is_rooted()` then drains the queue; ephemeron
fixpoint at `lib.rs:379-403`; weak maps cleared post-sweep (`lib.rs:257-273`).
Rule: every `Gc` reachable only from the heap must be visited by
`trace_non_roots`, else it is wrongly rooted (leak) or, if counts corrupt,
freed live (UAF — hence the `gc_header.rs:40-63` cap).

**6.2 Miri expansion.** Today: job `Miri` (`.github/workflows/rust.yml:
316-349`), nightly + `cargo miri setup`, command `cargo miri test --workspace
--exclude boa_cli --exclude boa_examples miri` (`rust.yml:348-349`) — `miri`
is a substring filter, so only tests whose path contains "miri" run:
`core/gc/src/test/allocation.rs:1`, `cell.rs:1`, `erased.rs:1`, `weak.rs:1`,
`weak_map.rs:1` (`mod miri`), `core/engine/src/builtins/
finalization_registry/tests.rs:1` (`mod miri`), `core/engine/src/builtins/
array_buffer/utils.rs:881` (`mod tests_miri`). `MIRIFLAGS=-Zmiri-tree-borrows`
(`.cargo/config.toml:6`); engine tests avoid OS APIs under Miri via
`FixedClock`+`IdleModuleLoader` (`core/engine/src/lib.rs:362-373`); slow test
ignored under Miri (`allocation.rs:37-38`). Expansion targets (concrete):
`core/gc/src/test/std_types.rs` + `internals/gc_header.rs:135-219` (no miri
coverage today); new `mod miri` in `core/engine/src/value/*` (tag round-trip,
`as_variant`, `to_boolean` paths at `nan_boxed.rs:759+`), `core/string/src/
tests.rs`, `core/engine/src/builtins/typed_array/*` (extend `utils.rs:881`
pattern to `element/mod.rs` read/write + `set_value`/`get_value`), object/
property + IC tests, and `boa_engine` unit tests exercising `GcRefCell`
borrow panic paths. Keep pinned nightly + `MIRIFLAGS` discipline from P0;
run multi-seed on schedule (Miri checks one execution only).

**6.3 Sanitizers (P6 owns sanitizer `cargo test` runs; P4 owns fuzz-instrumented runs).**
Scheduled `RUSTFLAGS="-Zsanitizer=address,undefined"
cargo test -p boa_gc -p boa_string -p boa_engine …` (flags pinned in P0) over
the Miri test set; failures enter the regression DB. ASan covers allocator
bugs (`builder.rs:89,163`, `sequence.rs:83`, `lib.rs:138`); UBSan covers
align/bounds assumptions (`element/mod.rs:305-337`, `utils.rs:131,330`).

**6.4 Kani.** No Kani in tree today (repo-wide search for "kani" is empty).
Feasibility: all three kernels use `std` (`nan_boxed.rs:119-122`,
`trace.rs:1-16`, `BOA_GC` thread-locals at `lib.rs:44-51`), so harnesses run
with Kani's std support, bounded and extracted-small: (a) JsValue codec —
harness beside `core/engine/src/value/inner/nan_boxed.rs` proving
`tag_*`/`untag_*` round-trips (`nan_boxed.rs:244-308`) and tag-disjointness
for `kani::any::<u64>()` inputs (pure, no alloc — cheapest proof; assert
`is_float`/`is_*` partition the u64 space); (b) GC rooting — harness in
`core/gc` (new `kani/` module or `#[cfg(kani)]` in `test/`) over
`GcHeader::{inc_ref_count,inc_non_root_count,is_rooted}` (`gc_header.rs:
40-110`) proving `non_root<=ref` invariant and no over-mark on bounded
op-sequences, plus a small `Gc`-graph test reusing `test/mod.rs:54-58`
`run_test` thread isolation to avoid cross-harness `BOA_GC` state
(thread-locals + global `RefCell` are the scaling risk — narrow to headers
if full `collect` explodes); (c) validity checker — harness beside the
P5 `verify(&CodeBlock)` routine proving accept/reject on bounded blocks;
depends on `ByteCompiler::finish` (`core/engine/src/bytecompiler/mod.rs:2788`)
output language. Document bounds/unwind limits per harness; narrow, never
silently drop (plan risk 3).

**6.5 Loom.** Expected negative: thread-local `BOA_GC` (`lib.rs:44-51`),
`Rc`-flavoured `Gc` (`pointers/gc.rs:157`), `!Sync` `SequenceString`
(`sequence.rs:16-18`); only shared-memory code is the mutex-guarded futex map
(`futex.rs:240-248`) and atomic buffer ops (`utils.rs`). Confirm via P0
inventory and close, else model `notify_many` (`futex.rs:299+`) under `--cfg loom`.

**Gotchas.** (1) `cargo miri test … miri` is substring matching — renaming a
mod drops coverage silently; the 6.2 gate must assert the expanded list is
non-empty. (2) `Trace for NanBoxedValue` skipping non-Object tags holds only
while those payloads stay non-GC — re-verify on any repr change. (3)
`GcRefCell::trace` skipping `Writing` can miss edges if a collection lands
inside `borrow_mut` — audit long-lived borrows. (4) `SliceString` `'static`
extension (`slice.rs:45`) and every `from_raw`/`from_ptr` caller need explicit
owner arguments. (5) Kani on full `collect` will likely explode on
thread-local + `Vec` state — start at `GcHeader` + codec pure fns. (6)
Seeded-UB drill: one bug per layer; record which layer caught which.

**Validation / gate.**

- Audit completeness script: 100% of inventoried sites justified + owned +
  test-covered; CI fails on new unjustified `unsafe`.
- Miri suite green on the expanded filter across the adopted seed set;
  sanitizer runs green; Kani harnesses prove (or bounded-pass with documented
  bounds) with zero failures.
- Seeded-UB drill: introduce three representative UB/panic bugs in unsafe-adjacent
  code in a scratch branch → at least one of (audit checklist, Miri, sanitizer,
  Kani) catches each; record which layer caught which.
- **Exit criteria:** inventory 100% justified; Miri+sanitizer+Kani green on
  schedules; Loom done or negatively closed; seeded-UB drill demonstrated.

**P6 progress notes (append-only).**

- P6.0 grounding CLOSED: CI pins stable 1.94.0 (no nightly/miri/kani in CI);
  local `nightly-2026-10-01` + Miri sysroot ready, `kani-verifier 0.68.0`
  installed + first-time setup done (logs in `target/*.log`, all EXIT=0).
- 6.1 unsafe audit CLOSED at 724/724 (exit 0). Generator schema v3 (bypass
  sites carry `// =>` target context); 360 first-match-wins rules incl. a
  fail-closed `line(s)` selector for code-identical blocks; committed
  `docs/unsafe-inventory.json` always fully derived (regen + rules, never
  hand-edited); new `unsafe_inventory` CI job runs
  `scripts/check-unsafe-inventory.sh` (stale-or-uncovered fails).
  Warn-level `missing_safety_doc`/`undocumented_unsafe_blocks` deliberately
  NOT enabled (`-D warnings` on main would fail pre-existing sites).
- Gotcha (3) RESOLVED: `GcRefCell::trace` + `trace_non_roots` both skip
  while `Writing` — sound by paired conservatism (pointees look rooted and
  survive; at worst floating garbage). Gotcha (4) closed per site.
- Backstop theorem (from `collect`): untraced `Gc` handles root their
  targets — 5 sites rely on it (DateTimeFormat caches, namespace
  resolved_bindings, synthetic-module captures, ProcessProvider, Logger),
  each analyzed leak-free via owner-drop + traced back-edges.
- SAFETY fixes: builder `Layout::new` (no `&` to uninit header) +
  non-overlap doc; forward-copy `src >= dest` contract; interner SAFETY
  comment corrections. Cleared alarms: uri lookahead, wintertc `replace`,
  JsObject `repr(C)` upcasts, ephemeron clear-before-sweep ordering.
- Gates: `fmt` clean; clippy zero-new (2 pre-existing in
  `iterator_helper`); boa_string 25 + boa_interner 8 + array_buffer utils 5
  green; `verify-baseline.sh` 1 pre-existing Cargo.lock failure (unchanged).
- 6.2 Miri expansion CLOSED. 8 -> 16 `mod miri` sites (gc_header,
  std_types, nan_boxed 9, string 22, inline_cache 10, object 3+1,
  attribute 14, new element roundtrips 3); `miri_seeds` nightly job
  (seeds 1,2,3); `scripts/check-miri-filter.sh` 6 checks (16-mod
  allowlist, fuzz exclusion, `--test-threads=1`, sandboxed context
  constructor, miri-ignore exact allowlist with `MIRI-IGNORE:`
  justifications, in-file `#[cfg(test)]` gating).
  Root causes fixed: `boa_engine/fuzz` unification -> `--exclude boa_fuzz
  --exclude boa_fuzzilli`; parallelism OOM (6.9GB @ 16 threads, 6.4GB @
  2) -> `--test-threads=1`; `Context::default()` canonicalize abort ->
  shared `test_context()` helper, 11 sites converted; `mod miri` in
  nan_boxed lacked `#[cfg(test)]` (leaked unused import into normal
  builds) -> gated. Ignores (3, all justified): `cyclic_prototype_walk`
  (4.3GB serial infinite-walk OOM), `gc_recursion` (pre-existing 1M-node
  chain), `test_file_trace` (`File::open` is an isolation abort, not
  `Err`). FULL SUITE GREEN (`target/miri-expanded.log`, EXPANDED_EXIT=0):
  engine 48+1ign/613s, gc 34+2ign/2045s, string 22/7s; 104 oks, 0 UB.
  Multi-seed green: string 22/22 x3, engine 48+1ign x3
  (`target/miri-seeds.log`). Gates: fmt clean, clippy zero-new (2
  pre-existing iterator_helper), engine 1168 + gc 36 + string 25 normal,
  inventory 727/727 zero drift, verify-baseline 1 pre-existing
  Cargo.lock failure only.
- 6.3 Sanitizers CLOSED. Spec deviation (grounded): `-Zsanitizer=undefined`
  does not exist on pinned nightly-2026-10-01 (rustc rejects it; the
  `ubsan.a` runtime only serves CFI) -> 6.3 runs ASan +
  `-Zub-checks=yes`, with Miri 6.2 backfilling alignment/validity UB;
  CFI considered and rejected. Proc-macro crates refuse sanitizer flags
  -> `-Zhost-config`/`-Ztarget-applies-to-host` split in new
  `scripts/run-sanitizer.sh` (fail-closed on `RUSTFLAGS`, artifacts in
  `target/sanitizer/`). GREEN: miri set 22+36+49, then FULL unsafe-core
  suites engine 1168 (467s) + gc 36 + string 25, SAN_EXIT=0, ASan+LSan+
  ub-checks silent (`target/sanitizer-*.log`). New `sanitizer` job on the
  nightly schedule (llvm-tools + symbolizer on PATH for triage). No
  failures -> no regression-DB entries. Gates: fmt clean, no Rust
  touched (clippy unchanged), verify-baseline 1 pre-existing failure.
- 6.5 Loom CLOSED negative (expected outcome, premises re-verified
  2026-10-04, not just cited): (a) engine/GC state is thread-local
  (`gc/lib.rs:44-51`, `context/mod.rs:48-50`) — no shared memory to
  model; (b) shared-buffer atomics are single-shot ops
  (`atomics/mod.rs:320-338` one CAS per type, no retry loop;
  `shared.rs:380` one `fetch_update`, all `SeqCst`) — no multi-step
  lock-free protocol for Loom to interleave; covered by Miri 6.2 + ASan
  6.3 + atomics stress tests; (c) futex map is mutex-guarded
  (`futex.rs:252-253`, `Send` at :248) — mutex-correctness is the
  Mutex's job; (d) symbol registry is third-party `DashMap`
  (`symbol/mod.rs:36,44`); (e) all 4 `Send`/`Sync` impls confirmed at
  inventoried lines. No `--cfg loom` code exists because there is no
  first-party lock-free protocol to check. Ref: `docs/concurrency-
  inventory.md` verdict section.
- 6.4 Kani kernels CLOSED: 7 proofs green (`cargo kani`, KANI_EXIT=0).
  (a) codec 4/4 complete, loop-free (i32/bool/f64-canonical/pointer
  round-trips + exactly-once classification), non-vacuity proven by a
  flipped-assertion control (FAILED, reverted). (b) GC headers 2/2
  (cap-exact with unwind 8, mark-orthogonal ghost with unwind 9);
  full-collect graph narrowed: Kani 0.68 ICEs on any thread-local
  access (`intrinsics.rs:243`, proven by 3-line probe) — tested
  argument = Miri/ASan gc suites. (c) verify header-rules 1/1
  (equivalence over all flags/counts); whole-`verify` narrowed (CBMC
  OOM on 1-byte input; `goto-instrument` SIGKilled even on empty) and
  jump/register kernels narrowed (`std` HashSet pulls lazy-static
  `futex`, an unsupported construct) — tested argument = P5 battery +
  27 unit tests. Env finding: `cargo kani` 0.68 auto-provides the kani
  crate (explicit dep causes duplicate-registration errors; vendoring
  detour removed, zero `Cargo.lock` impact). `cfg(kani)` declared in
  workspace lints (normal builds silent); clippy zero-new.
- Seeded-UB drill CLOSED (method: sequential introduce->run->revert in the
  working tree, zero residue verified by grep after each; no commits per
  repo rule — the plan's "scratch branch" realized without history).
  Bug 1 (Kani): non-canonical NaN payload in `tag_f64` — native GREEN,
  Miri GREEN, Kani RED naming `NaN canonicalizes` (unique catch; ASan N/A,
  no memory ops in the seeded line). Bug 2 (Miri): `memmove`
  `copy`->`copy_nonoverlapping` overlap-UB with a temporary vehicle test
  (reverted) — native RED (debug UB-check abort), Miri RED (UB:
  overlapping ranges, exact site), sanitizer RED (same UB-check abort, no
  ASan report: in-bounds as predicted); contradicts the audit's
  overlap-tolerant `memmove` contract. Bug 3 (sanitizer): builder `push`
  capacity off-by-one — native RED (SIGSEGV), Miri RED (UB: dangling
  pointer), ASan RED (SEGV report at `ptr::write`, proving the 6.3
  pipeline goes red with diagnostics). Bonus observation (drill-adjacent):
  a skipped borrow-check seed trips the drop-time `debug_assert` in ALL
  runners — defense-in-depth tripwires are live everywhere. Every bug
  caught by >=1 layer; per-layer liveness demonstrated (Kani
  counterexample, 2 Miri UB reports, 1 ASan report, 2 UB-check aborts).
- P6 CLOSED. Exit criteria: inventory 727/727 justified+owned+covered
  with CI diff check; Miri green (16 mods, 104 tests, multi-seed) on PR
  + nightly schedule; sanitizer green (full unsafe-core suites) on
  nightly schedule; Kani green (7 proofs) on nightly schedule (new
  `kani` job; like `miri_seeds`/`sanitizer`, CI-unproven until the first
  scheduled run); Loom negatively closed with re-verified premises;
  seeded-UB drill demonstrated. Final gates: fmt clean, clippy zero-new
  (2 pre-existing iterator_helper + 7 pre-existing fuzzilli under
  --all-targets), engine 1168 + gc 36 + string 25 + interner 8 normal,
  verify-baseline 1 pre-existing Cargo.lock failure only.

### P7 — Multi-dimensional coverage, mutation adequacy, readiness battery (the gate that never opens again)

**Goal.** Prove the suites are strong (not just large), consolidate every
phase's checks into one battery with a dashboard, and run it green to exit
step 0. From here on, the battery runs permanently; Project-2 work lands only
on green.

**Work items.**

7.1. **Coverage in four dimensions, published per commit.** Extend the
existing coverage job (`rust.yml:156`, same invocation discipline): (a)
code — line/branch/function per crate; (b) VM — opcode/operand/exception-path
matrix from P5; (c) semantic — Proxy traps, accessors, private fields,
realms, modules, async jobs, WeakRef/finalization, `eval`, strict/sloppy,
IC shape transitions; (d) state-transition — shape A→B, GC-before/during
call, exception-through-frame, collection-during-iteration. All four are
non-decreasing gates; uncovered cells are test-writing items with owners.
7.2. **Mutation testing of the suites.** Run engine-aware mutants over the
mature suites: comparison-boundary flips, dropped prototype-chain steps,
skipped IC guards, removed GC barriers/roots (in scratch only, expecting
Miri/stress to catch), operand perturbations, swapped error types. Adopt a
kill threshold (e.g., ≥95% with zero unjustified survivors in choke-point
crates); every survivor gets a new killing test or a written justification
reviewed like an ignore entry. Mutation runs on a schedule (full) plus
per-PR on touched crates (sampled).
7.3. **Readiness battery + dashboard.** One command (and one scheduled CI
workflow) runs: full Test262 trend check, differential corpus, fuzz smoke
budgets, sanitizer smoke, Miri core subset, coverage deltas, mutation sample,
perf smoke vs `perf/baseline.json`, panic-trend check, ignore/unsafe/quarantine
lint checks. Results render as a single dashboard (pass/fail per check +
trend arrows); any red fails the run. The full (non-smoke) battery runs
nightly and on release candidates.
7.4. **Exit review + handoff contract.** Review the battery green on the
candidate commit against Success Criteria 1–10 with explicit sign-off per
criterion; record residual risks (what each method cannot show: Test262
incompleteness per TC39's own caveat, Miri single-execution, Kani bounds,
Loom conditionality, fuzzing's perpetual incompleteness). Downstream work
inherits: the pinned baseline, the battery (must stay green), the triage
queues (must stay empty), and the rule that the interpreter remains the
semantic reference all later tiers are differentially checked against.

#### P7 implementation notes (grounded in current code)

**Existing coverage job (extend, don't replace).** `coverage` job runs only on
main-targeted builds (`.github/workflows/rust.yml:160`), installs
cargo-tarpaulin (`.github/workflows/rust.yml:186-189`), runs
`cargo tarpaulin --workspace --features annex-b,intl_bundled,experimental --ignore-tests --engine llvm --out xml`
(`.github/workflows/rust.yml:192`) and uploads to Codecov via codecov-action v5
(`.github/workflows/rust.yml:194-195`). Thresholds live in
`.github/codecov.yml:4-10`: project gate with 5% variance allowance, patch
coverage off. P7.1 keeps this exact invocation discipline and adds dimensions
as new jobs/artifacts feeding the same dashboard.

**7.1 — Four coverage dimensions mapped to instrumentation points.**

- (a) Code (line/branch/function per crate): keep tarpaulin+llvm as the source
  of truth; tighten `.github/codecov.yml:8` (5% → smaller, then 0 + monotonic
  check) and turn `patch: off` (`.github/codecov.yml:10`) on for choke-point
  crates from P0. Note tarpaulin passes `--ignore-tests`
  (`.github/workflows/rust.yml:192`), so unit-test helper code is excluded by
  default — battery must compare per-crate deltas, not just the workspace
  aggregate. No new instrumentation code needed.
- (b) VM (opcode/operand/exception-path matrix): single choke point is
  `Context::execute_one` (`core/engine/src/vm/mod.rs:777-800`), which every
  opcode dispatch funnels through and which already branches per-feature exactly
  like the `fuzz` instruction-budget hook (`core/engine/src/vm/mod.rs:781-790`)
  and the `trace` hook (`core/engine/src/vm/mod.rs:792-797`). Add a parallel
  `vm-coverage` feature to `core/engine/Cargo.toml:14-68` (next to
  `fuzz`/`flowgraph`/`trace`) that records `(Opcode, operand-shape)` hits into
  a thread-local histogram dumped as JSON at context drop. Opcode universe is
  the `#[repr(u8)] pub(crate) enum Opcode` generated by `generate_opcodes!`
  (`core/engine/src/vm/opcode/mod.rs:350-367`, variants from
  `core/engine/src/vm/opcode/mod.rs:557`, ~196 total); operand kinds are
  `RegisterOperand`/`IndexOperand`/etc. (`core/engine/src/vm/opcode/mod.rs:234-333`).
  Exception-path cells hook `handle_error` (`core/engine/src/vm/mod.rs:803`)
  and the handler-search loop (`core/engine/src/vm/mod.rs:920-928`). Matrix =
  opcodes × operand-shapes × {normal, throw, handler-found}; uncovered cells
  become test-writing items. `flowgraph` (`core/engine/src/vm/flowgraph/`,
  feature `core/engine/Cargo.toml:65`) already walks instruction streams and is
  the reference for offline (static) cross-checks of the dynamic histogram.
  Ownership: P5.4 builds this matrix; P7.1b consumes it as a gate input —
  single implementation, no second histogram.
- (c) Semantic (traps/accessors/realms/modules/jobs/finalization/eval/
  strict/IC): two existing feeds. (1) Tester already emits per-run
  `features.json` (feature-name sets per commit,
  `tests/tester/src/results.rs:78-79,139-152`) — P7 promotes this from
  informational to a gated semantic matrix by diffing feature sets across runs.
  (2) IC shape-transition coverage instruments `InlineCache`
  (`core/engine/src/vm/inline_cache/mod.rs:29-39`: `entries: ArrayVec<CacheEntry, PIC_CAPACITY>`
  with `PIC_CAPACITY = 4`, `core/engine/src/vm/inline_cache/mod.rs:16`, plus
  `megamorphic` flag) — record transitions empty→mono→poly(2..4)→megamorphic
  under the same `vm-coverage` feature. Bytecode-shape regressions are already
  caught by insta snapshots: `glob!("../scripts/", "**/*.js")` +
  `script.codeblock().to_string()` + `assert_snapshot!`
  (`tests/insta-bytecode/src/lib.rs:1-13`), rendered by `Display for CodeBlock`
  (`core/engine/src/vm/code_block.rs:937-938`), enforced in CI via
  `cargo insta test -p insta-bytecode` (`.github/workflows/rust.yml:262-263`);
  P7 adds snapshot scripts per uncovered semantic cell rather than new tooling.
- (d) State-transition (shape A→B, GC-before/during call,
  exception-through-frame, collection-during-iteration): reuse (b)'s histogram
  infra with transition keys. Shape transitions: hook where `WeakShape`/`Slot`
  entries are added in `inline_cache/mod.rs` (cf. `CacheEntry`,
  `core/engine/src/vm/inline_cache/mod.rs:20-25`). Exception-through-frame:
  `handle_error`'s frame-pop loop (`core/engine/src/vm/mod.rs:920-928`).
  GC-timing transitions need P4/P6 stress-GC hooks; P7 only defines the
  transition-key schema and the non-decreasing gate, landing real probes where
  P6's GC work exposes them. Gotcha: all four dimensions must be recorded in
  one Test262 run where possible — full runs are ~hour-scale, so per-dimension
  re-runs multiply CI cost. (Open design point for implementation: the exact
  single-run multi-dimension recording design — feature unification of
  `vm-coverage` with the normal Test262 run — is intentionally left to the
  implementer with the cost constraint above.)

**7.2 — Mutation approach recommendation.** Use `cargo-mutants` (config-file
driven, workspace-aware, `--in-place` scratch runs; no repo changes needed to
pilot) for generic Rust mutants on touched crates per-PR, plus a small
Boa-specific mutant set implemented as source transforms or `cfg` shims,
because generic operator coverage misses engine semantics. Engine-specific
operators mapped to code: comparison-boundary flips in
`core/engine/src/vm/opcode/binary_ops/`; dropped prototype-chain steps in
property lookup (`GetPropertyByName` family under `core/engine/src/vm/opcode/get/`);
skipped IC guards by forcing `megamorphic = true`
(`core/engine/src/vm/inline_cache/mod.rs:36-38`) or empty `entries`; removed
GC roots/barriers in scratch only (expect Miri/sanitizer/stress to catch —
Miri gate is `cargo miri test --workspace --exclude boa_cli --exclude boa_examples miri`,
`.github/workflows/rust.yml:348-349`); operand perturbation at
`RegisterOperand`/`IndexOperand` construction
(`core/engine/src/vm/opcode/mod.rs:237-243,285-291`); swapped `JsNativeErrorKind`
variants at builtin error sites. Schedule: full mutation nightly (kill
threshold ≥95%, zero unjustified survivors in choke-point crates), sampled
per-PR on touched crates. Every survivor → new killing test or written
justification reviewed like an ignore entry. Gotcha: mutation runs must build
with the P0-pinned toolchain or mutant diffs drift; never mutate the `fuzz`
or `trace` shims themselves (self-defeating oracles).

**7.3 — Battery composition (reuse existing workflows, add one scheduler).**
One command + one scheduled workflow fans out to: (1) Test262 trend —
`cargo run --release --bin boa_tester -- run -v -o <out>`
(`.github/workflows/test262.yml:47`) then `boa_tester compare base new -m`
(`.github/workflows/test262.yml:62`) against
`boa-dev/data` `test262/refs/heads/main/latest.json`
(`.github/workflows/test262.yml:36-41,55`); main-branch results are pushed
back to `boa-dev/data` by `.github/workflows/test262_release.yml:48-69`.
(2) Differential corpus (P3 runner). (3) Fuzz smoke: `cd tests/fuzz &&
cargo fuzz build -s none --dev` exists (`.github/workflows/rust.yml:384-385`);
battery upgrades build-only to budgeted `run` + `cmin`/`tmin`/`coverage`.
(4) Sanitizer smoke (P6 flags). (5) Miri core subset (broaden
`.github/workflows/rust.yml:349` filter). (6) Coverage deltas (tarpaulin XML +
(b)/(c)/(d) JSON). (7) Mutation sample. (8) Perf smoke: criterion benches
(`criterion 0.8.2`, root `Cargo.toml:135`; `[[bench]] harness = false`,
`benches/Cargo.toml:22-24`) walk `benches/scripts/**/*.js`
(`benches/benches/scripts.rs:58-68`, `_`-prefixed files excluded,
`benches/benches/scripts.rs:64-66`), parse+compile+evaluate once and extract
`main` (`benches/benches/scripts.rs:22-48`), then `b.iter` on
`function.call` (`benches/benches/scripts.rs:94-101`); v8-benches get
`sample_size(10)` + 5 s measurement (`benches/benches/scripts.rs:77-80`).
Compare against `perf/baseline.json` (P0) with criterion's built-in
statistical comparison; the legacy `combined.js` path
(`cargo run --bin boa --release -- $BOA_DATA_ROOT/bench/bench-v8/combined.js`,
`Makefile.toml:29-32`) stays as a smoke fallback, not the gate.
`docs/profiling.md:13` confirms flamegraph/Valgrind need no feature flags —
diagnosis tooling, not gate inputs. (9) Panic-trend: `Statistics { total,
passed, ignored, panic }` (`tests/tester/src/main.rs:547-555`) and
`TestOutcomeResult::{Passed,Ignored,Failed,Panic}`
(`tests/tester/src/main.rs:795-804`) already counted; `compare_results`
(`tests/tester/src/results.rs:172-412`) reports fixed/broken/new-panics
(`ResultDiff`, `tests/tester/src/results.rs:416-421`) — battery fails on any
new panic/crash/timeout signature. (10) Config lints (ignore/unsafe/
quarantine, from P1/P6).

**7.4 — Exit review mechanics (concrete procedure).** The review is a checklist
run against the battery dashboard on the exit-candidate commit: for each
Success Criterion 1–10, record verdict + linked evidence artifact (e.g. C2 →
pinned `latest.json` + ignore audit report; C4 → differential queue at zero
with verdict log; C6 → inventory at 100% justified + Miri/sanitizer logs;
full mapping in the review template at `docs/step0-exit.md`). Residual risks
go in a register with fixed schema per row: `method | known blind spot |
compensating control | owner` (e.g. Test262 incompleteness per TC39's caveat →
compensated by differential + fuzz + metamorphic; Miri single-execution →
multi-seed + sanitizers; Kani bounds → documented per harness). Sign-off is
per-criterion, not blanket; any red criterion reopens its owning phase
instead of proceeding. The handoff contract (pinned baseline, battery stays
green, queues stay empty, interpreter-as-reference rule) is recorded in the
same document and acknowledged by whoever starts downstream work.

**Dashboard data sources (all already produced; P7 only renders).**
`latest.json` (full per-test verdicts + `c`/`u` commits,
`tests/tester/src/results.rs:14-22,73,107-118`), `results.json` (append-only
`ReducedResultInfo` history — free trend arrows,
`tests/tester/src/results.rs:76,120-133`), `features.json` (semantic-feature
history, `tests/tester/src/results.rs:79,139-152`) — all written per run by
`write_json` (`tests/tester/src/results.rs:84-159`) into branch-scoped dirs
(`pull` vs `refs/heads/main`, `tests/tester/src/results.rs:90-101`) and
published via the `test262-results` artifact
(`.github/workflows/test262.yml:79-83`) and the `boa-dev/data` repo. PR
commenting pattern to copy: `workflow_run`-triggered follow-up job
(`.github/workflows/test262_comment.yml:3-7`) that downloads the artifact and
upserts one comment keyed by `<!-- test262-compliance-report -->`
(`.github/workflows/test262_comment.yml:33-45`) — the battery dashboard reuses
this key-comment mechanism with one row per battery check. Gotchas:
`compare` is currently advisory (exit code always success,
`tests/tester/src/results.rs:411`) — gating consumes the strict flag P1.5 adds
(P1 owns `compare` strictness; P7 adds no second flag);
`get_test262_commit` reads `.git/refs/heads/main`
(`tests/tester/src/results.rs:162-168`), which breaks under worktrees/
detached checkouts and must be hardened in P0 pinning first.

**Validation / gate.**

- Battery drill: revert one fixed bug per detection layer (Test262-only,
  differential-only, fuzz-only, Miri/sanitizer-only, mutation-only) → the
  battery goes red naming the right layer for all five; restore → green.
- Dashboard shows 30 consecutive scheduled runs green with zero untriaged
  items across queues (triage, crashes, mutants, ignores, quarantine).
- **Exit criteria (step 0 done):** Success Criteria 1–10 each signed off with
  linked evidence; battery green on the exit commit; residual-risk register
  published; Project-2 may begin.
- 7.0 recon CLOSED (2026-10-04): exit criteria, pins, and battery
  artifacts inventoried (tester `latest.json`/`results.json`/
  `features.json`, differential queue, fuzz battery/matrix, benches,
  lints, codecov, VM matrix). `codecov.yml` tightened (project 5%->1%
  + patch 80% choke; target needs calibration against engine ~62%).
- 7.1 four coverage dimensions CLOSED as non-decreasing gates
  (2026-10-04). Code: tarpaulin split (small crates, then engine with
  `--timeout 600/1200`) + union-merge by covered lines is canonical
  (single 4-crate run flakes `Timed out waiting for test response`);
  baseline `target/tarpaulin-merged.xml` (36585/55992) emitted to
  `docs/coverage-baseline.json` (engine 0.6228 24564/39444, gc 0.7761,
  interner 0.8913, string 0.7002); `scripts/check-coverage-delta.py`
  gains `--merge` + `--emit-baseline` + per-crate delta gate (self
  GREEN, evil RED proven). VM: `scripts/check-vm-matrix.sh` GREEN
  (RATCHET PASS 259 cells, 93 justifications; evil RATCHET FAIL exit 1
  proven). Semantic: `scripts/check-semantic-features.py` +
  `docs/semantic-features-baseline.json` (198 features, P0.2 run-1
  verbatim, corpus-derived drift detector, outcome-independent); self
  GREEN on run-1/run-2/fresh full Test262, evil RED proven; fresh full
  suite `target/test262-fresh/` T262_EXIT=0. State: `coverage_ic_
  transition` (`core/engine/src/vm/coverage.rs`) hooked in
  `InlineCache::set` (`vm/inline_cache/mod.rs`, transition-only —
  hits/mega no-ops bump nothing), `ic_transitions_emit_cells` 10
  passed; probe `tests/fuzz/probes/ic-transitions.js` walks
  mono->poly2/3/4->mega and is load-bearing for those rungs (ratchet
  dynamic=2 = probe plain+opt runs; to-mono 18586 overwhelmingly
  battery-driven, probe's extra to-mono accepted as steady mono
  noise); ratchet regen 259->264 rows, RATCHET PASS (264 cells, 93
  justifications), EXIT=0. `regen.py` self-verify cwd bug fixed
  (`cwd=MATRIX.parent`, argv resolved absolute; self-verify failed
  whenever regen ran outside `tests/fuzz`) — REGEN_EXIT=0.
- 7.2 mutation INTERIM (2026-10-04; stage-2 verdict pending, see addendum).
  Tool: cargo-mutants 27.1.0, config `.cargo/mutants.toml` (scope `core/*`
  product only, oracles excluded, `gitignore=true` mandatory — without it the
  scratch copy eats the 37G ignored `tests/fuzz/target` and dies ENOSPC).
  Discovery gaps found by `--list-files` vs attended sources: `cfg_if!`
  hides `value/inner/{nan_boxed,legacy}.rs` + `sys/{fallback,js}/mod.rs`
  (nan_boxed is default-active choke-point code!); module-name convention
  hides default-active `runtime/fetch/tests/mod.rs` (TestFetcher compiles
  into the product); plus orphan dead file
  `object/builtins/jsfinalization_registry.rs` (108 lines, never
  mod-included, stale TypedArray copy-paste — upstream finding) and a dead
  `cfg(feature="float16")` row in `string/common.rs` (feature undeclared).
  Guard: `scripts/check-mutation-filter.sh` (GREEN, evil RED proven) —
  textual `mod`-chain ascent for `#[cfg(test)]` (handles `cfg(all(test,…))`
  too), allowlist ORPHAN/COMPENSATED/VACUOUS/NON-DEFAULT, each machine-
  checked (VACUOUS re-proves zero-mutant generation via `--Zmutate-file`).
  Pilot `boa_string` (460 mutants): 185 caught, 172 missed, 103 unviable
  (Default-less-type tool noise), 0 timeout; baseline green. Methodology
  finding: leaf-crate behavior is tested upstream — `is_empty→true` missed
  by the 25 crate tests but kills 15 engine string tests (drill-proven) —
  hence TWO-STAGE verdicts (stage 1 package suites, stage 2 survivors vs
  wider suites, exact-match `-F` filter incl. descriptions since same-span
  mixed outcomes exist). Stage-2 recipe learned hard: first attempt OOM-
  killed the 15GB box (kernel oom-kill 17:34, 4-way engine builds); measured
  engine suite 100s/1.9GB at 2 threads → throttle `-j2` +
  `RUST_TEST_THREADS=2` (CLI passthrough impossible: mutants appends test
  args without `--`, baseline exit 4 proven). Contains correction: an early
  "zero callers" claim was wrong (narrow grep) — regexp/string builtins call
  `contains` for flag checks; the probe mutant HANGS (`match_with_overridden
  _exec` + 8 fast failures) → timeouts are kills after triage. Durable
  collateral: `contains_byte` unit test (Latin1+Utf16 × present/absent)
  kills 4/4 retested mutants. Boa-specific set `docs/boa-mutants.md`: 9/9
  PROVEN killed via `scripts/run-boa-mutants.sh` (BOA_EXIT=0, zero residue),
  incl. folding-bypass lesson (CMP killer must use variable comparisons;
  `abstract_relational_comparison` all-literal stays green), IC-2 mono-site
  degradation analysis, IC-1 correction (unit tests kill it too — coverage
  kill is a second layer, not exclusive), GC-1 deterministic SIGABRT 3/3,
  NAN-1 Kani RED naming the property. `scripts/run-mutation.sh`: pinned-
  toolchain guard, resource throttle, exact stage-2 filter, supersede-merge
  (later wins by name; verified 460+1→460 with missed→timeout move),
  threshold gate (rate + machine-checked `JUSTIFIED` lines in
  `docs/mutation-survivors.md`; timeouts count killed only when triaged).
  Counts: engine 16579, parser 1992, gc 479, string 460, interner 98 →
  honest schedule (nightly `mutation_*` ×4 + per-PR `mutation_sample`,
  YAML-valid): small crates full both stages nightly; parser 1/12, engine
  1/120 rotation (full engine pass ≈120 nights at -j2 — documented with
  scale-out path); PR in-diff (`--in-diff` takes a diff FILE, proven)
  gating at threshold 0 = full accounting, no rate bar on tiny samples.
  PR-mode simulated end-to-end (1/1 caught, PASS). Reserve-equivalence
  class justified (10 mutants, capacity hints). Gates: fmt clean, clippy
  zero-new, string 26/26, filter green, verify-baseline 1 pre-existing.

## Validation Plan (global command reference)

Every phase's gate ultimately reduces to these runnable checks (exact flags
for new tooling are fixed when built; existing commands below are verified
in-tree):

| Check | Command (from repo root, pinned toolchain) | Expected evidence |
|---|---|---|
| Unit + integration suites | `cargo test --workspace` | zero failures |
| Full Test262 run | `cargo run --release --bin boa_tester -- run -o <out>` | verdict file; counted outcomes, zero untriaged |
| No-regression verdict | `cargo run --release --bin boa_tester -- compare <base> <new>` | pass count monotonic; no new failures/skips/crashes |
| Regression DB | single documented command (`tests/regression/`) | all entries pass |
| Differential corpus | `tools/differential/run` (new in P3) | zero untriaged mismatches |
| Fuzz smoke (per PR) | `cd tests/fuzz && cargo fuzz run -s none -- -max_total_time=<budget> <target>` per target | zero new crashes; seeds pass |
| Fuzz long + instrumented | same with `-s address,undefined` on schedule | zero untriaged crashes; coverage non-decreasing |
| Corpus ops | `cargo fuzz cmin|tmin|coverage <target>` | minimized reproducers; merged corpus; coverage reports |
| Miri core | broadened `cargo miri test …` (extends `rust.yml:348-349`) | green on seed set |
| Sanitizer tests | `RUSTFLAGS="-Zsanitizer=address,undefined" cargo test …` (flags pinned in P0) | green |
| Kani kernels | `cargo kani -p <crate> --harness <name>` per harness | proven / bounded-pass with documented bounds |
| Loom (if in scope) | `RUSTFLAGS="--cfg loom" cargo test --release …` | green |
| Coverage | existing coverage job invocation (`rust.yml:156`), extended dimensions | four dimensions published, non-decreasing |
| Mutation | scheduled full + per-PR sampled runs | kill threshold met; survivors justified |
| Perf smoke | criterion benches + JS smoke set vs `perf/baseline.json` | within threshold or signed off |
| Config lints | ignore-list lint, unsafe-inventory check, quarantine check | zero unjustified entries |
| Full battery | one P7 command + scheduled workflow + dashboard | all green |

Cross-cutting drills that must each demonstrate red→green: seeded-regression
(P1), pipeline/normalizer fixtures (P3), seeded-crash + logic-revert (P4),
transform-soundness (P5), seeded-UB (P6), per-layer revert battery (P7).

## Risks / Open Questions

1. **Oracle availability.** P3 needs a headless oracle engine buildable/pinnable
   in CI. If none of the evaluated candidates (`d8`/`jsshell`/`jsc`) proves
   practical, fallback is Test262 + metamorphic + valid-only generation as the
   primary oracles, with differential added later. Decision due in P3.1 with
   evidence either way.
2. **Fuzz compute budget.** Nightly multi-hour multi-target fuzzing needs
   runners; undersized budgets produce false confidence. P4 must publish
   budgets + achieved coverage so the gate means something auditable.
3. **Kani scalability.** Real GC/value code may exceed bounded-checking
   capacity; harnesses may need narrowing (smaller bounds, extracted pure
   functions). Non-goals for narrowing are documented, never silent (P6.4).
4. **Test262 churn.** A pinned commit ages; re-pinning is a planned,
   reviewed event (re-baseline + re-triage deltas), never drive-by.
5. **Upstream vs fork.** Tester/harness/CI changes should go upstream where
   possible; fork-local scaffolding (differential runner, dashboards) stays
   clearly separated so rebases stay cheap. Decide per artifact in P1.
6. **Flaky-timing tests.** Async/GC-timing-sensitive tests may fight
   determinism gates; the quarantine protocol (P1.6) bounds the damage, and
   stress-mode determinism work (P4.4) shrinks the set over time.

## Sources

Exact URLs whose underlying content was inspected (non-empty) during this run:

- `https://github.com/boa-dev/boa/discussions/4487` — JIT prototype scope,
  opcode count, copy-and-patch, feedback-vector gap, GC benchmark share.
- `https://raw.githubusercontent.com/tc39/test262/main/README.md` — suite
  size (50,000+ files, May 2025) and TC39's incompleteness caveat.
- `https://raw.githubusercontent.com/googleprojectzero/fuzzilli/main/Docs/HowFuzzilliWorks.md` —
  FuzzIL generation, syntactic/semantic validity design, REPRL execution.
- `https://raw.githubusercontent.com/rust-lang/miri/master/README.md` — UB
  detection classes; single-execution limitation.
- `https://raw.githubusercontent.com/model-checking/kani/main/README.md` —
  bit-precise model checking; safety + correctness harnesses.
- `https://raw.githubusercontent.com/tokio-rs/loom/master/README.md` —
  concurrent-execution permutation; documented model gaps.
- `https://raw.githubusercontent.com/rust-fuzz/cargo-fuzz/main/README.md` —
  `run`/`tmin`/`cmin`/`coverage` workflow.
- `https://llvm.org/docs/LibFuzzer.html` — coverage-guided fuzzing engine.
- `https://clang.llvm.org/docs/AddressSanitizer.html` — memory-error
  instrumentation reference.
- `https://clang.llvm.org/docs/UndefinedBehaviorSanitizer.html` — UB
  instrumentation reference.
- `https://web-platform-tests.org/` — cross-implementation confidence purpose.
- `https://webkit.org/testing/` — test-with-fix practice.
- `https://doc.rust-lang.org/reference/behavior-considered-undefined.html` —
  `unsafe` soundness obligation.
- `https://v8.dev/docs` — V8 background (C++ engine, generational GC,
  embedder-provided DOM).

In-repo evidence — read this session: `Cargo.toml`, `README.md`,
`CONTRIBUTING.md`, `core/gc/src/lib.rs`, `core/engine/src/value/inner.rs`,
`core/wintertc/src/lib.rs`, `tests/fuzz/README.md`,
`.github/workflows/rust.yml`, `.cargo/config.toml`; listed this session:
`core/runtime/src/*`, `core/wintertc/src/*`, `tests/fuzz/fuzz_targets/*`;
verified against file bodies by the research workflow: `CHANGELOG.md`,
`test262_config.toml`, `tests/tester`, `docs/vm.md`,
`core/engine/src/{builtins,bytecompiler,vm,codeblock,context}`,
`.github/workflows/test262.yml`.

## Appendix: Repository coding standards (follow in all step-0 work)

All new and modified code in this program must match the repo's existing
conventions below. They were extracted from configs, `CONTRIBUTING.md`, CI,
and sampled source; paths are evidence, not decoration.

### Formatting and tooling

- **rustfmt with default settings** (no `rustfmt.toml` exists): 4-space
  indent, `max_width = 100`, LF endings, UTF-8, trimmed trailing whitespace,
  final newline (`.editorconfig`). JS/JSON/Markdown use 2-space indent;
  Makefiles use tabs.
- Before pushing, run the same gate CI runs: `cargo make run-ci`
  (`Makefile.toml` + `make/ci.toml` + `.husky/pre-push`): `cargo fmt -- --check`,
  `cargo clippy --all-features --all-targets`, and
  `cargo clippy --no-default-features` — all with warnings denied
  (`-D warnings`, so **zero warnings is the standard**).
- Non-Rust files go through Prettier (`npx prettier -w .`, see `package.json`
  / `.prettierignore`); prose and identifiers go through the `typos`
  spellchecker (`typos.toml` carries the project word list — extend it instead
  of working around it).
- Tests run under `cargo test` / nextest with `fail-fast = false` in CI
  (`.config/nextest.toml`): the whole suite always runs; never rely on early
  abort to hide failures.

### Lints (workspace-wide, `Cargo.toml` `[workspace.lints]` + `clippy.toml`)

- rustc lint groups at `warn` plus `missing_docs`,
  `missing_debug_implementations`, `missing_copy_implementations`,
  `unreachable_pub`, `unused_qualifications`, and more; rustdoc lints
  (`broken_intra_doc_links`, etc.); clippy `all/correctness/suspicious/style/
  complexity/perf/pedantic` at `warn`, plus `dbg_macro`, `print_stdout`,
  `print_stderr` (printing is allowed in tests only:
  `allow-print-in-tests = true`).
- Every crate sets `#![cfg_attr(not(test), forbid(clippy::unwrap_used))]`
  (e.g. `core/gc/src/lib.rs:11`): **no `unwrap`/`expect` in non-test code**.
- `clippy.toml` bans allocating `str` case/replace methods — always use the
  `cow_utils::CowUtils` non-allocating equivalents (`cow_to_ascii_lowercase`,
  `cow_replace`, …); doc comments may use the spellchecked idents
  `ECMAScript`, `JavaScript`, `SpiderMonkey`, `GitHub`.

### Naming

- Standard Rust casing everywhere: `UpperCamelCase` types/traits/enums,
  `snake_case` functions/modules, `UPPER_SNAKE_CASE` consts/statics.
- Engine wrapper types take the **`Js` prefix**: `JsValue`, `JsObject`,
  `JsString`, `JsError`, `JsArray`, `JsBigInt`, `JsArrayBuffer`, …
  Fallible engine code returns `JsResult<T> = Result<T, JsError>`
  (`core/engine/src/lib.rs:152`); native errors use `JsNativeError` /
  `JsNativeErrorKind`.
- Spec mapping is the naming authority (`CONTRIBUTING.md:136-175`): abstract
  operations map to same-name Rust functions (`IsCallable` → `is_callable`);
  internal slots (`[[Prototype]]`) map to private fields; spec `?` maps to the
  `?` operator; spec `!` (infallible) maps to `expect`-style handling via
  `JsExpect`/`js_expect` (which converts would-be panics into
  `EngineError::Panic`), never to bare `unwrap`.
- Opcodes are PascalCase verbs: `Var`, `InitVar`, `SetName`,
  `GetPropertyByName`, … (`core/engine/src/vm/opcode/`).
- Builtins follow one shape per object (`core/engine/src/builtins/array/
  mod.rs` is the template): a struct (`Array`), `pub(crate) fn` per method,
  registration in `fn init(realm: &Realm)` with the builder chain
  (`.static_method(Self::from, js_string!("from"), 1)`,
  `.static_accessor`, …), `JsArgs` for arguments, `js_string!`/`js_str!`/
  `js_value!` macros for literals.
- GC types derive together: `#[derive(Debug, Clone, Trace, Finalize)]` (plus
  `JsData` for engine user data); handles are `Gc<T>` / `GcRefCell` /
  `JsObject` wrappers, never raw pointers in safe code.
- Modules are `snake_case`; each builtin keeps its tests in a `tests.rs`
  sibling wired as `mod tests;`. Feature flags are lowercase/kebab-case.

### Comments and documentation

- **Every public item is documented** (`missing_docs` is deny-by-CI): crate
  and module docs use `//!`; each builtin module header carries `[spec]:` and
  `[mdn]:` link definitions (e.g. `core/engine/src/builtins/array/mod.rs:1-9`).
  Public fallible APIs document `# Errors`; panicking, unsafe, and exampled
  APIs use `# Panics`, `# Safety`, `# Examples` respectively.
- **Spec-step comments are mandatory in engine code** (`CONTRIBUTING.md:167-175`):
  quote the algorithm steps as numbered comments (`// 2. Let proto be ? ...`,
  `// 4.a. Return ! ArrayCreate(0, proto).`); where Rust or performance forces
  divergence, add a note explaining the difference instead of silently
  deviating.
- **Every `unsafe` block is preceded by a `// SAFETY:` comment** stating the
  upheld invariant (verified pattern, e.g.
  `core/engine/src/builtins/number/conversions.rs:85`); P6 extends this to a
  machine-checked inventory, but the comment convention already exists — follow
  it from day one.
- Prefer intra-doc links (`` [`JsValue`] ``); keep `TODO`/`FIXME`/`NOTE`
  markers rare, specific, and owned (the plan's quarantine rule applies the
  same discipline to tests).

### Tests

- Unit tests live next to the code (`mod tests` inline or `tests.rs` sibling
  under `#[cfg(test)]`).
- JS-behavior tests use the shared harness from the crate root:
  `run_test_actions([TestAction::run_harness(), TestAction::run("…"),
  TestAction::assert("…"), TestAction::assert_eq("…", …), …])`
  (`core/engine/src/lib.rs:360`, `core/engine/src/builtins/array/tests.rs`),
  with `indoc!` for multi-line JS. New behavioral tests must use this harness
  rather than inventing ad-hoc contexts.
- `rstest`/`test-case` exist for parametrized Rust-level tests
  (e.g. `core/runtime/tests/clone.rs`); bytecode snapshots use `cargo-insta`
  (`tests/insta-bytecode`, `Makefile.toml` `insta-test`/`insta-review` tasks).
- New tests follow the fix-loop discipline (Approach, decision 6): minimal JS
  reproducer + Rust unit test at the responsible layer + Test262-style test
  where applicable + fuzz seed, all citing the spec section.

### Commits and review

- Conventional commits with scope, lowercase subject, PR reference:
  `fix(engine): … (#5514)`, `feat(engine): … (#5503)`,
  `chore(deps): …`, `refactor: …` (see `git log`). Claim issues before working
  them; all changes land via reviewed PRs (`CONTRIBUTING.md:6-16`).
- New warnings, new `unsafe` without `SAFETY`, new Test262 ignores without
  justification, and behaviour changes without spec citations do not land —
  the P1/P6 lints enforce mechanically what review enforces socially.
