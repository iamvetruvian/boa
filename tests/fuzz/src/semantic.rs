//! In-fuzzer semantic oracles (P4.2).
//!
//! Crash-only fuzzing cannot see logic bugs. These oracles compare
//! independently-obtained outcomes of the same generated program:
//!
//! - [`eval_outcome`] + [`outcomes_equal`]: determinism double-eval and
//!   print/parse idempotency, always on. Any disagreement is a real
//!   engine bug (generated programs are deterministic: only `a..=h`
//!   identifiers, no I/O, no time, no host APIs).
//! - [`eval_outcome_stressed`] + [`compile_outcome_stressed`]: normal-vs-
//!   GC-stress differentials (P4.4). The second leg runs under
//!   collect-every-N allocations; any divergence from the normal leg is a
//!   missing GC root or a collection-timing assumption. Timing-observable
//!   programs and allocation-churn outliers skip it ([`should_stress`])
//!   instead of asserting.
//! - [`eval_outcome_unoptimized`]: optimizer on/off differential (P5.2).
//!   Fresh contexts optimize by default; this leg disables all passes and
//!   any divergence from the normal leg is an optimizer miscompile —
//!   except the filed DCE completion leak, which
//!   [`is_filed_dce_completion_leak`] skips so the loop reaches new bugs.
//! - [`metamorphic_variants`]: P5.3 semantics-preserving transforms (one
//!   [`Variant`] per firing transform, each asserted evaluation-identical).
//! - [`eval_oracle`] + [`oracle_agrees`]: env-gated differential against
//!   the pinned P3 oracle (`BOA_FUZZ_ORACLE=<script-goal jsshell path>`).
//!   Conservative by design: completion-class only (completed vs threw +
//!   error class), skipping fuel-exhaustion and oracle timeouts, since
//!   neither side can classify non-termination cheaply. The 500-program
//!   P4.2 probe measured 98.8% agreement with zero false positives (its 3
//!   mismatches were all filed Boa bugs), so assertion mode is viable.
//! - [`stress_point`]: forced collection at a hostile point (pre-eval, live
//!   roots held), called on every fuzz-built context.
//!
//! Two deliberate flood-control boundaries: object/function values compare
//! by `typeof` tag only (P3 differential owns shape comparison on the
//! Test262 corpus), and thrown *values* compare by marker (both sides threw
//! a non-error), not content.

use boa_engine::{Context, JsError, JsNativeErrorKind, JsValue, Source};
use boa_gc::{force_collect, gc_total_allocs, StressGuard};
use std::{
    io::Cursor,
    num::NonZeroU64,
    process::{Command, Stdio},
    thread::sleep,
    time::Duration,
};

/// Instruction fuel per eval, matching `vm-implied`'s historical budget.
pub const EVAL_FUEL: usize = 1 << 16;

/// Seconds before an oracle child counts as timed out (skip, not signal).
pub const ORACLE_TIMEOUT_SECS: u64 = 5;

/// Max rendered characters kept for strings/bigints (join-bombs cap).
pub const RENDER_CAP: usize = 1024;

/// Env var enabling oracle-differential mode (script-goal shell path).
pub const ORACLE_ENV_VAR: &str = "BOA_FUZZ_ORACLE";

/// Default GC-stress cadence: collect every this many allocations.
///
/// Calibrated so one stressed eval runs dozens of collections while staying
/// within ~2x of normal-eval wall time; see the P4.4 as-built notes.
pub const GC_STRESS_EVERY: u64 = 100;

/// Env var overriding [`GC_STRESS_EVERY`] (nonzero integer, else the default).
pub const GC_STRESS_ENV_VAR: &str = "BOA_FUZZ_GC_STRESS_EVERY";

/// Allocation budget for the stressed leg.
///
/// Inputs whose normal leg serves more than this many allocations skip the
/// stressed leg (keeping the determinism, crash, idempotency, and oracle
/// legs). Stressed collection cost is superlinear in churn — a fuel-
/// saturated `while (import(this)) {}` burns 132k allocations and runs 38x
/// slower stressed (12s vs 0.3s) — so uncapped stress would let rare
/// pathological mutants starve the fleet's time budget. Typical generated
/// inputs churn hundreds of allocations; the cap only bites the extreme
/// tail. Deterministic: allocation counts don't vary by machine.
pub const STRESS_MAX_ALLOCS: u64 = 100_000;

/// Maximum `(`/`[`/`{` nesting a *derived* input (metamorphic variant)
/// may carry. Originals are never dropped (they are corpus signal); mutants
/// are parse-gated on the same thread they evaluate on, so the red-zone
/// cannot split them from themselves.
///
/// Background: the parser enforces a 256 KB stack red-zone (bug-#22
/// hardening), so accepted depth adapts to remaining stack — measured ~10
/// levels on a 2 MB test thread, ~212 on the 8 MB main thread. A derived
/// input that straddles the red-zone trips a catchable `SyntaxError` its
/// original does not, which reads as a false divergence (P5.3 gate
/// finding: 32/200 pilots before this cap). Harness legs run on 64 MB
/// worker threads (cliff projected past ~1700), so 256 keeps every
/// comparison far below the red-zone on every thread, with margin to
/// spare; templates nest ≤ ~12 and corpus shapes ≤ ~100, so the cap only
/// binds the absurd tail.
///
/// Residual: a corpus input sitting within ±1 of the 64 MB cliff (~1700)
/// could still split a ±1-depth mutant from itself. Probability ~0, and
/// the triage signature is unmistakable (`Maximum call stack size
/// exceeded` on a thousand-deep input): artifact, not bug.
pub const MAX_DERIVED_NESTING: usize = 256;

/// Conservative textual nesting depth: max simultaneous open `(`/`[`/`{`.
///
/// Strings, comments, and regex literals are deliberately NOT skipped:
/// over-counting only drops derived inputs (never originals), so the bias
/// is toward skipping — the safe direction for a resource-bound filter.
#[must_use]
pub fn max_nesting_depth(source: &str) -> usize {
    let mut depth = 0usize;
    let mut max = 0usize;
    for ch in source.chars() {
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                max = max.max(depth);
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    max
}

/// The comparable outcome of one eval: completion class + primitive value.
///
/// `EngineError::Panic` never appears here: internal panics are crash-class
/// and [`eval_outcome`] re-panics on them so libFuzzer records a crash
/// instead of silently discarding the `JsResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalOutcome {
    /// `true` when the program completed (returned a value).
    pub completed: bool,
    /// Native error class (`SyntaxError`, …), `thrown-value` for a thrown
    /// non-error, `FuelExhausted` / `RuntimeLimit` for engine limits.
    /// `None` when [`EvalOutcome::completed`].
    pub error_class: Option<String>,
    /// Canonical primitive rendering (typeof tag for objects/functions).
    /// Empty unless completed.
    pub primitive: String,
}

/// Evaluate `source` in a fresh default context under [`EVAL_FUEL].
///
/// Fresh (not shared) interners keep the two determinism legs independent;
/// names resolve identically within each run, so outcomes stay comparable.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic` (an internal engine panic surfaced as
/// `JsError`): swallowing it would blind crash detection.
pub fn eval_outcome(source: &str) -> EvalOutcome {
    let mut context = Context::builder()
        .instructions_remaining(EVAL_FUEL)
        .build()
        .expect("fuzz context must build");
    eval_outcome_in(&mut context, source)
}

/// Evaluate `source` in a caller-provided context (P4.3 host contexts).
///
/// The caller owns fuel configuration; [`stress_point`] still runs so GC
/// stress (P4.4) covers host-boundary evals too.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic`, like [`eval_outcome`].
pub fn eval_outcome_in(context: &mut Context, source: &str) -> EvalOutcome {
    stress_point(context);
    match context.eval(Source::from_reader(Cursor::new(source), None)) {
        Ok(value) => EvalOutcome {
            completed: true,
            error_class: None,
            primitive: render_value(&value),
        },
        Err(err) => classify_error(&err),
    }
}

/// Compare two outcomes for the determinism/idempotency oracles.
#[must_use]
pub fn outcomes_equal(first: &EvalOutcome, second: &EvalOutcome) -> bool {
    first == second
}

/// Render a value without invoking user code (no `to_string` coercions).
fn render_value(value: &JsValue) -> String {
    if value.is_undefined() {
        return String::from("undefined");
    }
    if value.is_null() {
        return String::from("null");
    }
    if let Some(boolean) = value.as_boolean() {
        return boolean.to_string();
    }
    if let Some(number) = value.as_number() {
        if number.is_nan() {
            return String::from("NaN");
        }
        // Rust `Debug` for `f64` is shortest-round-trip (`-0.0` vs `0.0`
        // stay distinct), which is exactly the precision needed.
        return format!("{number:?}");
    }
    if let Some(string) = value.as_string() {
        let full = string.to_std_string_escaped();
        return capped("string", full.len(), &full);
    }
    if let Some(bigint) = value.as_bigint() {
        let full = bigint.to_string_radix(10);
        return capped("bigint", full.len(), &full);
    }
    // Objects, functions, symbols: typeof tag only (see module docs).
    format!("typeof:{}", value.type_of())
}

/// Length-prefixed, capped rendering for arbitrarily large payloads.
fn capped(kind: &str, len: usize, text: &str) -> String {
    if text.len() <= RENDER_CAP {
        return format!("{kind}[{len}]:{text}");
    }
    // Truncate on a char boundary; the length prefix keeps it injective
    // enough for equality checks while bounding memory.
    let mut end = RENDER_CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{kind}[{len}]:{}…", &text[..end])
}

/// Classify a failed eval into an [`EvalOutcome`].
fn classify_error(err: &JsError) -> EvalOutcome {
    let class = error_class_name(err);
    let primitive = if class == "thrown-value" {
        err.as_opaque().map(render_value).unwrap_or_default()
    } else {
        String::new()
    };
    EvalOutcome {
        completed: false,
        error_class: Some(class),
        primitive,
    }
}

/// The stable class string of a [`JsError`] (P4.3 host-boundary oracles).
///
/// Same mapping [`classify_error`] uses, without value rendering: native
/// kinds keep their ECMAScript names, thrown non-errors become
/// `thrown-value`, engine limits become `FuelExhausted` / `RuntimeLimit`.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic` so libFuzzer records a crash.
#[must_use]
pub fn error_class_name(err: &JsError) -> String {
    if let Some(engine) = err.as_engine() {
        let class = format!("{engine:?}");
        if class.contains("NoInstructionsRemain") {
            return String::from("FuelExhausted");
        }
        if class.contains("Panic") {
            panic!("fuzz target: engine panicked: {err}");
        }
        return String::from("RuntimeLimit");
    }
    if let Some(native) = err.as_native() {
        return native_class_name(native.kind());
    }
    if err.as_opaque().is_some() {
        return String::from("thrown-value");
    }
    String::from("Unknown")
}

/// Map a native error kind to its ECMAScript class name.
fn native_class_name(kind: &JsNativeErrorKind) -> String {
    kind.to_string()
}

/// Metamorphic program variants that must all evaluate identically.
///
/// Real P5.3 semantics-preserving transforms (see [`crate::metamorphic`]):
/// one [`Variant`] per firing transform, each of which the harness asserts
/// evaluates identically to the original.
pub use crate::metamorphic::{metamorphic_variants, Variant};

/// GC-stress injection point (P4.4).
///
/// Called on every fuzz-built context before eval. Forces a collection at a
/// hostile point — live roots (the realm, the about-to-run script's
/// compilation artifacts) are held while unreachable garbage from context
/// construction is swept — so missing roots surface on every input instead
/// of only when the 1 MB threshold happens to trip mid-eval. The every-N
/// cadence inside stressed legs ([`StressGuard`]) covers in-eval points
/// (calls, property access, unwind); this covers the boundary.
pub fn stress_point(_context: &mut Context) {
    force_collect();
}

/// The effective stress cadence: [`GC_STRESS_ENV_VAR`] when it parses as a
/// nonzero integer, else [`GC_STRESS_EVERY`].
#[must_use]
pub fn stress_cadence() -> NonZeroU64 {
    std::env::var(GC_STRESS_ENV_VAR)
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .and_then(NonZeroU64::new)
        .unwrap_or_else(|| NonZeroU64::new(GC_STRESS_EVERY).expect("default cadence is nonzero"))
}

/// Evaluates `source` exactly like [`eval_outcome`], but under GC stress.
///
/// A [`StressGuard`] holds collect-every-[`stress_cadence`] for the whole
/// eval (fresh context, same fuel) and restores the previous setting on
/// return or panic. Pair with a normal leg and compare: any divergence is a
/// missing root or a collection-timing assumption — unless the program can
/// legitimately observe collection timing (see
/// [`gc_observes_collection_timing`]), in which case skip this leg.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic`, like [`eval_outcome`].
pub fn eval_outcome_stressed(source: &str) -> EvalOutcome {
    let _guard = StressGuard::stress(stress_cadence());
    eval_outcome(source)
}

/// Evaluates `source` in a caller-provided context under GC stress.
///
/// The [`eval_outcome_in`] twin of [`eval_outcome_stressed`]: the caller
/// owns the context (P4.3 host contexts) and its fuel configuration, while
/// this wrapper holds the [`StressGuard`] for the eval.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic`, like [`eval_outcome_in`].
pub fn eval_outcome_stressed_in(context: &mut Context, source: &str) -> EvalOutcome {
    let _guard = StressGuard::stress(stress_cadence());
    eval_outcome_in(context, source)
}

/// Evaluates `source` exactly like [`eval_outcome`], but with the optimizer
/// disabled (P5.2 configuration differential).
///
/// Fresh contexts optimize by default (`OptimizerOptions::OPTIMIZE_ALL`),
/// so this is the unoptimized twin: any divergence from the normal leg is an
/// optimizer miscompile (constant folding, strength reduction, or dead code
/// elimination changing observable behavior). No timing filter applies —
/// optimization is deterministic — but the leg only runs on parseable
/// inputs (the caller gates), since parse failures agree trivially.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic`, like [`eval_outcome`].
pub fn eval_outcome_unoptimized(source: &str) -> EvalOutcome {
    eval_outcome_with_optimizer(source, boa_engine::optimizer::OptimizerOptions::empty())
}

/// Evaluates `source` exactly like [`eval_outcome`], but with `options` as
/// the optimizer configuration (P5.3 filed-bug fingerprinting).
///
/// [`eval_outcome`] is this with `OPTIMIZE_ALL`;
/// [`eval_outcome_unoptimized`] is this with `empty()`.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic`, like [`eval_outcome`].
pub fn eval_outcome_with_optimizer(
    source: &str,
    options: boa_engine::optimizer::OptimizerOptions,
) -> EvalOutcome {
    let mut context = Context::builder()
        .instructions_remaining(EVAL_FUEL)
        .build()
        .expect("fuzz context must build");
    context.set_optimizer_options(options);
    eval_outcome_in(&mut context, source)
}

/// Evaluate `source` in a caller-provided context, then drain the job
/// queue (P5.4 matrix fidelity).
///
/// Plain `eval` never runs async continuations, so promise callbacks and
/// async-generator resumptions are invisible without the drain; the tester
/// drains after every test, and the matrix tool must match it. When the
/// eval itself throws, its outcome stands (jobs never ran); otherwise a
/// job error replaces the outcome (it explains `err-*` cells from jobs).
/// `run_jobs` only drains settled microtasks — pending promises sit
/// unresolved — so the drain always terminates.
///
/// # Panics
///
/// Re-panics on `EngineError::Panic`, like [`eval_outcome`].
pub fn eval_outcome_drained_in(context: &mut Context, source: &str) -> EvalOutcome {
    let outcome = eval_outcome_in(context, source);
    if !outcome.completed {
        return outcome;
    }
    match context.run_jobs() {
        Ok(()) => outcome,
        Err(err) => EvalOutcome {
            completed: false,
            error_class: Some(error_class_name(&err)),
            primitive: String::new(),
        },
    }
}

/// Evaluate `source` in a fresh default context under [`EVAL_FUEL`] with
/// `options`, then drain the job queue (see [`eval_outcome_drained_in`]).
///
/// # Panics
///
/// Panics if the fuzz context fails to build; re-panics on
/// `EngineError::Panic`, like [`eval_outcome`].
pub fn eval_outcome_drained_with_optimizer(
    source: &str,
    options: boa_engine::optimizer::OptimizerOptions,
) -> EvalOutcome {
    let mut context = Context::builder()
        .instructions_remaining(EVAL_FUEL)
        .build()
        .expect("fuzz context must build");
    context.set_optimizer_options(options);
    eval_outcome_drained_in(&mut context, source)
}

/// Compiles `source` exactly like [`compile_outcome`], but under GC stress.
///
/// Compilation allocates heavily (AST, bytecode, constants) without running
/// user code, so no timing filter applies: any divergence from the normal
/// leg is a bytecompiler rooting bug.
///
/// # Panics
///
/// Panics if the fuzz context fails to build, like [`compile_outcome`].
pub fn compile_outcome_stressed(source: &str) -> CompileOutcome {
    let _guard = StressGuard::stress(stress_cadence());
    compile_outcome(source)
}

/// Full differential legs on one program; returns the normal-leg outcome.
///
/// Normal vs GC-stress (missing roots/timing) and normal vs unoptimized
/// (optimizer miscompiles, except the filed DCE completion leak — see
/// [`is_filed_dce_completion_leak`]). Parse failures never reach here —
/// the caller gates — so every leg runs on compilable-or-early-error
/// programs.
///
/// # Panics
///
/// Panics on any leg divergence (the fuzz target treats these as crashes).
pub fn check_legs(source: &str, what: &str) -> EvalOutcome {
    let allocs_before = gc_total_allocs();
    let first = eval_outcome(source);
    let stressed = should_stress(source, gc_total_allocs() - allocs_before);
    let second = if stressed {
        eval_outcome_stressed(source)
    } else {
        eval_outcome(source)
    };
    let leg = if stressed { "gc-stress" } else { "second" };
    assert!(
        outcomes_equal(&first, &second),
        "{what}: {leg} eval diverged.\nSource:\n{source}\nFirst:\n{first:?}\nSecond:\n{second:?}",
    );

    let unoptimized = eval_outcome_unoptimized(source);
    if !outcomes_equal(&first, &unoptimized)
        && !is_filed_dce_completion_leak(source, &first, &unoptimized)
    {
        panic!(
            "{what}: optimizer on/off diverged.\nSource:\n{source}\nOptimized:\n{first:?}\nUnoptimized:\n{unoptimized:?}",
        );
    }

    first
}

/// Assert one metamorphic [`Variant`] evaluates identically to its original
/// (whose normal-leg outcome is `first`).
///
/// A divergence indicts the transform first (see the triage rule in
/// [`crate::metamorphic`]) — but before reporting, the rare path
/// re-compares unoptimized: it runs the optimizer leg on the variant
/// itself (a variant-only miscompile is caught by name), and skips when
/// both sides agree unoptimized (the split was optimizer-caused on the
/// original side, which [`check_legs`] owns — it either reported a new
/// optimizer bug or skipped the filed DCE leak). Sound only when called
/// after [`check_legs`] on the same original.
///
/// # Panics
///
/// Panics on a genuine metamorphic split or a variant-only optimizer
/// miscompile (the fuzz target treats these as crashes).
pub fn check_metamorphic_pair(source: &str, first: &EvalOutcome, variant: &Variant) {
    let outcome = eval_outcome(&variant.source);
    if outcomes_equal(first, &outcome) {
        return;
    }
    let unopt_first = eval_outcome_unoptimized(source);
    let unopt_variant = eval_outcome_unoptimized(&variant.source);
    assert!(
        outcomes_equal(&outcome, &unopt_variant),
        "variant (transform: {}): optimizer on/off diverged.\nVariant:\n{}\nOptimized:\n{outcome:?}\nUnoptimized:\n{unopt_variant:?}",
        variant.name, variant.source,
    );
    if outcomes_equal(&unopt_first, &unopt_variant) {
        return;
    }
    panic!(
        "metamorphic divergence (transform: {}).\nSource:\n{source}\nVariant:\n{}\nFirst:\n{first:?}\nVariant outcome:\n{outcome:?}",
        variant.name, variant.source,
    );
}

/// Whether the stressed leg should run for `source` after a normal leg
/// that served `normal_allocs` allocations.
///
/// Skips when the program can legitimately observe collection timing
/// ([`gc_observes_collection_timing`]) or when the normal leg exceeded
/// [`STRESS_MAX_ALLOCS`] (the stressed leg would be pathologically slow).
/// Both skip branches keep the plain second leg, so determinism is still
/// checked — only the stress differential is skipped.
#[must_use]
pub fn should_stress(source: &str, normal_allocs: u64) -> bool {
    normal_allocs <= STRESS_MAX_ALLOCS && !gc_observes_collection_timing(source)
}

/// Whether `source` can legitimately observe collection timing.
///
/// `WeakRef.deref` and `WeakMap`/`WeakSet` probes after key death may
/// resolve differently depending on when the collector ran — that is allowed
/// observable behavior, not an engine bug — as may `FinalizationRegistry`
/// delivery once callbacks run. Callers skip the stressed leg (keeping the
/// normal determinism legs) when this returns `true`.
///
/// Best-effort substring pre-filter: computed spellings (e.g. building
/// `"WeakRef"` from `"Weak" + "Ref"`) evade it. A stress divergence on a
/// *filtered* program is therefore still triaged as a possible engine bug,
/// never auto-dismissed.
#[must_use]
pub fn gc_observes_collection_timing(source: &str) -> bool {
    const SENSITIVE: &[&str] = &["WeakRef", "FinalizationRegistry", "WeakMap", "WeakSet"];
    SENSITIVE.iter().any(|api| source.contains(api))
}

/// Reprint `source` through parse+print: `Ok` with the printed form when it
/// parses, `Err` with the error class when it does not.
///
/// Powers the vm-level idempotency oracle (`eval(src)` vs `eval(reprint)`);
/// the parser target owns the finer print-fixpoint property.
pub fn reprint(source: &str) -> Result<String, String> {
    use boa_ast::scope::Scope;
    use boa_interner::{Interner, ToInternedString};
    use boa_parser::Parser;

    let mut interner = Interner::default();
    match Parser::new(Source::from_reader(Cursor::new(source), None))
        .parse_script(&Scope::new_global(), &mut interner)
    {
        Ok(script) => Ok(script.statements().to_interned_string(&interner)),
        Err(err) => Err(format!("{err:?}")),
    }
}

/// The comparable outcome of one script compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutcome {
    /// Parse + compile both succeeded.
    pub compiled: bool,
    /// Full disassembly on success (empty otherwise). Compared for
    /// compile-determinism: the same source must produce identical code.
    pub disassembly: String,
}

/// Parse + compile `source` in a fresh context, running the P5 verify hook.
pub fn compile_outcome(source: &str) -> CompileOutcome {
    use boa_engine::Script;

    let mut context = Context::builder().build().expect("fuzz context must build");
    stress_point(&mut context);
    let parsed = Script::parse(
        Source::from_reader(Cursor::new(source), None),
        None,
        &mut context,
    );
    let Ok(script) = parsed else {
        return CompileOutcome {
            compiled: false,
            disassembly: String::new(),
        };
    };
    match script.codeblock(&mut context) {
        Ok(block) => {
            verify_hook(&block);
            CompileOutcome {
                compiled: true,
                disassembly: format!("{block}"),
            }
        }
        Err(_) => CompileOutcome {
            compiled: false,
            disassembly: String::new(),
        },
    }
}

/// Bytecode verifier hook (P5.1).
///
/// Called with every successfully compiled block; rejects any block that
/// violates the validity model (`docs/bytecode-validity.md`). A failure here
/// is a compiler bug and must crash the fuzzer with the precise error.
pub fn verify_hook(block: &boa_gc::Gc<boa_engine::vm::CodeBlock>) {
    block.verify().expect("compiler produced invalid bytecode");
}

/// The comparable outcome of one oracle child run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleOutcome {
    /// The child exited 0 with no error pattern (completed).
    pub completed: bool,
    /// Native error class, or `thrown-value` for `uncaught exception:`.
    /// `None` when completed or unclassifiable.
    pub error_class: Option<String>,
    /// The child was killed after [`ORACLE_TIMEOUT_SECS`] (skip, not signal).
    pub timed_out: bool,
    /// The child failed in a way no pattern classifies (skip, not signal).
    pub unclassified: bool,
}

/// Run `source` under the oracle shell at `oracle_path` (`-e` protocol).
///
/// Spawns `<oracle> -e <source>`, waits up to [`ORACLE_TIMEOUT_SECS`], and
/// kills a straggler. jsshell (script goal) is the supported oracle;
/// `node -e` runs module goal and would skew `await`/`import` programs.
pub fn eval_oracle(oracle_path: &str, source: &str) -> OracleOutcome {
    let mut child = match Command::new(oracle_path)
        .arg("-e")
        .arg(source)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            return OracleOutcome {
                completed: false,
                error_class: None,
                timed_out: false,
                unclassified: true,
            };
        }
    };
    // Poll-then-kill: std has no `wait_timeout`, and one tiny dep is not
    // worth avoiding twenty lines. 50ms granularity.
    let mut timed_out = false;
    for _ in 0..ORACLE_TIMEOUT_SECS * 20 {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    if child
        .try_wait()
        .map(|status| status.is_none())
        .unwrap_or(false)
    {
        timed_out = true;
        let _ = child.kill();
    }
    let output = child.wait_with_output();
    if timed_out {
        return OracleOutcome {
            completed: false,
            error_class: None,
            timed_out: true,
            unclassified: false,
        };
    }
    let Ok(output) = output else {
        return OracleOutcome {
            completed: false,
            error_class: None,
            timed_out: false,
            unclassified: true,
        };
    };
    parse_oracle_output(output.status.success(), &output.stderr)
}

/// Classify oracle child output: exit status + stderr error pattern.
///
/// jsshell prints `<Class>Error: msg` or `uncaught exception: <value>`;
/// exit 0 with neither means completion.
fn parse_oracle_output(success: bool, stderr: &[u8]) -> OracleOutcome {
    let text = String::from_utf8_lossy(stderr);
    const CLASSES: [&str; 9] = [
        "AggregateError",
        "Error",
        "EvalError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "TypeError",
        "URIError",
        "UriError",
    ];
    let class = text
        .split_whitespace()
        .find_map(|word| {
            let bare = word.trim_matches(|char: char| !char.is_alphanumeric());
            let stem = bare.strip_suffix(':').unwrap_or(bare);
            CLASSES.contains(&stem).then(|| stem.to_owned())
        })
        .or_else(|| {
            text.contains("uncaught exception:")
                .then(|| String::from("thrown-value"))
        });
    match (success, class) {
        (true, None) => OracleOutcome {
            completed: true,
            error_class: None,
            timed_out: false,
            unclassified: false,
        },
        (_, Some(class)) => OracleOutcome {
            completed: false,
            error_class: Some(class),
            timed_out: false,
            unclassified: false,
        },
        _ => OracleOutcome {
            completed: false,
            error_class: None,
            timed_out: false,
            unclassified: true,
        },
    }
}

/// A known open-bug divergence shape: libFuzzer stops at the first crash,
/// so without suppression the loop could never get past already-filed bugs
/// to new ones. Each entry retires when its bug lands (enforced by
/// `known_divergence_filters_only_open_bugs`).
struct KnownDivergence {
    /// Regression-DB id; must still be `status = "open"`.
    bug: &'static str,
    boa_completed: bool,
    boa_class: Option<&'static str>,
    oracle_class: &'static str,
}

/// Open-bug divergence shapes, skipped (not asserted) by oracle mode.
///
/// Deliberately narrow (exact completion triples), and the unfiltered P3
/// batch differential remains the backstop: a *new* bug sharing a triple
/// would be suppressed here but still caught when fuzz corpora flow through
/// `boa_differential`.
const KNOWN_DIVERGENCES: &[KnownDivergence] = &[
    // #20: `var [];` parses in Boa (completes when unreached) but is a
    // SyntaxError in both oracles.
    KnownDivergence {
        bug: "destructuring-missing-initializer-accepted",
        boa_completed: true,
        boa_class: None,
        oracle_class: "SyntaxError",
    },
    // #20, reached-code shape: Boa throws TypeError at runtime instead.
    KnownDivergence {
        bug: "destructuring-missing-initializer-accepted",
        boa_completed: false,
        boa_class: Some("TypeError"),
        oracle_class: "SyntaxError",
    },
    // #21: bare `using;` is a Boa SyntaxError but a ReferenceError in both
    // oracles.
    KnownDivergence {
        bug: "bare-using-identifier-rejected",
        boa_completed: false,
        boa_class: Some("SyntaxError"),
        oracle_class: "ReferenceError",
    },
];

/// The open-bug id when a Boa/oracle pair matches a known divergence shape.
#[must_use]
pub fn known_divergence(boa: &EvalOutcome, oracle: &OracleOutcome) -> Option<&'static str> {
    if oracle.completed {
        return None;
    }
    let oracle_class = oracle.error_class.as_deref()?;
    KNOWN_DIVERGENCES.iter().find_map(|known| {
        (boa.completed == known.boa_completed
            && boa.error_class.as_deref() == known.boa_class
            && oracle_class == known.oracle_class)
            .then_some(known.bug)
    })
}

/// Whether an optimizer-leg divergence is the filed DCE completion leak
/// (`dce-dead-branch-completion-leak`), and must be skipped so the loop
/// reaches new bugs.
///
/// The fingerprint is signature plus mechanism confirmation: both sides
/// complete, the unoptimized side completes `undefined`, the optimized
/// side completes something else, and re-running with every pass *except*
/// dead-code elimination agrees with unoptimized. A DCE-plus-another-pass
/// interaction, or any other optimizer bug, fails the confirmation and
/// panics normally. Costs one extra eval, and only runs on divergence
/// (the steady-state loop never pays it).
#[must_use]
pub fn is_filed_dce_completion_leak(
    source: &str,
    optimized: &EvalOutcome,
    unoptimized: &EvalOutcome,
) -> bool {
    if !optimized.completed || !unoptimized.completed {
        return false;
    }
    if unoptimized.primitive != "undefined" || optimized.primitive == "undefined" {
        return false;
    }
    let without_dce = eval_outcome_with_optimizer(
        source,
        boa_engine::optimizer::OptimizerOptions::OPTIMIZE_ALL
            .difference(boa_engine::optimizer::OptimizerOptions::DEAD_CODE_ELIMINATION),
    );
    outcomes_equal(&without_dce, unoptimized)
}

/// Compare a Boa outcome against an oracle outcome (completion classes).
///
/// Returns `None` when the pair is unjudgeable (fuel exhaustion, oracle
/// timeout, unclassified oracle failure): the caller skips, never asserts.
/// `Some(true)` agrees, `Some(false)` is a reportable divergence.
#[must_use]
pub fn oracle_agrees(boa: &EvalOutcome, oracle: &OracleOutcome) -> Option<bool> {
    if oracle.timed_out || oracle.unclassified {
        return None;
    }
    if boa.error_class.as_deref() == Some("FuelExhausted") {
        return None;
    }
    match (boa.completed, oracle.completed) {
        (true, true) => Some(true),
        (false, false) => Some(boa.error_class == oracle.error_class),
        _ => Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boa_gc::{gc_collections, gc_total_allocs};

    #[test]
    fn drained_eval_runs_promise_callbacks() {
        // Differential: the same script leaves the flag unset without the
        // drain (jobs never run) and sets it with the drain.
        let script = "globalThis.drainFlag = 0; Promise.resolve().then(() => { globalThis.drainFlag = 1; });";
        // Fresh contexts need explicit fuel: `Context::default()` carries
        // zero `instructions_remaining`, which the `fuzz` feature enforces.
        let mut plain = Context::builder()
            .instructions_remaining(EVAL_FUEL)
            .build()
            .expect("test context must build");
        let outcome = eval_outcome_in(&mut plain, script);
        assert!(outcome.completed);
        let flag = plain
            .eval(Source::from_bytes("globalThis.drainFlag;"))
            .expect("read flag");
        let flag = flag.to_string(&mut plain).expect("render flag");
        assert_eq!(flag.to_std_string_escaped(), "0");

        let mut drained = Context::builder()
            .instructions_remaining(EVAL_FUEL)
            .build()
            .expect("test context must build");
        let outcome = eval_outcome_drained_in(&mut drained, script);
        assert!(outcome.completed);
        let flag = drained
            .eval(Source::from_bytes("globalThis.drainFlag;"))
            .expect("read flag");
        let flag = flag.to_string(&mut drained).expect("render flag");
        assert_eq!(flag.to_std_string_escaped(), "1");
    }

    #[test]
    fn primitives_render_deterministically() {
        let first = eval_outcome("1 + 2;");
        let second = eval_outcome("1 + 2;");
        assert!(first.completed);
        assert_eq!(first.primitive, "3.0");
        assert!(outcomes_equal(&first, &second));
    }

    #[test]
    fn float_edge_cases_render() {
        assert_eq!(eval_outcome("0/0;").primitive, "NaN");
        assert_eq!(eval_outcome("-0;").primitive, "-0.0");
        assert_eq!(eval_outcome("0.1 + 0.2;").primitive, "0.30000000000000004");
        // NaN equals NaN across evals (compare-by-render, not IEEE).
        assert!(outcomes_equal(
            &eval_outcome("0/0;"),
            &eval_outcome("Number('nan');")
        ));
    }

    #[test]
    fn error_classes_classify() {
        assert_eq!(
            eval_outcome("zzz_no_such_binding;").error_class.as_deref(),
            Some("ReferenceError")
        );
        assert_eq!(
            eval_outcome("null.x;").error_class.as_deref(),
            Some("TypeError")
        );
        assert_eq!(
            eval_outcome("let [a];").error_class.as_deref(),
            // P4 bug #20 (open): parses today, dies at runtime. Post-fix
            // this becomes SyntaxError; the assertion pins current truth.
            Some("TypeError")
        );
        assert_eq!(
            eval_outcome("function (;").error_class.as_deref(),
            Some("SyntaxError")
        );
    }

    #[test]
    fn thrown_values_marked_not_classed() {
        let outcome = eval_outcome("throw 42;");
        assert!(!outcome.completed);
        assert_eq!(outcome.error_class.as_deref(), Some("thrown-value"));
        assert_eq!(outcome.primitive, "42.0");
    }

    #[test]
    fn fuel_exhaustion_detected() {
        let outcome = eval_outcome("while (true) {}");
        assert!(!outcome.completed);
        assert_eq!(
            outcome.error_class.as_deref(),
            Some("FuelExhausted"),
            "outcome: {outcome:?}"
        );
    }

    #[test]
    fn objects_compare_by_typeof_tag() {
        assert_eq!(eval_outcome("({});").primitive, "typeof:object");
        assert_eq!(
            eval_outcome("(function () {});").primitive,
            "typeof:function"
        );
        assert_eq!(eval_outcome("Symbol();").primitive, "typeof:symbol");
    }

    #[test]
    fn oracle_output_parses() {
        let completed = parse_oracle_output(true, b"");
        assert!(completed.completed);

        let reference = parse_oracle_output(
            false,
            b"-e:1:1 ReferenceError: x is not defined\nStack:\n  @-e:1:1",
        );
        assert!(!reference.completed);
        assert_eq!(reference.error_class.as_deref(), Some("ReferenceError"));

        let thrown =
            parse_oracle_output(false, b"-e:1:1 uncaught exception: 42\nStack:\n  @-e:1:1");
        assert_eq!(thrown.error_class.as_deref(), Some("thrown-value"));

        let unknown = parse_oracle_output(false, b"");
        assert!(unknown.unclassified);

        // Near-miss words must not classify: exit 0 + no exact class word
        // still counts as completion.
        let zero_with_warning = parse_oracle_output(true, b"warning: TypeError-ish note");
        assert!(zero_with_warning.completed);
    }

    #[test]
    fn oracle_agreement_rules() {
        let completed = EvalOutcome {
            completed: true,
            error_class: None,
            primitive: String::from("1"),
        };
        let threw_type = EvalOutcome {
            completed: false,
            error_class: Some(String::from("TypeError")),
            primitive: String::new(),
        };
        let fuel = EvalOutcome {
            completed: false,
            error_class: Some(String::from("FuelExhausted")),
            primitive: String::new(),
        };
        let oracle_ok = OracleOutcome {
            completed: true,
            error_class: None,
            timed_out: false,
            unclassified: false,
        };
        let oracle_type = OracleOutcome {
            completed: false,
            error_class: Some(String::from("TypeError")),
            timed_out: false,
            unclassified: false,
        };
        let oracle_syntax = OracleOutcome {
            completed: false,
            error_class: Some(String::from("SyntaxError")),
            timed_out: false,
            unclassified: false,
        };
        let oracle_timeout = OracleOutcome {
            completed: false,
            error_class: None,
            timed_out: true,
            unclassified: false,
        };
        assert_eq!(oracle_agrees(&completed, &oracle_ok), Some(true));
        assert_eq!(oracle_agrees(&threw_type, &oracle_type), Some(true));
        assert_eq!(oracle_agrees(&threw_type, &oracle_syntax), Some(false));
        assert_eq!(oracle_agrees(&completed, &oracle_type), Some(false));
        assert_eq!(oracle_agrees(&completed, &oracle_timeout), None);
        assert_eq!(oracle_agrees(&fuel, &oracle_ok), None);
    }

    #[test]
    fn nesting_depth_counts_all_bracket_kinds() {
        assert_eq!(max_nesting_depth("a + b;"), 0);
        assert_eq!(max_nesting_depth("((a) + (b));"), 2);
        assert_eq!(max_nesting_depth("([{x}]);"), 3);
        // Unbalanced closers clamp (never underflow); strings over-count
        // (conservative: drops derived inputs, never originals).
        assert_eq!(max_nesting_depth(")];"), 0);
        assert_eq!(max_nesting_depth("\"(((\";"), 3);
    }

    #[test]
    fn with_optimizer_matches_unoptimized_on_empty() {
        assert!(outcomes_equal(
            &eval_outcome_with_optimizer("1;", boa_engine::optimizer::OptimizerOptions::empty()),
            &eval_outcome_unoptimized("1;")
        ));
    }

    #[test]
    fn dce_fingerprint_matches_filed_shape_only() {
        // Pins CURRENT (buggy) behavior; update when
        // `dce-dead-branch-completion-leak` lands (the skip goes vacuous,
        // then this test fails loud to force its removal).
        let source = "0; if (false) { 1; }";
        let optimized = eval_outcome(source);
        let unoptimized = eval_outcome_unoptimized(source);
        assert!(!outcomes_equal(&optimized, &unoptimized));
        assert!(is_filed_dce_completion_leak(
            source,
            &optimized,
            &unoptimized
        ));
        // Non-completion divergences never match (no confirmation eval).
        let throwing = eval_outcome("null.x;");
        assert!(!is_filed_dce_completion_leak(
            "null.x;", &throwing, &throwing
        ));
        // Mechanism confirmation rejects fabricated signatures: `0;` has
        // no DCE trigger, so DCE-off cannot agree with the fabricated
        // `undefined` side.
        let plain = eval_outcome("0;");
        let fabricated = EvalOutcome {
            completed: true,
            error_class: None,
            primitive: String::from("undefined"),
        };
        assert!(!is_filed_dce_completion_leak("0;", &plain, &fabricated));
    }

    #[test]
    fn legs_silently_pass_filed_dce_shape() {
        // End-to-end wiring pin for both DCE skips (optimizer leg +
        // metamorphic lazy path). Update when
        // `dce-dead-branch-completion-leak` lands: the divergence asserts
        // below fail loud to force removing the skips.
        check_legs("1;", "sanity");
        let source = "0; if (false) { 1; }";
        let first = check_legs(source, "dce");
        let variants = metamorphic_variants(source);
        assert!(!variants.is_empty());
        // The pair must still diverge optimized (else this test pins
        // vacuous agreement instead of the skip).
        assert!(
            variants
                .iter()
                .any(|v| !outcomes_equal(&first, &eval_outcome(&v.source))),
            "DCE pair stopped diverging: fix landed, remove the skips"
        );
        for variant in &variants {
            check_metamorphic_pair(source, &first, variant);
        }
    }

    #[test]
    fn metamorphic_hook_delegates_to_transforms() {
        // `1;` fires only paren-add (the literal gains parens); real
        // coverage lives in `metamorphic.rs` (unit battery + oracle gate).
        assert_eq!(metamorphic_variants("1;").len(), 1);
        assert_eq!(metamorphic_variants("(a) + (b);").len(), 1);
    }

    #[test]
    fn known_divergences_match_their_shapes() {
        let completed = EvalOutcome {
            completed: true,
            error_class: None,
            primitive: String::from("undefined"),
        };
        let threw_type = EvalOutcome {
            completed: false,
            error_class: Some(String::from("TypeError")),
            primitive: String::new(),
        };
        let threw_syntax = EvalOutcome {
            completed: false,
            error_class: Some(String::from("SyntaxError")),
            primitive: String::new(),
        };
        let oracle_syntax = OracleOutcome {
            completed: false,
            error_class: Some(String::from("SyntaxError")),
            timed_out: false,
            unclassified: false,
        };
        let oracle_reference = OracleOutcome {
            completed: false,
            error_class: Some(String::from("ReferenceError")),
            timed_out: false,
            unclassified: false,
        };
        assert_eq!(
            known_divergence(&completed, &oracle_syntax),
            Some("destructuring-missing-initializer-accepted")
        );
        assert_eq!(
            known_divergence(&threw_type, &oracle_syntax),
            Some("destructuring-missing-initializer-accepted")
        );
        assert_eq!(
            known_divergence(&threw_syntax, &oracle_reference),
            Some("bare-using-identifier-rejected")
        );
        // A genuinely new shape is not suppressed.
        assert_eq!(known_divergence(&threw_type, &oracle_reference), None);
    }

    #[test]
    fn known_divergence_filters_only_open_bugs() {
        // Self-retiring: every filter entry must reference a live `open`
        // bug. When a bug lands, this test fails until the entry is
        // removed — a landed fix must re-expose its shape to the oracle.
        let manifest = std::fs::read_to_string("../../tests/regression/regressions.toml")
            .expect("regressions.toml must be readable from tests/fuzz");
        for known in KNOWN_DIVERGENCES {
            let id_line = format!("id = \"{}\"", known.bug);
            let id_pos = manifest
                .find(&id_line)
                .unwrap_or_else(|| panic!("filter references unknown bug id: {}", known.bug));
            let entry_tail = &manifest[id_pos..];
            let status_line = entry_tail
                .lines()
                .find(|line| line.starts_with("status = "))
                .unwrap_or_else(|| panic!("no status for bug {}", known.bug));
            assert_eq!(
                status_line, "status = \"open\"",
                "bug {} is not open; retire its filter entry",
                known.bug
            );
        }
    }

    #[test]
    fn reprint_round_trips_valid_source() {
        let printed = reprint("let a = 1;").expect("must parse");
        assert!(printed.contains("let a"), "printed: {printed}");
        assert!(reprint("function (;").is_err());
    }

    #[test]
    fn compile_outcome_deterministic() {
        let first = compile_outcome("let a = 1; a + 2;");
        let second = compile_outcome("let a = 1; a + 2;");
        assert!(first.compiled);
        assert!(!first.disassembly.is_empty());
        assert_eq!(first, second);
    }

    #[test]
    fn compile_outcome_rejects_invalid() {
        assert!(!compile_outcome("function (;").compiled);
    }

    #[test]
    fn should_stress_gates_on_churn_and_timing() {
        assert!(should_stress("1 + 1;", 100));
        assert!(should_stress("1 + 1;", STRESS_MAX_ALLOCS));
        assert!(!should_stress("1 + 1;", STRESS_MAX_ALLOCS + 1));
        assert!(!should_stress("new WeakRef({});", 100));
        assert!(!should_stress("new WeakRef({});", STRESS_MAX_ALLOCS + 1));
    }

    #[test]
    fn churn_outlier_skips_stress_end_to_end() {
        // The libFuzzer slow unit, reduced: a fuel-saturated import loop.
        // Its normal leg must exceed the cap — fail-safe: if the engine
        // ever gets lean enough to fit, recalibrate the cap instead of
        // deleting this test.
        let outlier = "while (import(this)) {}";
        let before = gc_total_allocs();
        let _ = eval_outcome(outlier);
        let outlier_allocs = gc_total_allocs() - before;
        assert!(
            outlier_allocs > STRESS_MAX_ALLOCS,
            "recalibrate: outlier churn fell to {outlier_allocs}"
        );
        assert!(!should_stress(outlier, outlier_allocs));
        // A typical input fits with an order of magnitude to spare.
        let before = gc_total_allocs();
        let _ = eval_outcome("let a = [1, 2, 3]; a.map((x) => x * 2);");
        let typical = gc_total_allocs() - before;
        assert!(
            typical < STRESS_MAX_ALLOCS / 10,
            "typical churn grew to {typical}"
        );
    }

    #[test]
    fn stress_leg_agrees_on_allocation_heavy_programs() {
        // Deep graphs, cycles, closures, exceptions, string churn: every
        // shape must evaluate identically with and without stress.
        let fixtures = [
            "let a = []; for (let i = 0; i < 500; i++) a.push({i, s: 'x'.repeat(i % 37)}); a.length;",
            "let o = {}; o.self = o; let p = {o}; p.o.self === o;",
            "function mk(n) { return () => () => n; } let fns = []; for (let i = 0; i < 200; i++) fns.push(mk(i)); fns[199]()();",
            "let s = ''; for (let i = 0; i < 300; i++) { try { if (i % 7 === 0) throw new RangeError('x' + i); s += i; } catch (e) { s += e.name; } } s.length;",
            "let m = new Map(); for (let i = 0; i < 200; i++) m.set('k' + i, [i, {v: i}]); m.size;",
            "((((((((((1 + 2))))))))));",
            "null.x;",
            "throw { custom: [1, 2, 3] };",
        ];
        for source in fixtures {
            let normal = eval_outcome(source);
            let stressed = eval_outcome_stressed(source);
            assert!(
                outcomes_equal(&normal, &stressed),
                "stress divergence on fixture: {source}\nnormal:  {normal:?}\nstressed: {stressed:?}"
            );
        }
    }

    #[test]
    fn stressed_eval_collects_more_than_normal_eval() {
        // Proof the stressed leg actually stresses: an explicit every-10
        // guard must collect strictly more than the normal path on the same
        // allocation-heavy program. (Explicit guard, not
        // `eval_outcome_stressed`, so a concurrently-set
        // `BOA_FUZZ_GC_STRESS_EVERY` cannot skew the cadence.)
        let source = "let a = []; for (let i = 0; i < 500; i++) a.push({i}); a.length;";
        let normal_before = gc_collections();
        let _ = eval_outcome(source);
        let normal_delta = gc_collections() - normal_before;
        let guard = StressGuard::stress(NonZeroU64::new(10).expect("nonzero"));
        let stressed_before = gc_collections();
        let _ = eval_outcome(source);
        let stressed_delta = gc_collections() - stressed_before;
        drop(guard);
        assert!(
            stressed_delta > normal_delta,
            "stress collected {stressed_delta}, normal {normal_delta}"
        );
        assert!(
            stressed_delta >= 10,
            "every-10 over a 500-push program must collect often: {stressed_delta}"
        );
    }

    #[test]
    fn stress_leg_agrees_in_host_context() {
        use crate::adversarial::host_context;

        for source in [
            "new URL('https://example.com/a?b=c').host;",
            "new Response('hi', { status: 201 }).status;",
            "let u = new URLSearchParams('a=1&a=2'); u.getAll('a').length;",
        ] {
            let normal = eval_outcome_in(&mut host_context(), source);
            let stressed = eval_outcome_stressed_in(&mut host_context(), source);
            assert!(
                outcomes_equal(&normal, &stressed),
                "host stress divergence on: {source}\nnormal:  {normal:?}\nstressed: {stressed:?}"
            );
        }
    }

    #[test]
    fn stressed_compile_matches_normal() {
        let source = "let a = 1; function f(x) { return x * 2; } f(a + 2);";
        assert_eq!(compile_outcome(source), compile_outcome_stressed(source));
        assert!(!compile_outcome_stressed("function (;").compiled);
    }

    #[test]
    fn collection_timing_filter_matches_weak_apis() {
        for source in [
            "new WeakRef({});",
            "new FinalizationRegistry(() => {});",
            "new WeakMap().set({}, 1);",
            "new WeakSet().add({});",
            "globalThis['WeakRef'];",
        ] {
            assert!(
                gc_observes_collection_timing(source),
                "filter missed: {source}"
            );
        }
        for source in [
            "1 + 1;",
            "let weak = 1;",
            "new Map().set({}, 1);",
            "new Set([1, 2]);",
            "let WeaklyHeld = 3;",
        ] {
            assert!(
                !gc_observes_collection_timing(source),
                "filter over-matched: {source}"
            );
        }
    }

    #[test]
    fn stress_cadence_env_override() {
        let prev = std::env::var(GC_STRESS_ENV_VAR).ok();
        std::env::set_var(GC_STRESS_ENV_VAR, "7");
        assert_eq!(stress_cadence(), NonZeroU64::new(7).expect("nonzero"));
        for bad in ["0", "-3", "nope", ""] {
            std::env::set_var(GC_STRESS_ENV_VAR, bad);
            assert_eq!(
                stress_cadence(),
                NonZeroU64::new(GC_STRESS_EVERY).expect("nonzero"),
                "invalid override {bad:?} must fall back to the default"
            );
        }
        match prev {
            Some(value) => std::env::set_var(GC_STRESS_ENV_VAR, value),
            None => std::env::remove_var(GC_STRESS_ENV_VAR),
        }
    }

    #[test]
    fn stress_point_collects() {
        let mut context = Context::builder()
            .instructions_remaining(EVAL_FUEL)
            .build()
            .expect("test context must build");
        let _ = eval_outcome_in(&mut context, "let a = [1, 2, 3]; a;");
        let before = gc_collections();
        stress_point(&mut context);
        assert_eq!(gc_collections(), before + 1);
    }

    #[test]
    fn generated_programs_agree_under_stress() {
        use arbitrary::{Arbitrary, Unstructured};

        use crate::common::FuzzSource;

        // Soundness probe for the vm/bytecompiler stress differentials: 200
        // deterministic generator outputs must evaluate (and compile)
        // identically with and without stress. Inputs whose normal legs
        // already disagree are skipped — pre-existing nondeterminism, not
        // stress's signal — so this test can only fail on stress-caused
        // divergence.
        fn next_bytes(state: &mut u64, out: &mut [u8]) {
            for chunk in out.chunks_mut(8) {
                *state ^= *state << 13;
                *state ^= *state >> 7;
                *state ^= *state << 17;
                let bytes = state.to_le_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
        }

        let mut checked = 0;
        for seed in 0..200_u64 {
            let mut state = seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x1234_5678);
            let mut bytes = vec![0_u8; 512];
            next_bytes(&mut state, &mut bytes);
            let mut unstructured = Unstructured::new(&bytes);
            let Ok(input) = FuzzSource::arbitrary(&mut unstructured) else {
                continue;
            };
            let source = &input.source;
            let allocs_before = gc_total_allocs();
            let first = eval_outcome(source);
            let normal_allocs = gc_total_allocs() - allocs_before;
            let second = eval_outcome(source);
            if !outcomes_equal(&first, &second) {
                continue;
            }
            // Mirrors the target's leg selection exactly, including the
            // churn gate: whatever the target stresses, the probe stresses.
            if should_stress(source, normal_allocs) {
                let stressed = eval_outcome_stressed(source);
                assert!(
                    outcomes_equal(&first, &stressed),
                    "stress divergence on generated input (seed {seed}).\nSource:\n{source}\nNormal:\n{first:?}\nStressed:\n{stressed:?}"
                );
            }
            assert_eq!(
                compile_outcome(source),
                compile_outcome_stressed(source),
                "stressed compile diverged (seed {seed}).\nSource:\n{source}"
            );
            checked += 1;
        }
        assert!(checked > 150, "too few generatable inputs: {checked}");
    }
}
