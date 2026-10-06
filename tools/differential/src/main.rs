//! Differential testing pipeline: Boa vs a pinned oracle engine.
//!
//! Runs the same composed programs on Boa (in-process [`boa_side`] driver
//! inside a supervised `run-case` child) and the oracle shell, normalizes
//! both observations, and files every mismatch in the triage queue with a
//! minimized reproducer. See `README.md` for the runner contract.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![allow(clippy::too_many_lines, clippy::print_stdout, unreachable_pub)]

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

use boa_outcomes::TestOutcomeResult;
use clap::{Parser, ValueHint};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    boa_side::BoaCaseOutcome,
    capture::RawObservation,
    corpus::{Case, Corpus, load_corpus},
    normalize::{FieldDiff, compare},
    oracle::{OracleKind, OracleOutcome, run_oracle},
    queue::{OracleRecord, QueueEntry, TriageQueue, Verdict, VerdictFile, VerdictRecord, area_of},
    reduce::{Budget, MINIMIZED_BODY_BUDGET, minimize_with_prefix},
    supervise::{ChildEnd, classify_boa_child, supervise},
};

mod boa_side;
mod capture;
mod compose;
mod corpus;
mod normalize;
mod oracle;
mod queue;
mod reduce;
mod supervise;

/// Differential case verdict: the crate-local outcome taxonomy.
///
/// Individual side outcomes reuse the shared [`TestOutcomeResult`]; this
/// enum names the *comparison* result. (P1.2 closed-taxonomy pattern: the
/// tester's taxonomy stays closed to tester outcomes, so differential
/// comparison outcomes live here, not there.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum DiffOutcome {
    /// Both sides observed and normalized equal.
    Agree,
    /// Both sides observed but normalized different.
    Mismatch,
    /// At least one side unobserved (timeout/crash/failure asymmetry).
    OneSided,
}

/// Full per-case record in `diff-results.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CaseRecord {
    /// Stable case id.
    id: String,
    /// Case origin.
    origin: String,
    /// Suspected area.
    area: String,
    /// Boa side outcome.
    boa_outcome: TestOutcomeResult,
    /// Oracle side outcome.
    oracle_outcome: TestOutcomeResult,
    /// Comparison verdict.
    diff: DiffOutcome,
    /// Normalized field diffs (empty unless `Mismatch`).
    field_diffs: Vec<FieldDiff>,
    /// Boa side summary.
    boa_text: String,
    /// Oracle side summary.
    oracle_text: String,
    /// Minimized mismatch-preserving program (empty unless queued).
    minimized_program: String,
    /// Minimizer id (`line-reduce-v2`).
    minimized_by: String,
    /// Minimized test-body line count (harness excluded; budgeted).
    minimized_lines: usize,
    /// Boa observation when valid markers were emitted.
    boa_observation: Option<RawObservation>,
    /// Oracle observation when valid markers were emitted.
    oracle_observation: Option<RawObservation>,
}

/// Exclusion record: a corpus candidate that never ran, with its reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExclusionRecord {
    /// Candidate id.
    id: String,
    /// Machine-readable reason.
    reason: String,
}

/// Run counts for `diff-results.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RunCounts {
    /// Runnable cases executed.
    total: usize,
    /// Agreeing cases.
    agree: usize,
    /// Mismatching cases.
    mismatch: usize,
    /// One-sided (abnormal asymmetry) cases.
    one_sided: usize,
    /// Excluded candidates.
    excluded: usize,
}

/// Full run store (`diff-results.json`, P1.1 conventions).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunResults {
    /// Schema version.
    schema_version: u8,
    /// Writer identity.
    tool: String,
    /// Writer version.
    tool_version: String,
    /// UTC RFC 3339 update time.
    updated: String,
    /// Oracle identity.
    oracle: OracleRecord,
    /// Corpus name.
    corpus: String,
    /// Per-case wall-clock budget in seconds (identical both sides).
    timeout_secs: u64,
    /// Run counts.
    counts: RunCounts,
    /// Exclusion accounting.
    exclusions: Vec<ExclusionRecord>,
    /// Per-case records in corpus order.
    cases: Vec<CaseRecord>,
}

/// Boa vs-oracle differential runner.
#[derive(Debug, Parser)]
#[command(author, version, about, name = "Boa differential tester")]
enum Cli {
    /// Run a corpus on both sides and file mismatches in the triage queue.
    Run {
        /// Corpus manifest (`corpora/*.toml`).
        #[arg(long, value_hint = ValueHint::FilePath)]
        corpus: PathBuf,
        /// Oracle adapter (`jsshell`, `node`).
        #[arg(long, default_value = "jsshell")]
        oracle_kind: String,
        /// Oracle binary path (or `BOA_DIFF_ORACLE`).
        #[arg(long, value_hint = ValueHint::FilePath)]
        oracle: Option<PathBuf>,
        /// Expected oracle version string (hard failure on mismatch: never
        /// run against an unpinned engine).
        #[arg(long)]
        expected_version: String,
        /// Output directory (`diff-results.json` + `triage-queue.json`).
        #[arg(short, long, value_hint = ValueHint::DirPath)]
        output: PathBuf,
        /// Per-case wall-clock budget in seconds, identical on both sides.
        #[arg(long, default_value = "30")]
        timeout: u64,
        /// Committed verdicts TOML pre-applied to identical signatures.
        #[arg(long, value_hint = ValueHint::FilePath)]
        verdicts: Option<PathBuf>,
        /// Run cases serially (debugging; default is parallel).
        #[arg(long)]
        serial: bool,
    },
    /// Run one composed program on the Boa side (internal child protocol).
    ///
    /// Do not invoke directly: prints one [`BoaCaseOutcome`] JSON line and
    /// always exits 0. No stability guarantee is made for its arguments.
    RunCase {
        /// Composed program file.
        #[arg(long, value_hint = ValueHint::FilePath)]
        program: PathBuf,
    },
    /// Inspect and verdict the triage queue.
    Triage {
        /// Queue file.
        #[arg(long, value_hint = ValueHint::FilePath)]
        queue: PathBuf,
        /// Breach (exit 1) when any entry is untriaged.
        #[arg(long)]
        check: bool,
        /// List entries with areas and verdicts.
        #[arg(long)]
        list: bool,
        /// Reset `oracle-quirk` verdicts to untriaged (oracle-upgrade drill).
        #[arg(long)]
        reset_oracle_quirks: bool,
        /// Export triaged entries to a committed verdicts TOML file.
        #[arg(long, value_hint = ValueHint::FilePath)]
        export: Option<PathBuf>,
        /// Only entries in this area (with `--set-verdict`).
        #[arg(long)]
        area: Option<String>,
        /// Only entries whose id contains this (with `--set-verdict`).
        #[arg(long)]
        pattern: Option<String>,
        /// Only entries with exactly this comma-separated field-diff set
        /// (with `--set-verdict`; order-independent).
        #[arg(long)]
        fields: Option<String>,
        /// Apply a verdict (`boa-bug`, `oracle-quirk`, `spec-ambiguity`).
        #[arg(long)]
        set_verdict: Option<String>,
        /// Spec URL (`boa-bug`, `spec-ambiguity`).
        #[arg(long)]
        spec: Option<String>,
        /// Regression-DB id (`boa-bug`).
        #[arg(long)]
        regression: Option<String>,
        /// Quirk note (`oracle-quirk`).
        #[arg(long)]
        note: Option<String>,
        /// Upstream filing link (`spec-ambiguity`).
        #[arg(long)]
        filing: Option<String>,
        /// Re-review trigger (`spec-ambiguity`).
        #[arg(long)]
        expiry: Option<String>,
    },
}

/// Shared per-run execution context.
struct RunContext {
    /// Oracle adapter.
    kind: OracleKind,
    /// Oracle binary.
    oracle: PathBuf,
    /// Our own binary (for `run-case` children).
    own_exe: PathBuf,
    /// Scratch dir for composed programs.
    work_dir: PathBuf,
    /// Per-case wall-clock budget.
    timeout_secs: u64,
    /// Unique-tempfile counter.
    counter: AtomicU64,
}

impl RunContext {
    /// Writes `text` to a unique scratch file and returns its path.
    ///
    /// The caller deletes the file after use: minimization stages one file
    /// per probe, so anything left behind accumulates into gigabytes on
    /// large corpora (observed 915MB orphaned from a killed 5.9k-case run).
    /// Staged names are unique per call, so deletion is race-free under the
    /// parallel run. The workdir itself is removed at the end of [`run_corpus`].
    fn stage(&self, id: &str, text: &str) -> Result<PathBuf> {
        let safe: String = id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let nonce = self.counter.fetch_add(1, Ordering::SeqCst);
        let path = self.work_dir.join(format!("{safe}-{nonce}.js"));
        std::fs::write(&path, text)
            .wrap_err_with(|| eyre!("could not stage {}", path.display()))?;
        Ok(path)
    }
}

/// Program entry point.
fn main() -> Result<()> {
    color_eyre::install()?;
    // Worker stacks must fit deeply nested inputs: the parser's stack guard
    // (256 KiB red zone) measures absolute remaining stack, but rayon's 2 MiB
    // default workers overflow on just 32 nesting levels (test262
    // S13.2.1_A1_T1), which needs ~3 MiB optimized and ~13 MiB unoptimized.
    // 32 MiB covers the deepest corpus tests in both profiles; thread stacks
    // commit lazily so the resident cost stays proportional to actual depth.
    rayon::ThreadPoolBuilder::new()
        .stack_size(32 * 1024 * 1024)
        .build_global()
        .expect("no rayon pool exists yet at startup");
    match Cli::parse() {
        Cli::Run {
            corpus,
            oracle_kind,
            oracle,
            expected_version,
            output,
            timeout,
            verdicts,
            serial,
        } => run_corpus(
            &corpus,
            &oracle_kind,
            oracle.as_deref(),
            &expected_version,
            &output,
            timeout,
            verdicts.as_deref(),
            serial,
        ),
        Cli::RunCase { program } => run_case_child(&program),
        Cli::Triage {
            queue,
            check,
            list,
            reset_oracle_quirks,
            export,
            area,
            pattern,
            fields,
            set_verdict,
            spec,
            regression,
            note,
            filing,
            expiry,
        } => triage_cmd(
            &queue,
            check,
            list,
            reset_oracle_quirks,
            export.as_deref(),
            area.as_deref(),
            pattern.as_deref(),
            fields.as_deref(),
            set_verdict.as_deref(),
            spec.as_deref(),
            regression.as_deref(),
            note.as_deref(),
            filing.as_deref(),
            expiry.as_deref(),
        ),
    }
}

/// `run-case` child: evaluates one composed program, prints one JSON line.
fn run_case_child(program: &Path) -> Result<()> {
    let outcome = boa_side::run_case_file(program);
    println!(
        "{}",
        serde_json::to_string(&outcome).wrap_err("could not serialize the case outcome")?
    );
    Ok(())
}

/// Runs a full corpus on both sides and merges mismatches into the queue.
#[allow(clippy::too_many_arguments)]
fn run_corpus(
    corpus_path: &Path,
    oracle_kind: &str,
    oracle: Option<&Path>,
    expected_version: &str,
    output: &Path,
    timeout_secs: u64,
    verdicts_path: Option<&Path>,
    serial: bool,
) -> Result<()> {
    let kind = OracleKind::from_name(oracle_kind)
        .ok_or_else(|| eyre!("unknown oracle kind {oracle_kind:?} (want jsshell|node)"))?;
    let oracle_path = oracle
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("BOA_DIFF_ORACLE").map(PathBuf::from))
        .ok_or_else(|| eyre!("no oracle binary: pass --oracle or set BOA_DIFF_ORACLE"))?;
    let probed = kind
        .probe_version(&oracle_path)
        .map_err(|err| eyre!("{err}"))?;
    if probed != expected_version {
        return Err(eyre!(
            "oracle version is {probed:?}, expected pin {expected_version:?}: refusing to run against an unpinned engine"
        ));
    }
    println!(
        "oracle: {} {probed} ({})",
        kind.name(),
        oracle_path.display()
    );

    let corpus: Corpus = load_corpus(corpus_path).map_err(|err| eyre!("{err}"))?;
    println!(
        "corpus {}: {} cases, {} excluded",
        corpus.name,
        corpus.cases.len(),
        corpus.excluded.len()
    );
    report_exclusions(&corpus);

    std::fs::create_dir_all(output)
        .wrap_err_with(|| eyre!("could not create {}", output.display()))?;
    let work_dir = output.join(".work");
    drop(std::fs::remove_dir_all(&work_dir));
    std::fs::create_dir_all(&work_dir)
        .wrap_err_with(|| eyre!("could not create {}", work_dir.display()))?;
    let ctx = RunContext {
        kind,
        oracle: oracle_path.clone(),
        own_exe: std::env::current_exe().wrap_err("could not locate own binary")?,
        work_dir,
        timeout_secs,
        counter: AtomicU64::new(0),
    };

    // Records travel with their original programs (minimization input);
    // only the minimized form is persisted, so results stay small.
    let mut executed: Vec<(CaseRecord, String, usize)> = if serial {
        corpus
            .cases
            .iter()
            .map(|case| execute_case(case, &ctx))
            .collect::<Result<_>>()?
    } else {
        corpus
            .cases
            .par_iter()
            .map(|case| execute_case(case, &ctx))
            .collect::<Result<_>>()?
    };
    // Verdicts load before minimization (and fail fast on foreign files:
    // refusing after a full run would waste hours on large corpora).
    let verdicts = verdicts_path
        .map(VerdictFile::load)
        .transpose()
        .map_err(|err| eyre!("{err}"))?;
    if let Some(verdicts) = &verdicts
        && (verdicts.oracle_kind != kind.name() || verdicts.oracle_version != probed)
    {
        return Err(eyre!(
            "verdicts file targets {} {}, this run uses {} {probed}: refusing to apply foreign verdicts",
            verdicts.oracle_kind,
            verdicts.oracle_version,
            kind.name(),
        ));
    }
    // Minimization re-runs both sides per candidate; only queued mismatches
    // pay for it, in parallel like the main run. Records whose signature
    // already has a committed verdict skip it: their repro was minimized
    // when first triaged, so re-minimizing every run is pure cost. Skips are
    // marked, never silently empty.
    let skipped = AtomicUsize::new(0);
    let exhausted = AtomicUsize::new(0);
    let skip_or_minimize = |record: &mut CaseRecord, program: &str, prefix: usize| {
        if committed_verdict(verdicts.as_ref(), record).is_some() {
            skipped.fetch_add(1, Ordering::SeqCst);
            VERDICT_SKIPPED.clone_into(&mut record.minimized_by);
        } else if minimize_record(record, program, prefix, &ctx) {
            exhausted.fetch_add(1, Ordering::SeqCst);
        }
    };
    if serial {
        for (record, program, prefix) in executed
            .iter_mut()
            .filter(|(record, _, _)| record.diff != DiffOutcome::Agree)
        {
            skip_or_minimize(record, program, *prefix);
        }
    } else {
        executed
            .par_iter_mut()
            .filter(|(record, _, _)| record.diff != DiffOutcome::Agree)
            .for_each(|(record, program, prefix)| skip_or_minimize(record, program, *prefix));
    }
    let skipped = skipped.load(Ordering::SeqCst);
    if skipped > 0 {
        println!("minimization: {skipped} skipped (pre-triaged signatures)");
    }
    let exhausted = exhausted.load(Ordering::SeqCst);
    if exhausted > 0 {
        println!(
            "minimization: {exhausted} hit the per-entry budget (best-effort repros, marked {MINIMIZER_EXHAUSTED}; see over-budget report)"
        );
    }
    let records: Vec<CaseRecord> = executed.into_iter().map(|(record, _, _)| record).collect();

    let mut counts = RunCounts {
        total: records.len(),
        excluded: corpus.excluded.len(),
        ..RunCounts::default()
    };
    for record in &records {
        match record.diff {
            DiffOutcome::Agree => counts.agree += 1,
            DiffOutcome::Mismatch => counts.mismatch += 1,
            DiffOutcome::OneSided => counts.one_sided += 1,
        }
    }
    println!(
        "result: {} agree, {} mismatch, {} one-sided ({} total, {} excluded)",
        counts.agree, counts.mismatch, counts.one_sided, counts.total, counts.excluded
    );
    report_over_budget(&records);

    let results = RunResults {
        schema_version: 1,
        tool: "boa_differential".to_owned(),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        updated: utc_now_rfc3339()?,
        oracle: OracleRecord {
            kind: kind.name().to_owned(),
            binary: oracle_path.display().to_string(),
            version: probed.clone(),
        },
        corpus: corpus.name.clone(),
        timeout_secs,
        counts,
        exclusions: corpus
            .excluded
            .iter()
            .map(|exclusion| ExclusionRecord {
                id: exclusion.id.clone(),
                reason: exclusion.reason.to_owned(),
            })
            .collect(),
        cases: records.clone(),
    };
    let results_path = output.join("diff-results.json");
    std::fs::write(&results_path, serde_json::to_string_pretty(&results)?)
        .wrap_err_with(|| eyre!("could not write {}", results_path.display()))?;

    let queue_path = output.join("triage-queue.json");
    merge_queue(&queue_path, &results, verdicts.as_ref())?;
    drop(std::fs::remove_dir_all(&ctx.work_dir));
    println!(
        "wrote {} and {}",
        results_path.display(),
        queue_path.display()
    );
    Ok(())
}

/// Reports queued entries whose minimized test body exceeds the adopted
/// line budget (the pipeline drill requires zero of these).
fn report_over_budget(records: &[CaseRecord]) {
    let over: Vec<(&str, usize)> = records
        .iter()
        .filter(|record| record.diff != DiffOutcome::Agree)
        .filter(|record| record.minimized_lines > MINIMIZED_BODY_BUDGET)
        .map(|record| (record.id.as_str(), record.minimized_lines))
        .collect();
    if over.is_empty() {
        println!("minimized bodies: all within the {MINIMIZED_BODY_BUDGET}-line budget");
    } else {
        println!(
            "minimized bodies: {} OVER the {MINIMIZED_BODY_BUDGET}-line budget:",
            over.len()
        );
        for (id, lines) in over.iter().take(20) {
            println!("  over-budget {id} ({lines} lines)");
        }
    }
}

/// Prints exclusion counts by reason (exclusions are never silent).
fn report_exclusions(corpus: &Corpus) {
    use std::collections::BTreeMap;
    let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();
    for exclusion in &corpus.excluded {
        *by_reason.entry(exclusion.reason).or_default() += 1;
    }
    for (reason, count) in by_reason {
        println!("excluded {count} ({reason})");
    }
}

/// Executes one case on both sides and compares (no minimization yet).
///
/// Returns the record plus the original program and its harness prefix
/// length (minimization input; the record persists only the minimized form).
fn execute_case(case: &Case, ctx: &RunContext) -> Result<(CaseRecord, String, usize)> {
    let composed = compose::compose(&case.program);
    let staged = ctx.stage(&case.id, &composed)?;
    // A Boa child that exits 0 but prints no valid outcome line (e.g. its
    // single JSON line was cut by the supervisor's bounded capture) is a
    // case-level harness failure, never a run abort: one unobservable case
    // must not kill a 6k-case run.
    let boa = match run_boa_child(&staged, ctx) {
        Ok(boa) => boa,
        Err(err) => {
            let oracle = run_oracle(ctx.kind, &ctx.oracle, &staged, ctx.timeout_secs);
            drop(std::fs::remove_file(&staged));
            let text = format!("{err:#}");
            let outcome = BoaCaseOutcome {
                outcome: TestOutcomeResult::HarnessError,
                text,
                observation: None,
            };
            return Ok((
                compare_sides(case, outcome, oracle),
                case.program.clone(),
                case.prefix_lines,
            ));
        }
    };
    let oracle = run_oracle(ctx.kind, &ctx.oracle, &staged, ctx.timeout_secs);
    drop(std::fs::remove_file(&staged));
    Ok((
        compare_sides(case, boa, oracle),
        case.program.clone(),
        case.prefix_lines,
    ))
}

/// Runs the Boa side through a supervised `run-case` child.
fn run_boa_child(staged: &Path, ctx: &RunContext) -> Result<BoaCaseOutcome> {
    let child = supervise(
        &ctx.own_exe,
        &[
            "run-case".as_ref(),
            "--program".as_ref(),
            staged.as_os_str(),
        ],
        ctx.timeout_secs,
    );
    if !matches!(child.end, ChildEnd::Exited(0)) {
        let (outcome, text) = classify_boa_child(&child);
        return Ok(BoaCaseOutcome {
            outcome,
            text,
            observation: None,
        });
    }
    serde_json::from_str(child.stdout.trim()).wrap_err("boa child printed no valid outcome line")
}

/// Compares both sides into a [`CaseRecord`] (minimization filled later).
fn compare_sides(case: &Case, boa: BoaCaseOutcome, oracle: OracleOutcome) -> CaseRecord {
    let (diff, field_diffs) = match (&boa.observation, &oracle.observation) {
        (Some(boa_obs), Some(oracle_obs)) => {
            let diffs = compare(boa_obs, oracle_obs);
            if diffs.is_empty() {
                (DiffOutcome::Agree, diffs)
            } else {
                (DiffOutcome::Mismatch, diffs)
            }
        }
        _ => (DiffOutcome::OneSided, Vec::new()),
    };
    CaseRecord {
        id: case.id.clone(),
        origin: case.origin.to_owned(),
        area: area_of(&case.id),
        boa_outcome: boa.outcome,
        oracle_outcome: oracle.outcome,
        diff,
        field_diffs,
        boa_text: truncate_text(boa.text, 1000),
        oracle_text: truncate_text(oracle.text, 1000),
        minimized_program: String::new(),
        minimized_by: String::new(),
        minimized_lines: 0,
        boa_observation: boa.observation,
        oracle_observation: oracle.observation,
    }
}

/// Minimizes a queued record's test body in place (kind-preserving).
///
/// The harness `prefix_lines` stay fixed; `minimized_lines` counts the
/// minimized body only (the budgeted quantity).
/// Minimizer id recorded when minimization is skipped: the record's
/// signature already has a committed verdict, so its repro was minimized
/// when first triaged. Skipped records keep an empty program with
/// `minimized_lines == 0`, which trivially satisfies the budget report.
const VERDICT_SKIPPED: &str = "verdict-skipped-v1";

/// Per-entry minimization budget: 120s wall-clock or 10k probes, whichever
/// binds first. Typical entries take seconds (Array: ~3-10s on this path),
/// so 120s is 12-40x headroom and 10k probes ~100x; in production the clock
/// always binds first, and the count is a test hook plus freak backstop.
/// Slow-converging entries would otherwise stall a whole run on one thread
/// (observed: 7k+ probes and counting on a 1317-line generated test).
const MAX_MINIMIZE_CANDIDATES: usize = 10_000;
/// Wall-clock half of the per-entry minimization budget (see above).
const MINIMIZE_TIME_BUDGET: Duration = Duration::from_secs(120);

/// Minimizer id recorded when the per-entry budget ran out: best-so-far
/// repro, still fully usable for signature triage. Loud by construction —
/// this mark plus the run's exhaustion count plus the over-budget report.
const MINIMIZER_EXHAUSTED: &str = "line-reduce-v2-exhausted";

/// Committed verdict matching `record`'s signature, if any. Shared by the
/// minimization skip and [`merge_queue`]'s preload so the two can never
/// disagree about what counts as pre-triaged.
fn committed_verdict<'v>(
    committed: Option<&'v VerdictFile>,
    record: &CaseRecord,
) -> Option<&'v VerdictRecord> {
    committed.and_then(|committed| {
        committed.lookup(
            &record.id,
            record.boa_outcome,
            record.oracle_outcome,
            &record
                .field_diffs
                .iter()
                .map(|diff| diff.field.clone())
                .collect::<Vec<_>>(),
        )
    })
}

/// Minimizes one queued record; returns true when the per-entry budget ran
/// out (best-so-far kept, marked [`MINIMIZER_EXHAUSTED`]).
fn minimize_record(
    record: &mut CaseRecord,
    program: &str,
    prefix_lines: usize,
    ctx: &RunContext,
) -> bool {
    let want = record.diff;
    let mut budget = Budget::new(MAX_MINIMIZE_CANDIDATES, MINIMIZE_TIME_BUDGET);
    let (minimized, body_lines, exhausted) = minimize_with_prefix(
        program,
        prefix_lines,
        &|candidate| {
            let Ok(staged) = ctx.stage(&record.id, &compose::compose(candidate)) else {
                return false;
            };
            let Ok(boa) = run_boa_child(&staged, ctx) else {
                drop(std::fs::remove_file(&staged));
                return false;
            };
            let oracle = run_oracle(ctx.kind, &ctx.oracle, &staged, ctx.timeout_secs);
            drop(std::fs::remove_file(&staged));
            match (&boa.observation, &oracle.observation) {
                (Some(boa_obs), Some(oracle_obs)) => {
                    want == DiffOutcome::Mismatch && !compare(boa_obs, oracle_obs).is_empty()
                }
                _ => want == DiffOutcome::OneSided,
            }
        },
        &mut budget,
    );
    record.minimized_lines = body_lines;
    minimized.clone_into(&mut record.minimized_program);
    if exhausted {
        MINIMIZER_EXHAUSTED.clone_into(&mut record.minimized_by);
    } else {
        "line-reduce-v2".clone_into(&mut record.minimized_by);
    }
    exhausted
}

/// Merges fresh mismatches into the triage queue, carrying verdicts forward
/// only across identical diffs.
fn merge_queue(
    queue_path: &Path,
    results: &RunResults,
    committed: Option<&VerdictFile>,
) -> Result<()> {
    let mut queue = if queue_path.exists() {
        let existing = TriageQueue::load(queue_path).map_err(|err| eyre!("{err}"))?;
        if existing.oracle.kind != results.oracle.kind
            || existing.oracle.version != results.oracle.version
        {
            println!(
                "queue oracle ({} {}) differs from this run ({} {}): starting fresh, {} prior verdict(s) dropped",
                existing.oracle.kind,
                existing.oracle.version,
                results.oracle.kind,
                results.oracle.version,
                existing.entries.len(),
            );
            TriageQueue::empty(results.oracle.clone(), &results.corpus)
        } else {
            existing
        }
    } else {
        TriageQueue::empty(results.oracle.clone(), &results.corpus)
    };

    let mut carried = 0;
    let mut reopened = 0;
    let mut fresh = 0;
    let mut pre_triaged = 0;
    let mut next = std::collections::BTreeMap::new();
    for record in results
        .cases
        .iter()
        .filter(|record| record.diff != DiffOutcome::Agree)
    {
        let mut verdict = queue.entries.get(&record.id).and_then(|prior| {
            if prior.boa_outcome == record.boa_outcome
                && prior.oracle_outcome == record.oracle_outcome
                && prior.field_diffs == record.field_diffs
            {
                carried += 1;
                prior.verdict.clone()
            } else {
                reopened += 1;
                None
            }
        });
        if !queue.entries.contains_key(&record.id) {
            fresh += 1;
        }
        // Committed verdicts pre-triage identical fresh signatures (CI path).
        if verdict.is_none()
            && let Some(preload) = committed_verdict(committed, record)
        {
            verdict = Some(preload.to_verdict().map_err(|err| eyre!("{err}"))?);
            pre_triaged += 1;
        }
        next.insert(
            record.id.clone(),
            QueueEntry {
                id: record.id.clone(),
                origin: record.origin.clone(),
                area: record.area.clone(),
                boa_outcome: record.boa_outcome,
                oracle_outcome: record.oracle_outcome,
                boa_text: record.boa_text.clone(),
                oracle_text: record.oracle_text.clone(),
                field_diffs: record.field_diffs.clone(),
                minimized_program: record.minimized_program.clone(),
                minimized_by: record.minimized_by.clone(),
                minimized_lines: record.minimized_lines,
                boa_observation: record.boa_observation.clone(),
                oracle_observation: record.oracle_observation.clone(),
                verdict,
            },
        );
    }
    let resolved = queue
        .entries
        .len()
        .saturating_sub(next.len().saturating_sub(fresh));
    queue.entries = next;
    queue.corpus.clone_from(&results.corpus);
    queue.save(queue_path).map_err(|err| eyre!("{err}"))?;
    println!(
        "queue: {} entries ({} fresh, {} carried, {} reopened, {} resolved, {} pre-triaged)",
        queue.entries.len(),
        fresh,
        carried,
        reopened,
        resolved,
        pre_triaged,
    );
    if let Some(committed) = committed {
        // Only verdicts for cases this run EXECUTED count as stale: a
        // sub-corpus run must never advise pruning verdicts it never ran
        // (following that advice would gut the committed file).
        let executed: std::collections::BTreeSet<&str> = results
            .cases
            .iter()
            .map(|record| record.id.as_str())
            .collect();
        let stale = committed
            .verdicts
            .iter()
            .filter(|record| {
                executed.contains(record.id.as_str()) && !queue.entries.contains_key(&record.id)
            })
            .count();
        if stale != 0 {
            println!(
                "note: {stale} committed verdict(s) no longer mismatch (prune the verdicts file)"
            );
        }
    }
    Ok(())
}

/// Implements the `triage` subcommand.
#[allow(clippy::too_many_arguments)]
fn triage_cmd(
    queue_path: &Path,
    check: bool,
    list: bool,
    reset_oracle_quirks: bool,
    export: Option<&Path>,
    area: Option<&str>,
    pattern: Option<&str>,
    fields: Option<&str>,
    set_verdict: Option<&str>,
    spec: Option<&str>,
    regression: Option<&str>,
    note: Option<&str>,
    filing: Option<&str>,
    expiry: Option<&str>,
) -> Result<()> {
    let mut queue = TriageQueue::load(queue_path).map_err(|err| eyre!("{err}"))?;
    if let Some(kind) = set_verdict {
        if area.is_none() && pattern.is_none() && fields.is_none() {
            return Err(eyre!(
                "--set-verdict requires --area and/or --pattern and/or --fields"
            ));
        }
        let verdict = build_verdict(kind, &queue, spec, regression, note, filing, expiry)?;
        let wanted: Option<Vec<String>> = fields.map(|fields| {
            fields
                .split(',')
                .map(|field| field.trim().to_owned())
                .collect()
        });
        let matched = queue.set_verdict(area, pattern, wanted.as_deref(), &verdict);
        println!("verdict applied to {matched} entries");
        queue.save(queue_path).map_err(|err| eyre!("{err}"))?;
    }
    if reset_oracle_quirks {
        let reset = queue.reset_oracle_quirks();
        println!("reset {reset} oracle-quirk verdict(s) to untriaged");
        queue.save(queue_path).map_err(|err| eyre!("{err}"))?;
    }
    if let Some(export) = export {
        let file = VerdictFile::export(&queue);
        file.save(export).map_err(|err| eyre!("{err}"))?;
        println!(
            "exported {} verdict(s) to {}",
            file.verdicts.len(),
            export.display()
        );
    }
    if list {
        for entry in queue.entries.values() {
            println!(
                "{} [{}] {}",
                entry.id,
                entry.area,
                verdict_word(entry.verdict.as_ref())
            );
        }
    }
    report_queue_depth(&queue);
    if check {
        let untriaged = queue.untriaged();
        if untriaged.is_empty() {
            println!("triage gate: clean (zero untriaged)");
        } else {
            println!("triage gate: BREACH ({} untriaged):", untriaged.len());
            for id in untriaged.iter().take(50) {
                println!("  untriaged {id}");
            }
            if untriaged.len() > 50 {
                println!("  ... and {} more", untriaged.len() - 50);
            }
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Builds a [`Verdict`] from CLI flags, validating required fields per kind.
fn build_verdict(
    kind: &str,
    queue: &TriageQueue,
    spec: Option<&str>,
    regression: Option<&str>,
    note: Option<&str>,
    filing: Option<&str>,
    expiry: Option<&str>,
) -> Result<Verdict> {
    match kind {
        "boa-bug" => Ok(Verdict::BoaBug {
            spec: spec
                .ok_or_else(|| eyre!("boa-bug requires --spec <tc39.es URL>"))?
                .to_owned(),
            regression: regression.unwrap_or_default().to_owned(),
        }),
        "oracle-quirk" => Ok(Verdict::OracleQuirk {
            oracle: format!("{} {}", queue.oracle.kind, queue.oracle.version),
            note: note
                .ok_or_else(|| eyre!("oracle-quirk requires --note"))?
                .to_owned(),
        }),
        "spec-ambiguity" => Ok(Verdict::SpecAmbiguity {
            spec: spec
                .ok_or_else(|| eyre!("spec-ambiguity requires --spec <tc39.es URL>"))?
                .to_owned(),
            filing: filing.unwrap_or_default().to_owned(),
            expiry: expiry
                .ok_or_else(|| eyre!("spec-ambiguity requires --expiry <date|re-review:ref>"))?
                .to_owned(),
        }),
        _ => Err(eyre!("unknown verdict kind {kind:?}")),
    }
}

/// One-word verdict rendering for `--list` and depth reports.
fn verdict_word(verdict: Option<&Verdict>) -> &'static str {
    match verdict {
        None => "untriaged",
        Some(Verdict::BoaBug { .. }) => "boa-bug",
        Some(Verdict::OracleQuirk { .. }) => "oracle-quirk",
        Some(Verdict::SpecAmbiguity { .. }) => "spec-ambiguity",
    }
}

/// Prints the queue-depth metric (total + per-verdict counts).
fn report_queue_depth(queue: &TriageQueue) {
    use std::collections::BTreeMap;
    let mut by_verdict: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in queue.entries.values() {
        *by_verdict
            .entry(verdict_word(entry.verdict.as_ref()))
            .or_default() += 1;
    }
    println!(
        "queue_depth total={} corpus={}",
        queue.entries.len(),
        queue.corpus
    );
    for (verdict, count) in by_verdict {
        println!("queue_depth {verdict}={count}");
    }
}

/// Truncates side summaries for records (observations carry the detail).
fn truncate_text(text: String, max: usize) -> String {
    if text.len() <= max {
        text
    } else {
        text.chars().take(max).collect()
    }
}

/// Current UTC time formatted as RFC 3339.
fn utc_now_rfc3339() -> Result<String> {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .wrap_err("could not format the current time")
}
