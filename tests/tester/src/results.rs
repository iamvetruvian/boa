use crate::{Statistics, TestOutcomeResult, VersionedStats, crash::CrashRecord};

use super::SuiteResult;
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{BufReader, BufWriter},
    path::Path,
    process::Command,
};

/// Structure to store full result information.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ResultInfo {
    #[serde(rename = "c")]
    pub(crate) commit: Box<str>,
    #[serde(rename = "u")]
    pub(crate) test262_commit: Box<str>,
    #[serde(rename = "r")]
    pub(crate) results: SuiteResult,
}

/// Structure to store full result information.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct ReducedResultInfo {
    #[serde(rename = "c")]
    commit: Box<str>,
    #[serde(rename = "u")]
    test262_commit: Box<str>,
    #[serde(rename = "a")]
    stats: Statistics,
    #[serde(rename = "av", default)]
    versioned_stats: VersionedStats,
}

impl From<ResultInfo> for ReducedResultInfo {
    /// Creates a new reduced suite result from a full suite result.
    fn from(info: ResultInfo) -> Self {
        Self {
            commit: info.commit,
            test262_commit: info.test262_commit,
            stats: info.results.stats,
            versioned_stats: info.results.versioned_stats,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct FeaturesInfo {
    #[serde(rename = "c")]
    commit: Box<str>,
    #[serde(rename = "u")]
    test262_commit: Box<str>,
    #[serde(rename = "n")]
    suite_name: Box<str>,
    #[serde(rename = "f")]
    features: FxHashSet<String>,
}

impl From<ResultInfo> for FeaturesInfo {
    fn from(info: ResultInfo) -> Self {
        Self {
            commit: info.commit,
            test262_commit: info.test262_commit,
            suite_name: info.results.name,
            features: info.results.features,
        }
    }
}

/// File name of the "latest results" JSON file.
pub(crate) const LATEST_FILE_NAME: &str = "latest.json";

/// File name of the "all results" JSON file.
const RESULTS_FILE_NAME: &str = "results.json";

/// File name of the "features" JSON file.
const FEATURES_FILE_NAME: &str = "features.json";

/// File name of the crash-accounting JSON file.
pub(crate) const CRASHES_FILE_NAME: &str = "crashes.json";

/// Envelope for `crashes.json`, following the P1.1 shared JSON-store
/// conventions (schema version, tool identity, UTC timestamp).
#[derive(Debug, Clone, Serialize)]
struct CrashesFile<'a> {
    schema_version: u8,
    tool: &'a str,
    tool_version: &'a str,
    updated: String,
    records: &'a [CrashRecord],
}

/// Writes the results of running the test suite to the given JSON output file.
///
/// It will append the results to the ones already present, in an array.
pub(crate) fn write_json(
    results: SuiteResult,
    crashes: &[CrashRecord],
    output_dir: &Path,
    verbose: u8,
    test262_path: &Path,
) -> Result<()> {
    let mut branch = env::var("GITHUB_REF").unwrap_or_default();
    if branch.starts_with("refs/pull") {
        "pull".clone_into(&mut branch);
    }

    let output_dir = if branch.is_empty() {
        output_dir.to_path_buf()
    } else {
        let folder = output_dir.join(branch);
        fs::create_dir_all(&folder)?;
        folder
    };

    if verbose != 0 {
        println!("Writing the results to {}...", output_dir.display());
    }

    // Write the latest results.

    let latest = output_dir.join(LATEST_FILE_NAME);

    let new_results = ResultInfo {
        commit: env::var("GITHUB_SHA").unwrap_or_default().into_boxed_str(),
        test262_commit: get_test262_commit(test262_path)?,
        results,
    };

    let latest = BufWriter::new(fs::File::create(latest)?);
    serde_json::to_writer(latest, &new_results)?;

    // Write the full list of results, retrieving the existing ones first.

    let all_path = output_dir.join(RESULTS_FILE_NAME);

    let mut all_results: Vec<ReducedResultInfo> = if all_path.exists() {
        serde_json::from_reader(BufReader::new(fs::File::open(&all_path)?))?
    } else {
        Vec::new()
    };

    all_results.push(new_results.clone().into());

    let output = BufWriter::new(fs::File::create(&all_path)?);
    serde_json::to_writer(output, &all_results)?;

    if verbose != 0 {
        println!("Results written correctly");
    }

    // Write the full list of features, existing features go first.

    let features = output_dir.join(FEATURES_FILE_NAME);

    let mut all_features: Vec<FeaturesInfo> = if features.exists() {
        serde_json::from_reader(BufReader::new(fs::File::open(&features)?))?
    } else {
        Vec::new()
    };

    all_features.push(new_results.into());

    let features = BufWriter::new(fs::File::create(&features)?);
    serde_json::to_writer(features, &all_features)?;

    if verbose != 0 {
        println!("Features written correctly");
    }

    // Write the crash-accounting file (always, even when empty: zero
    // abnormal outcomes is itself a result worth recording).
    let crashes_path = output_dir.join(CRASHES_FILE_NAME);
    let crashes_file = CrashesFile {
        schema_version: 1,
        tool: "boa_tester",
        tool_version: env!("CARGO_PKG_VERSION"),
        updated: utc_now_rfc3339()?,
        records: crashes,
    };
    let crashes_out = BufWriter::new(fs::File::create(&crashes_path)?);
    serde_json::to_writer(crashes_out, &crashes_file)?;

    if verbose != 0 {
        println!("Crashes written correctly");
    }

    Ok(())
}

/// Current UTC time formatted as RFC 3339.
pub(crate) fn utc_now_rfc3339() -> Result<String> {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .wrap_err("could not format the current time")
}

/// Gets the commit OID of the Test262 checkout.
fn get_test262_commit(test262_path: &Path) -> Result<Box<str>> {
    // Prefer `git rev-parse` so detached checkouts, worktrees, and pins that
    // moved `refs/heads/main` all report the actually checked-out commit.
    if let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(test262_path)
        .args(["rev-parse", "HEAD"])
        .output()
        && output.status.success()
    {
        let commit = String::from_utf8_lossy(&output.stdout);
        let commit = commit.trim();
        if !commit.is_empty() {
            return Ok(commit.into());
        }
    }

    // Fall back to the historical `main` ref for checkouts whose git metadata
    // is readable but the `git` binary is unavailable.
    let main_head_path = test262_path.join(".git/refs/heads/main");
    let commit_id =
        fs::read_to_string(&main_head_path).wrap_err("could not determine the Test262 commit")?;
    Ok(commit_id.trim().into())
}

/// Fail conditions for `compare --fail-on`.
///
/// Each condition names a set of comparison findings that breach the run
/// (exit 1). With no `--fail-on` flag, `compare` stays advisory (exit 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub(crate) enum FailCondition {
    /// Any `Failed` outcome in the new results.
    Failed,
    /// Any `Panic` outcome in the new results.
    Panic,
    /// Any `Timeout` outcome in the new results.
    Timeout,
    /// Any `Crash` outcome in the new results.
    Crash,
    /// Any `HarnessError` outcome in the new results.
    HarnessError,
    /// Any newly skipped test (not ignored in base, ignored in new).
    NewSkip,
    /// Any test that passed in base but no longer passes.
    Broken,
    /// No-regression shorthand: `broken` + newly abnormal/newly skipped.
    Regression,
    /// Any failed/abnormal outcome whose id is not in `--triage`.
    Untriaged,
    /// Any base test id missing from the new results.
    Vanished,
}

/// Per-test outcome keyed by stable test id.
pub(crate) type OutcomeMap = FxHashMap<String, TestOutcomeResult>;

/// One categorized per-test finding: a stable id plus human context.
#[derive(Debug, Clone)]
struct Finding {
    id: String,
    detail: String,
}

impl Finding {
    /// Renders the finding for reports.
    fn display(&self) -> String {
        format!("{} {}", self.id, self.detail)
    }
}

/// Categorized comparison between two runs.
#[derive(Debug, Default)]
struct CompareOutcome {
    fixed: Vec<Finding>,
    broken: Vec<Finding>,
    new_panics: Vec<Finding>,
    panic_fixes: Vec<Finding>,
    new_timeouts: Vec<Finding>,
    timeout_fixes: Vec<Finding>,
    new_crashes: Vec<Finding>,
    crash_fixes: Vec<Finding>,
    new_harness_errors: Vec<Finding>,
    new_skips: Vec<Finding>,
    new_only: Vec<String>,
    vanished: Vec<String>,
    new_signatures: Vec<String>,
    fixed_signatures: Vec<String>,
    /// `(id, outcome)` for every failed/abnormal outcome in `new`.
    bad_in_new: Vec<(String, TestOutcomeResult)>,
}

/// Compares the results of two test suite runs.
///
/// Prints the conformance tables and per-test differences, then evaluates
/// `fail_on`. Returns `true` when a fail condition breached (the caller
/// exits nonzero); with empty `fail_on` the comparison is advisory.
#[allow(clippy::cast_possible_wrap, clippy::too_many_arguments)]
pub(crate) fn compare_results(
    base: &Path,
    new: &Path,
    markdown: bool,
    fail_on: &[FailCondition],
    quarantine_path: Option<&Path>,
    triage_path: Option<&Path>,
) -> Result<bool> {
    // If the path is a directory, use latest.json from that directory
    let base_path = if base.is_dir() {
        base.join(LATEST_FILE_NAME)
    } else {
        base.to_path_buf()
    };

    let new_path = if new.is_dir() {
        new.join(LATEST_FILE_NAME)
    } else {
        new.to_path_buf()
    };

    let base_results: ResultInfo = serde_json::from_reader(BufReader::new(
        fs::File::open(&base_path).wrap_err("could not open the base results file")?,
    ))
    .wrap_err("could not read the base results")?;

    let new_results: ResultInfo = serde_json::from_reader(BufReader::new(
        fs::File::open(&new_path).wrap_err("could not open the new results file")?,
    ))
    .wrap_err("could not read the new results")?;

    let base_total = base_results.results.stats.total as isize;
    let new_total = new_results.results.stats.total as isize;
    let total_diff = new_total - base_total;

    let base_passed = base_results.results.stats.passed as isize;
    let new_passed = new_results.results.stats.passed as isize;
    let passed_diff = new_passed - base_passed;

    let base_ignored = base_results.results.stats.ignored as isize;
    let new_ignored = new_results.results.stats.ignored as isize;
    let ignored_diff = new_ignored - base_ignored;

    let base_failed = base_results.results.stats.failed() as isize;
    let new_failed = new_results.results.stats.failed() as isize;
    let failed_diff = new_failed - base_failed;

    let base_panics = base_results.results.stats.panic as isize;
    let new_panics = new_results.results.stats.panic as isize;
    let panic_diff = new_panics - base_panics;

    let base_timeouts = base_results.results.stats.timeout as isize;
    let new_timeouts = new_results.results.stats.timeout as isize;
    let timeout_diff = new_timeouts - base_timeouts;

    let base_crashes = base_results.results.stats.crash as isize;
    let new_crashes = new_results.results.stats.crash as isize;
    let crash_diff = new_crashes - base_crashes;

    let base_harness = base_results.results.stats.harness_error as isize;
    let new_harness = new_results.results.stats.harness_error as isize;
    let harness_diff = new_harness - base_harness;

    let base_conformance = (base_passed as f64 / base_total as f64) * 100_f64;
    let new_conformance = (new_passed as f64 / new_total as f64) * 100_f64;
    let conformance_diff = new_conformance - base_conformance;

    let mut base_map = OutcomeMap::default();
    flatten_suite(&base_results.results, &mut base_map);
    let mut new_map = OutcomeMap::default();
    flatten_suite(&new_results.results, &mut new_map);
    let mut outcome = diff_outcomes(&base_map, &new_map);
    diff_crash_signatures(base, new, &mut outcome);

    let quarantine = load_quarantine(quarantine_path)?;
    let triaged = load_triaged(triage_path)?;
    let breaches = evaluate_fail_conditions(&outcome, fail_on, &quarantine, &triaged);
    let quarantined_exempt = count_quarantined(&outcome, &quarantine);

    if markdown {
        /// Simple function to add commas as thousands separator for integers.
        fn pretty_int(i: isize) -> String {
            let mut res = String::new();

            for (idx, val) in i.abs().to_string().chars().rev().enumerate() {
                if idx != 0 && idx % 3 == 0 {
                    res.insert(0, ',');
                }
                res.insert(0, val);
            }
            res
        }

        /// Generates a proper diff format, with some bold text if things change.
        fn diff_format(diff: isize) -> String {
            format!(
                "{}{}{}{}",
                if diff == 0 { "" } else { "**" },
                if diff.is_positive() {
                    "+"
                } else if diff.is_negative() {
                    "-"
                } else {
                    ""
                },
                pretty_int(diff),
                if diff == 0 { "" } else { "**" }
            )
        }

        println!("| Test result | main count | PR count | difference |");
        println!("| :---------: | :----------: | :------: | :--------: |");
        println!(
            "| Total | {} | {} | {} |",
            pretty_int(base_total),
            pretty_int(new_total),
            diff_format(total_diff),
        );
        println!(
            "| Passed | {} | {} | {} |",
            pretty_int(base_passed),
            pretty_int(new_passed),
            diff_format(passed_diff),
        );
        println!(
            "| Ignored | {} | {} | {} |",
            pretty_int(base_ignored),
            pretty_int(new_ignored),
            diff_format(ignored_diff),
        );
        println!(
            "| Failed | {} | {} | {} |",
            pretty_int(base_failed),
            pretty_int(new_failed),
            diff_format(failed_diff),
        );
        println!(
            "| Panics | {} | {} | {} |",
            pretty_int(base_panics),
            pretty_int(new_panics),
            diff_format(panic_diff),
        );
        println!(
            "| Timeouts | {} | {} | {} |",
            pretty_int(base_timeouts),
            pretty_int(new_timeouts),
            diff_format(timeout_diff),
        );
        println!(
            "| Crashes | {} | {} | {} |",
            pretty_int(base_crashes),
            pretty_int(new_crashes),
            diff_format(crash_diff),
        );
        println!(
            "| Harness errors | {} | {} | {} |",
            pretty_int(base_harness),
            pretty_int(new_harness),
            diff_format(harness_diff),
        );
        println!(
            "| Conformance | {:.2}% | {:.2}% | {}{}{:.2}%{} |",
            base_conformance,
            new_conformance,
            if conformance_diff.abs() > f64::EPSILON {
                "**"
            } else {
                ""
            },
            if conformance_diff > 0_f64 { "+" } else { "" },
            conformance_diff,
            if conformance_diff.abs() > f64::EPSILON {
                "**"
            } else {
                ""
            },
        );

        print_details_markdown("Fixed tests", &outcome.fixed);
        print_details_markdown("Broken tests", &outcome.broken);
        print_details_markdown("New panics", &outcome.new_panics);
        print_details_markdown("Fixed panics", &outcome.panic_fixes);
        print_details_markdown("New timeouts", &outcome.new_timeouts);
        print_details_markdown("Fixed timeouts", &outcome.timeout_fixes);
        print_details_markdown("New crashes", &outcome.new_crashes);
        print_details_markdown("Fixed crashes", &outcome.crash_fixes);
        print_details_markdown("New harness errors", &outcome.new_harness_errors);
        print_details_markdown("New skips", &outcome.new_skips);
        print_details_markdown(
            "New crash signatures",
            &plain_findings(&outcome.new_signatures),
        );
        print_details_markdown(
            "Fixed crash signatures",
            &plain_findings(&outcome.fixed_signatures),
        );
    } else {
        println!("Test262 conformance changes:");
        println!("| Test result  | main  |  PR   | difference |");
        println!("|    Total     | {base_total:^6} | {new_total:^5} | {total_diff:^10} |");
        println!("|    Passed    | {base_passed:^6} | {new_passed:^5} | {passed_diff:^10} |");
        println!("|   Ignored    | {base_ignored:^6} | {new_ignored:^5} | {ignored_diff:^10} |");
        println!("|    Failed    | {base_failed:^6} | {new_failed:^5} | {failed_diff:^10} |");
        println!("|    Panics    | {base_panics:^6} | {new_panics:^5} | {panic_diff:^10} |");
        println!("|   Timeouts   | {base_timeouts:^6} | {new_timeouts:^5} | {timeout_diff:^10} |");
        println!("|   Crashes    | {base_crashes:^6} | {new_crashes:^5} | {crash_diff:^10} |");
        println!("| Harness errs | {base_harness:^6} | {new_harness:^5} | {harness_diff:^10} |");

        print_details_text("Fixed tests", &outcome.fixed);
        print_details_text("Broken tests", &outcome.broken);
        print_details_text("New panics", &outcome.new_panics);
        print_details_text("Fixed panics", &outcome.panic_fixes);
        print_details_text("New timeouts", &outcome.new_timeouts);
        print_details_text("Fixed timeouts", &outcome.timeout_fixes);
        print_details_text("New crashes", &outcome.new_crashes);
        print_details_text("Fixed crashes", &outcome.crash_fixes);
        print_details_text("New harness errors", &outcome.new_harness_errors);
        print_details_text("New skips", &outcome.new_skips);
        print_details_text(
            "New crash signatures",
            &plain_findings(&outcome.new_signatures),
        );
        print_details_text(
            "Fixed crash signatures",
            &plain_findings(&outcome.fixed_signatures),
        );
        if !outcome.new_only.is_empty() || !outcome.vanished.is_empty() {
            println!();
            println!(
                "New-only tests: {}; vanished tests: {} (use --fail-on=vanished to gate).",
                outcome.new_only.len(),
                outcome.vanished.len()
            );
        }
    }

    if quarantined_exempt != 0 {
        println!("Quarantined (exempt from fail conditions): {quarantined_exempt}");
    }
    if breaches.is_empty() {
        if fail_on.is_empty() {
            println!("Advisory comparison: no --fail-on conditions given.");
        } else {
            println!("No fail conditions breached.");
        }
        Ok(false)
    } else {
        for breach in &breaches {
            println!("BREACH: {breach}");
        }
        Ok(true)
    }
}

/// Maximum per-test entries printed per category (lists past this print a
/// count instead, so subset-vs-full comparisons stay readable).
const MAX_PRINTED: usize = 50;

/// Flattens a suite tree into stable-test-id → outcome entries.
///
/// Ids join suite names with `/` (e.g. `test/built-ins/Array/from`), matching
/// the ids the runner assigns, so subset runs compare against full baselines.
pub(crate) fn flatten_suite(suite: &SuiteResult, out: &mut OutcomeMap) {
    flatten_suite_inner(suite, "", out);
}

/// Recursive helper for [`flatten_suite`].
fn flatten_suite_inner(suite: &SuiteResult, prefix: &str, out: &mut OutcomeMap) {
    let here = if prefix.is_empty() {
        suite.name.to_string()
    } else {
        format!("{prefix}/{}", suite.name)
    };
    for test in &suite.tests {
        out.insert(format!("{here}/{}", test.name), test.result);
    }
    for child in &suite.suites {
        flatten_suite_inner(child, &here, out);
    }
}

/// Diffs two flattened outcome maps into categorized findings.
fn diff_outcomes(base: &OutcomeMap, new: &OutcomeMap) -> CompareOutcome {
    use super::TestOutcomeResult;

    let mut outcome = CompareOutcome::default();

    for (id, new_result) in new {
        let Some(base_result) = base.get(id) else {
            outcome.new_only.push(id.clone());
            continue;
        };
        let base_result = *base_result;
        if base_result == *new_result {
            continue;
        }
        let was = Finding {
            id: id.clone(),
            detail: format!("(previously {base_result:?})"),
        };
        let now = Finding {
            id: id.clone(),
            detail: format!("(now {new_result:?})"),
        };
        match (*new_result, base_result) {
            (TestOutcomeResult::Passed, _) => outcome.fixed.push(was.clone()),
            (TestOutcomeResult::Ignored, _) => outcome.new_skips.push(was.clone()),
            // Only bad outcomes reach here (`Passed`/`Ignored` matched above).
            (_, TestOutcomeResult::Ignored) => outcome.broken.push(Finding {
                id: id.clone(),
                detail: format!("(now {new_result:?}, previously Ignored)"),
            }),
            (TestOutcomeResult::Failed, _) | (_, TestOutcomeResult::Passed) => {
                outcome.broken.push(now.clone());
            }
            _ => {}
        }
        if *new_result == TestOutcomeResult::Panic {
            outcome.new_panics.push(was.clone());
        }
        if base_result == TestOutcomeResult::Panic {
            outcome.panic_fixes.push(now.clone());
        }
        if *new_result == TestOutcomeResult::Timeout {
            outcome.new_timeouts.push(was.clone());
        }
        if base_result == TestOutcomeResult::Timeout {
            outcome.timeout_fixes.push(now.clone());
        }
        if *new_result == TestOutcomeResult::Crash {
            outcome.new_crashes.push(was.clone());
        }
        if base_result == TestOutcomeResult::Crash {
            outcome.crash_fixes.push(now.clone());
        }
        if *new_result == TestOutcomeResult::HarnessError {
            outcome.new_harness_errors.push(was);
        }
    }

    for id in base.keys() {
        if !new.contains_key(id) {
            outcome.vanished.push(id.clone());
        }
    }

    for (id, result) in new {
        if *result == TestOutcomeResult::Failed || result.is_abnormal() {
            outcome.bad_in_new.push((id.clone(), *result));
        }
    }

    for findings in [
        &mut outcome.fixed,
        &mut outcome.broken,
        &mut outcome.new_panics,
        &mut outcome.panic_fixes,
        &mut outcome.new_timeouts,
        &mut outcome.timeout_fixes,
        &mut outcome.new_crashes,
        &mut outcome.crash_fixes,
        &mut outcome.new_harness_errors,
        &mut outcome.new_skips,
    ] {
        findings.sort_by(|a, b| a.id.cmp(&b.id));
    }
    outcome.new_only.sort();
    outcome.vanished.sort();
    outcome.bad_in_new.sort_by(|a, b| a.0.cmp(&b.0));
    outcome
}

/// Diffs crash signatures from sibling `crashes.json` files when both sides
/// have one. Missing files skip the signature diff (noted, never failing).
fn diff_crash_signatures(base: &Path, new: &Path, outcome: &mut CompareOutcome) {
    let base_sigs = load_signatures(base);
    let new_sigs = load_signatures(new);
    match (base_sigs, new_sigs) {
        (Some(base_sigs), Some(new_sigs)) => {
            let mut added: Vec<String> = new_sigs.difference(&base_sigs).cloned().collect();
            let mut fixed: Vec<String> = base_sigs.difference(&new_sigs).cloned().collect();
            added.sort();
            fixed.sort();
            outcome.new_signatures = added;
            outcome.fixed_signatures = fixed;
        }
        (None, None) => {}
        _ => println!("note: crash-signature diff skipped (crashes.json missing on one side)"),
    }
}

/// Loads crash signatures from a `crashes.json` next to `path`.
fn load_signatures(path: &Path) -> Option<FxHashSet<String>> {
    #[derive(Deserialize)]
    struct Signatures {
        #[serde(default)]
        records: Vec<SignatureRecord>,
    }

    #[derive(Deserialize)]
    struct SignatureRecord {
        #[serde(default)]
        signature: String,
    }

    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let file = dir.join(CRASHES_FILE_NAME);
    if !file.exists() {
        return None;
    }
    let parsed: Signatures =
        serde_json::from_reader(BufReader::new(fs::File::open(file).ok()?)).ok()?;
    Some(parsed.records.into_iter().map(|r| r.signature).collect())
}

/// Loads the quarantine exemption list, if one was given.
fn load_quarantine(path: Option<&Path>) -> Result<super::QuarantineConfig> {
    path.map_or(Ok(super::QuarantineConfig::default()), |path| {
        let input = fs::read_to_string(path)
            .wrap_err_with(|| eyre!("could not read quarantine file `{}`", path.display()))?;
        toml::from_str(&input)
            .wrap_err_with(|| eyre!("invalid quarantine file `{}`", path.display()))
    })
}

/// Loads triaged test ids from a triage JSON store, if one was given.
///
/// Format (P1.1 shared conventions): an object with an `entries` map from
/// stable test id to a verdict record; only id presence is read here.
fn load_triaged(path: Option<&Path>) -> Result<FxHashSet<String>> {
    #[derive(Deserialize)]
    struct TriageFile {
        #[serde(default)]
        entries: std::collections::HashMap<String, serde_json::Value>,
    }

    path.map_or(Ok(FxHashSet::default()), |path| {
        let parsed: TriageFile = serde_json::from_reader(BufReader::new(
            fs::File::open(path)
                .wrap_err_with(|| eyre!("could not open triage file `{}`", path.display()))?,
        ))
        .wrap_err("could not read the triage file")?;
        Ok(parsed.entries.into_keys().collect())
    })
}

/// Counts quarantined ids among the new bad outcomes (for the exemption note).
fn count_quarantined(outcome: &CompareOutcome, quarantine: &super::QuarantineConfig) -> usize {
    outcome
        .bad_in_new
        .iter()
        .filter(|(id, _)| quarantine.contains(id))
        .count()
}

/// Evaluates fail conditions, returning one message per breach.
///
/// Quarantined ids never breach; `Untriaged` additionally exempts ids present
/// in the triage store.
#[allow(clippy::too_many_lines)]
fn evaluate_fail_conditions(
    outcome: &CompareOutcome,
    fail_on: &[FailCondition],
    quarantine: &super::QuarantineConfig,
    triaged: &FxHashSet<String>,
) -> Vec<String> {
    use super::TestOutcomeResult;

    if fail_on.is_empty() {
        return Vec::new();
    }

    // Bad outcomes in `new`, minus quarantined ids.
    let bad: Vec<(String, TestOutcomeResult)> = outcome
        .bad_in_new
        .iter()
        .filter(|(id, _)| !quarantine.contains(id))
        .cloned()
        .collect();
    let has = |wanted: TestOutcomeResult| bad.iter().any(|(_, result)| *result == wanted);
    // Quarantine-aware counts per finding category.
    let unexempt = |findings: &[Finding]| {
        findings
            .iter()
            .filter(|finding| !quarantine.contains(&finding.id))
            .count()
    };

    // New crash signatures breach like new per-test crashes, so a changed
    // crash on an already-crashing test cannot slip through. Signatures
    // match quarantine entries by substring, like finding ids.
    let new_sig_findings = plain_findings(&outcome.new_signatures);

    let mut breaches = Vec::new();
    for condition in fail_on.iter().copied() {
        match condition {
            FailCondition::Failed if has(TestOutcomeResult::Failed) => {
                breaches.push(format!(
                    "failed: {} failing test(s) in new results",
                    bad.iter()
                        .filter(|(_, r)| *r == TestOutcomeResult::Failed)
                        .count()
                ));
            }
            FailCondition::Panic => {
                let count = unexempt(&outcome.new_panics);
                if count != 0 {
                    breaches.push(format!("panic: {count} new panic(s)"));
                }
            }
            FailCondition::Timeout => {
                let count = unexempt(&outcome.new_timeouts);
                if count != 0 {
                    breaches.push(format!("timeout: {count} new timeout(s)"));
                }
            }
            FailCondition::Crash => {
                let count = unexempt(&outcome.new_crashes);
                if count != 0 {
                    breaches.push(format!("crash: {count} new crash(es)"));
                }
                let sigs = unexempt(&new_sig_findings);
                if sigs != 0 {
                    breaches.push(format!("crash: {sigs} new crash signature(s)"));
                }
            }
            FailCondition::HarnessError => {
                let count = unexempt(&outcome.new_harness_errors);
                if count != 0 {
                    breaches.push(format!("harness-error: {count} new harness error(s)"));
                }
            }
            FailCondition::NewSkip => {
                let count = unexempt(&outcome.new_skips);
                if count != 0 {
                    breaches.push(format!("new-skip: {count} newly skipped test(s)"));
                }
            }
            FailCondition::Broken => {
                let count = unexempt(&outcome.broken);
                if count != 0 {
                    breaches.push(format!("broken: {count} broken test(s)"));
                }
            }
            FailCondition::Regression => {
                let mut parts = Vec::new();
                for (label, findings) in [
                    ("broken", &outcome.broken),
                    ("new panic(s)", &outcome.new_panics),
                    ("new timeout(s)", &outcome.new_timeouts),
                    ("new crash(es)", &outcome.new_crashes),
                    ("new crash signature(s)", &new_sig_findings),
                    ("new harness error(s)", &outcome.new_harness_errors),
                    ("new skip(s)", &outcome.new_skips),
                ] {
                    let count = unexempt(findings);
                    if count != 0 {
                        parts.push(format!("{count} {label}"));
                    }
                }
                if !parts.is_empty() {
                    breaches.push(format!("regression: {}", parts.join(", ")));
                }
            }
            FailCondition::Untriaged => {
                let count = bad.iter().filter(|(id, _)| !triaged.contains(id)).count();
                if count != 0 {
                    breaches.push(format!("untriaged: {count} untriaged bad outcome(s)"));
                }
            }
            FailCondition::Vanished if !outcome.vanished.is_empty() => {
                breaches.push(format!(
                    "vanished: {} base test(s) missing from new results",
                    outcome.vanished.len()
                ));
            }
            FailCondition::Failed | FailCondition::Vanished => {}
        }
    }
    breaches
}

/// Wraps plain strings (signatures) as findings for printing.
fn plain_findings(items: &[String]) -> Vec<Finding> {
    items
        .iter()
        .map(|item| Finding {
            id: item.clone(),
            detail: String::new(),
        })
        .collect()
}

/// Prints one markdown details section, capped at [`MAX_PRINTED`] entries.
fn print_details_markdown(title: &str, items: &[Finding]) {
    if items.is_empty() {
        return;
    }
    println!();
    println!(
        "<details><summary><b>{title} ({}):</b></summary>",
        items.len()
    );
    println!("\n```");
    for item in items.iter().take(MAX_PRINTED) {
        println!("{}", item.display());
    }
    if items.len() > MAX_PRINTED {
        println!("… and {} more", items.len() - MAX_PRINTED);
    }
    println!("```");
    println!("</details>");
}

/// Prints one plain-text details section, capped at [`MAX_PRINTED`] entries.
fn print_details_text(title: &str, items: &[Finding]) {
    if items.is_empty() {
        return;
    }
    println!();
    println!("{title} ({}):", items.len());
    for item in items.iter().take(MAX_PRINTED) {
        println!("{}", item.display());
    }
    if items.len() > MAX_PRINTED {
        println!("… and {} more", items.len() - MAX_PRINTED);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FailCondition, OutcomeMap, compare_results, diff_outcomes, flatten_suite,
        get_test262_commit,
    };
    use std::path::PathBuf;
    use std::process::Command;

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "boa_tester_commit_{}_{}_{nanos}",
            std::process::id(),
            tag
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn reports_head_on_branch_and_detached_checkout() {
        let dir = unique_temp_dir("repo");
        git(&dir, &["init", "-b", "main"]);
        git(&dir, &["config", "user.email", "tester@boa"]);
        git(&dir, &["config", "user.name", "tester"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("test.js"), "// empty\n").expect("write file");
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-m", "init"]);

        let expected = git(&dir, &["rev-parse", "HEAD"]);
        assert_eq!(&*get_test262_commit(&dir).expect("read commit"), expected);

        // Detached checkouts (what `git reset --hard <pin>`-style pinning can
        // leave behind) must report the checked-out commit, not fail.
        git(&dir, &["checkout", "--detach", "HEAD"]);
        assert_eq!(&*get_test262_commit(&dir).expect("read commit"), expected);

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn falls_back_to_main_ref_without_git_metadata() {
        let dir = unique_temp_dir("fallback");
        let ref_dir = dir.join(".git/refs/heads");
        std::fs::create_dir_all(&ref_dir).expect("create ref dir");
        std::fs::write(ref_dir.join("main"), "abc123\n").expect("write ref");

        assert_eq!(&*get_test262_commit(&dir).expect("read commit"), "abc123");

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    /// Builds a minimal `latest.json` from `(name, outcome-letter)` tests.
    fn fixture_json(tests: &[(&str, &str)]) -> String {
        let stats = serde_json::json!({
            "t": tests.len(),
            "o": tests.iter().filter(|(_, o)| *o == "O").count(),
            "i": tests.iter().filter(|(_, o)| *o == "I").count(),
            "p": tests.iter().filter(|(_, o)| *o == "P").count(),
        });
        let versioned = serde_json::json!({
            "es5": stats, "es6": stats, "es7": stats, "es8": stats,
            "es9": stats, "es10": stats, "es11": stats, "es12": stats,
            "es13": stats,
        });
        serde_json::json!({
            "c": "", "u": "",
            "r": {
                "n": "test", "a": stats, "av": versioned, "s": [{
                    "n": "lang", "a": stats, "av": versioned,
                    "t": tests.iter().map(|(n, r)| serde_json::json!({"n": n, "v": 5, "r": r})).collect::<Vec<_>>(),
                }],
            },
        })
        .to_string()
    }

    fn flatten_fixture(json: &str) -> OutcomeMap {
        let info: super::ResultInfo = serde_json::from_str(json).expect("parse fixture");
        let mut map = OutcomeMap::default();
        flatten_suite(&info.results, &mut map);
        map
    }

    #[test]
    fn diff_categorizes_transitions_by_id() {
        let base = flatten_fixture(&fixture_json(&[
            ("stays-pass", "O"),
            ("breaks", "O"),
            ("fixes", "F"),
            ("unignores", "I"),
            ("regresses-from-ignore", "I"),
            ("panics", "O"),
            ("unpanics", "P"),
            ("skipped", "O"),
            ("vanishes", "O"),
        ]));
        let new = flatten_fixture(&fixture_json(&[
            ("stays-pass", "O"),
            ("breaks", "F"),
            ("fixes", "O"),
            ("unignores", "O"),
            ("regresses-from-ignore", "F"),
            ("panics", "P"),
            ("unpanics", "O"),
            ("skipped", "I"),
            ("appears", "O"),
        ]));
        let outcome = diff_outcomes(&base, &new);
        let ids = |items: &[super::Finding]| items.iter().map(|f| f.id.clone()).collect::<Vec<_>>();

        assert_eq!(
            ids(&outcome.fixed),
            vec![
                "test/lang/fixes",
                "test/lang/unignores",
                "test/lang/unpanics"
            ]
        );
        assert_eq!(
            ids(&outcome.broken),
            vec![
                "test/lang/breaks",
                "test/lang/panics",
                "test/lang/regresses-from-ignore"
            ]
        );
        // The old `Ignored -> Failed` silent drop is gone.
        assert!(
            outcome
                .broken
                .iter()
                .find(|f| f.id == "test/lang/regresses-from-ignore")
                .is_some_and(|f| f.detail.contains("previously Ignored"))
        );
        assert_eq!(ids(&outcome.new_panics), vec!["test/lang/panics"]);
        assert_eq!(ids(&outcome.panic_fixes), vec!["test/lang/unpanics"]);
        assert_eq!(ids(&outcome.new_skips), vec!["test/lang/skipped"]);
        assert_eq!(outcome.new_only, vec!["test/lang/appears"]);
        assert_eq!(outcome.vanished, vec!["test/lang/vanishes"]);
    }

    #[test]
    fn strict_compare_breaches_on_crash_timeout_and_new_skip() {
        let dir = unique_temp_dir("compare");
        let base_json = fixture_json(&[("a", "O"), ("b", "O"), ("c", "O")]);
        let new_json = fixture_json(&[("a", "C"), ("b", "T"), ("c", "I")]);
        std::fs::write(dir.join("base.json"), &base_json).expect("write base");
        std::fs::write(dir.join("new.json"), &new_json).expect("write new");

        let breached = compare_results(
            &dir.join("base.json"),
            &dir.join("new.json"),
            false,
            &[
                FailCondition::Crash,
                FailCondition::Timeout,
                FailCondition::NewSkip,
            ],
            None,
            None,
        )
        .expect("compare");
        assert!(breached);

        // Advisory mode never breaches.
        let breached = compare_results(
            &dir.join("base.json"),
            &dir.join("new.json"),
            false,
            &[],
            None,
            None,
        )
        .expect("compare");
        assert!(!breached);

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn changed_crash_signature_breaches_on_same_outcome() {
        let root = unique_temp_dir("sigs");
        let base_dir = root.join("base");
        let new_dir = root.join("new");
        std::fs::create_dir_all(&base_dir).expect("create base");
        std::fs::create_dir_all(&new_dir).expect("create new");
        // Same per-test outcome on both sides: the crash predates the run,
        // so only the signature diff can catch the change.
        std::fs::write(base_dir.join("latest.json"), fixture_json(&[("x", "C")])).expect("write");
        std::fs::write(new_dir.join("latest.json"), fixture_json(&[("x", "C")])).expect("write");
        std::fs::write(
            base_dir.join("crashes.json"),
            r#"{"records":[{"signature":"crash@boa_engine: old"}]}"#,
        )
        .expect("write");
        std::fs::write(
            new_dir.join("crashes.json"),
            r#"{"records":[{"signature":"crash@boa_engine: new"}]}"#,
        )
        .expect("write");

        for condition in [FailCondition::Crash, FailCondition::Regression] {
            let breached = compare_results(&base_dir, &new_dir, false, &[condition], None, None)
                .expect("compare");
            assert!(breached, "{condition:?} breaches on a changed signature");
        }

        // Identical signatures stay silent.
        std::fs::write(
            new_dir.join("crashes.json"),
            r#"{"records":[{"signature":"crash@boa_engine: old"}]}"#,
        )
        .expect("write");
        let breached = compare_results(
            &base_dir,
            &new_dir,
            false,
            &[FailCondition::Regression],
            None,
            None,
        )
        .expect("compare");
        assert!(!breached);

        std::fs::remove_dir_all(&root).expect("remove temp dir");
    }

    #[test]
    fn quarantine_and_triage_exempt_breaches() {
        use std::io::Write as _;

        let dir = unique_temp_dir("exempt");
        let base_json = fixture_json(&[("flaky", "O"), ("known", "O")]);
        let new_json = fixture_json(&[("flaky", "F"), ("known", "F")]);
        std::fs::write(dir.join("base.json"), &base_json).expect("write base");
        std::fs::write(dir.join("new.json"), &new_json).expect("write new");

        let mut quarantine = std::fs::File::create(dir.join("q.toml")).expect("create");
        writeln!(
            quarantine,
            "quarantine = [{{ pattern = \"test/lang/flaky\", reason = \"r\", link = \"l\", owner = \"o\", added = \"2026-10-02\", expiry = \"2099-01-01\", work_item = \"w\" }}]"
        )
        .expect("write");
        drop(quarantine);
        std::fs::write(
            dir.join("triage.json"),
            r#"{"entries": {"test/lang/known": {"verdict": "known-failure"}}}"#,
        )
        .expect("write");

        // Both bad ids exempted: one quarantined, one triaged.
        let breached = compare_results(
            &dir.join("base.json"),
            &dir.join("new.json"),
            false,
            &[FailCondition::Untriaged],
            Some(&dir.join("q.toml")),
            Some(&dir.join("triage.json")),
        )
        .expect("compare");
        assert!(!breached);

        // Without exemptions the same comparison breaches.
        let breached = compare_results(
            &dir.join("base.json"),
            &dir.join("new.json"),
            false,
            &[FailCondition::Untriaged],
            None,
            None,
        )
        .expect("compare");
        assert!(breached);

        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }
}
