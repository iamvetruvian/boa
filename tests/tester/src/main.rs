//! Test262 test runner
//!
//! This crate will run the full ECMAScript test suite (Test262) and report compliance of the
//! `boa` engine.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![allow(
    clippy::too_many_lines,
    clippy::redundant_pub_crate,
    clippy::cast_precision_loss,
    clippy::print_stderr,
    clippy::print_stdout
)]

use std::{
    num::NonZeroU64,
    ops::{Add, AddAssign},
    path::{Path, PathBuf},
    process::Command,
};

use bitflags::bitflags;
use clap::{ArgAction, Parser, ValueHint};
use color_eyre::{
    Result,
    eyre::{WrapErr, bail, eyre},
};
use colored::Colorize;
use cow_utils::CowUtils;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Unexpected, Visitor},
};

use boa_engine::optimizer::OptimizerOptions;
use edition::SpecEdition;
use read::ErrorType;

use self::{
    read::{MetaData, Negative, TestFlag, read_harness, read_suite, read_test},
    results::{FailCondition, compare_results, write_json},
};

mod audit;
mod crash;
mod edition;
mod exec;
mod read;
mod report;
mod results;

/// Structure that contains the configuration of the tester.
#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    commit: String,
    #[serde(default)]
    ignored: Ignored,
}

impl Config {
    /// Get the `Test262` repository commit.
    pub(crate) fn commit(&self) -> &str {
        &self.commit
    }

    /// Get [`Ignored`] `Test262` tests and features.
    pub(crate) const fn ignored(&self) -> &Ignored {
        &self.ignored
    }
}

/// A single audited ignore entry.
///
/// Every ignored test path or feature must carry a machine-readable
/// justification: why it is ignored, where to track the underlying work,
/// who owns removing the entry, and when it must be re-reviewed. The
/// `check-config` subcommand rejects entries with missing or expired fields.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IgnoreEntry {
    /// Substring matched against the test path (for `tests`) or the feature
    /// name with pre-dot prefix fallback (for `features`).
    pub(crate) pattern: Box<str>,
    /// Why the match is ignored.
    pub(crate) reason: Box<str>,
    /// Spec, proposal, or issue link tracking the underlying work.
    pub(crate) link: Box<str>,
    /// Owner responsible for removing the entry.
    pub(crate) owner: Box<str>,
    /// Date the entry was audited, `YYYY-MM-DD`.
    pub(crate) added: Box<str>,
    /// Re-review trigger: a `YYYY-MM-DD` date or `re-review:<ref>`.
    pub(crate) expiry: Box<str>,
    /// The P2 work item that removes the entry.
    pub(crate) work_item: Box<str>,
}

/// Structure to allow defining ignored tests, features and files that should
/// be ignored even when reading.
#[derive(Default, Debug, Deserialize)]
struct Ignored {
    #[serde(default)]
    tests: Vec<IgnoreEntry>,
    #[serde(default)]
    features: Vec<IgnoreEntry>,
    #[serde(default = "TestFlags::empty")]
    flags: TestFlags,
}

impl Ignored {
    /// Checks if the ignore list contains the given test in the list of
    /// tests to ignore.
    ///
    /// Matching is deliberately a substring match so one entry can cover a
    /// whole directory subtree; keep patterns as specific as possible.
    pub(crate) fn contains_test(&self, test_path: &Path) -> bool {
        self.match_test(test_path).is_some()
    }

    /// Returns the first `tests` entry matching the path, for attribution.
    pub(crate) fn match_test(&self, test_path: &Path) -> Option<&IgnoreEntry> {
        let test_path = test_path.to_str()?;
        self.tests
            .iter()
            .find(|entry| test_path.contains(&*entry.pattern))
    }

    /// Checks if the ignore list contains the given feature name in the list
    /// of features to ignore.
    pub(crate) fn contains_feature(&self, feature: &str) -> bool {
        self.match_feature(feature).is_some()
    }

    /// Returns the matching `features` entry, exact match first, then the
    /// pre-dot prefix fallback (same precedence as [`Self::contains_feature`]).
    pub(crate) fn match_feature(&self, feature: &str) -> Option<&IgnoreEntry> {
        if let Some(entry) = self
            .features
            .iter()
            .find(|entry| &*entry.pattern == feature)
        {
            return Some(entry);
        }
        // Some features are an accessor instead of a simple feature name e.g. `Intl.DurationFormat`.
        // This ensures those are also ignored.
        let prefix = feature.split('.').next()?;
        self.features.iter().find(|entry| &*entry.pattern == prefix)
    }

    pub(crate) const fn contains_any_flag(&self, flags: TestFlags) -> bool {
        flags.intersects(self.flags)
    }
}

/// Flakiness quarantine: tests that run but whose failures are known-flaky.
///
/// Same audited-entry schema as [`Ignored`]; unlike ignores, quarantined
/// tests still execute — `compare --fail-on` merely exempts their ids from
/// breaching, and prints the exemption count so it can never be silent.
#[derive(Default, Debug, Deserialize)]
pub(crate) struct QuarantineConfig {
    #[serde(default)]
    pub(crate) quarantine: Vec<IgnoreEntry>,
}

impl QuarantineConfig {
    /// Checks if a stable test id is quarantined (substring match).
    pub(crate) fn contains(&self, test_id: &str) -> bool {
        self.quarantine
            .iter()
            .any(|entry| test_id.contains(&*entry.pattern))
    }
}

/// Boa test262 tester
#[derive(Debug, Parser)]
#[command(author, version, about, name = "Boa test262 tester")]
enum Cli {
    /// Run the test suite.
    Run {
        /// Whether to show verbose output.
        #[arg(short, long, action = ArgAction::Count)]
        verbose: u8,

        /// Path to the Test262 suite.
        #[arg(
            long,
            value_hint = ValueHint::DirPath,
            conflicts_with = "test262_commit"
        )]
        test262_path: Option<PathBuf>,

        /// Override config's Test262 commit. To checkout the latest commit set this to "latest".
        #[arg(long)]
        test262_commit: Option<String>,

        /// Which specific test or test suite to run. Should be a path relative to the Test262 directory: e.g. "test/language/types/number"
        #[arg(short, long, default_value = "test", value_hint = ValueHint::AnyPath)]
        suite: PathBuf,

        /// Enable optimizations
        #[arg(long, short = 'O')]
        optimize: bool,

        /// Per-test wall-clock budget in seconds. Each test runs in an
        /// isolated subprocess that is killed (and counted as a timeout)
        /// after the budget expires. Without this flag tests run in-process
        /// with no per-test budget.
        #[arg(long)]
        timeout: Option<u64>,

        /// Optional output folder for the full results information.
        #[arg(short, long, value_hint = ValueHint::DirPath)]
        output: Option<PathBuf>,

        /// Execute tests serially
        #[arg(short, long)]
        disable_parallelism: bool,

        /// Path to a TOML file containing tester config.
        #[arg(short, long, default_value = "test262_config.toml", value_hint = ValueHint::FilePath)]
        config: PathBuf,

        /// Maximum ECMAScript edition to test for.
        #[arg(long)]
        edition: Option<SpecEdition>,

        /// Displays the conformance results per ECMAScript edition.
        #[arg(long)]
        versioned: bool,

        /// Injects the `Console` object into every context created.
        #[arg(long)]
        console: bool,

        /// Run every test under GC stress: the collector runs every N
        /// allocations (default 100) instead of at its 1 MB threshold, so
        /// missing GC roots and collection-timing assumptions surface as
        /// outcome divergences. Gate a stressed run against a normal run
        /// with `compare --fail-on=regression`.
        #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "100")]
        gc_stress: Option<u64>,

        /// Write the process-global `vm-coverage` dynamic histogram to FILE
        /// as JSON (`{"static": {}, "dynamic": {cell: count}}`) after the
        /// suite. Counts are exact under parallelism (addition commutes)
        /// but attribution is suite-global, not per-test. In-process only:
        /// conflicts with `timeout`. Requires `--features coverage`.
        #[arg(long, value_hint = ValueHint::FilePath, conflicts_with = "timeout")]
        coverage_out: Option<PathBuf>,
    },
    /// Compare two test suite results.
    Compare {
        /// Base results of the suite.
        #[arg(value_hint = ValueHint::FilePath)]
        base: PathBuf,

        /// New results to compare.
        #[arg(value_hint = ValueHint::FilePath)]
        new: PathBuf,

        /// Whether to use markdown output
        #[arg(short, long)]
        markdown: bool,

        /// Fail conditions that breach the run (exit 1). Without this flag
        /// the comparison is advisory. Accepts a comma-separated list.
        #[arg(long, value_delimiter = ',')]
        fail_on: Vec<FailCondition>,

        /// Quarantine file whose matching test ids are exempt from breaching.
        #[arg(long, value_hint = ValueHint::FilePath)]
        quarantine: Option<PathBuf>,

        /// Triage JSON store marking known-bad test ids as triaged.
        #[arg(long, value_hint = ValueHint::FilePath)]
        triage: Option<PathBuf>,
    },
    /// Run a single test attempt (internal subprocess protocol).
    ///
    /// Do not invoke directly: this is the isolated-child entry point used
    /// by `run --timeout`. It prints exactly one JSON outcome line to
    /// stdout and exits 0 on every completed attempt. No stability
    /// guarantee is made for its arguments or output.
    RunSingle {
        /// Path to the Test262 suite.
        #[arg(long, value_hint = ValueHint::DirPath)]
        test262_path: PathBuf,

        /// Test file to run.
        #[arg(long, value_hint = ValueHint::FilePath)]
        test: PathBuf,

        /// Run in strict mode.
        #[arg(long)]
        strict: bool,

        /// Enable optimizations.
        #[arg(long)]
        optimize: bool,

        /// Add the `console` object to the context.
        #[arg(long)]
        console: bool,

        /// GC-stress cadence inherited from the parent `run --gc-stress`.
        #[arg(long, value_name = "N")]
        gc_stress: Option<u64>,
    },
    /// Build the conformance-gap matrix from a run.
    ///
    /// Joins per-test outcomes with live Test262 metadata, attributes
    /// ignores, and writes `conformance-gap.json` + `conformance-gap.md`.
    /// With `--check`, enforces the trend ratchet against a previous matrix.
    Report {
        /// Run results: a results directory (uses `latest.json`) or the file itself.
        #[arg(value_hint = ValueHint::AnyPath)]
        run: PathBuf,

        /// Path to the Test262 checkout (read-only; used for test metadata).
        #[arg(long, default_value = "test262", value_hint = ValueHint::DirPath)]
        test262_path: PathBuf,

        /// Path to a TOML file containing tester config (ignore attribution).
        #[arg(short, long, default_value = "test262_config.toml", value_hint = ValueHint::FilePath)]
        config: PathBuf,

        /// Curated area triage TOML (area -> layer/work item/status).
        /// Areas without an entry render as untriaged.
        #[arg(long, value_hint = ValueHint::FilePath)]
        areas: Option<PathBuf>,

        /// Output directory for `conformance-gap.json` + `conformance-gap.md`.
        #[arg(short, long, value_hint = ValueHint::DirPath)]
        output: PathBuf,

        /// Previous matrix to check monotonicity against (trend gate).
        #[arg(long, value_hint = ValueHint::FilePath)]
        check: Option<PathBuf>,
    },
    /// Validate the tester config and quarantine file.
    CheckConfig {
        /// Path to a TOML file containing tester config.
        #[arg(short, long, default_value = "test262_config.toml", value_hint = ValueHint::FilePath)]
        config: PathBuf,

        /// Path to the quarantine file (defaults to `test262_quarantine.toml`
        /// when present, otherwise no quarantine).
        #[arg(long, value_hint = ValueHint::FilePath)]
        quarantine: Option<PathBuf>,

        /// Print the age/area report for all entries.
        #[arg(long)]
        report: bool,
    },
}

const DEFAULT_TEST262_DIRECTORY: &str = "test262";

/// Program entry point.
fn main() -> Result<()> {
    color_eyre::install()?;

    match Cli::parse() {
        Cli::Run {
            verbose,
            test262_path,
            test262_commit,
            suite,
            output,
            optimize,
            timeout,
            disable_parallelism,
            config: config_path,
            edition,
            versioned,
            console,
            gc_stress,
            coverage_out,
        } => {
            #[cfg(feature = "coverage")]
            if coverage_out.is_some() {
                boa_engine::vm::coverage::coverage_clear();
            }
            #[cfg(not(feature = "coverage"))]
            if coverage_out.is_some() {
                bail!("--coverage-out needs the `coverage` feature (`--features coverage`)");
            }
            let gc_stress = parse_gc_stress(gc_stress)?;
            let config: Config = {
                let input = std::fs::read_to_string(&config_path).wrap_err_with(|| {
                    eyre!("could not read config file `{}`", config_path.display())
                })?;
                toml::from_str(&input)
                    .wrap_err_with(|| eyre!("invalid config file `{}`", config_path.display()))?
            };

            let test262_commit = test262_commit
                .as_deref()
                .or_else(|| Some(config.commit()))
                .filter(|s| !["", "latest"].contains(s));

            let test262_path = if let Some(path) = test262_path.as_deref() {
                path
            } else {
                clone_test262(test262_commit, verbose)?;

                Path::new(DEFAULT_TEST262_DIRECTORY)
            }
            .canonicalize();
            let test262_path = &test262_path.wrap_err("could not get the Test262 path")?;

            run_test_suite(
                &config,
                verbose,
                !disable_parallelism,
                test262_path,
                suite.as_path(),
                output.as_deref(),
                edition.unwrap_or_default(),
                versioned,
                if optimize {
                    OptimizerOptions::OPTIMIZE_ALL
                } else {
                    OptimizerOptions::empty()
                },
                console,
                timeout,
                gc_stress,
            )?;
            #[cfg(feature = "coverage")]
            if let Some(path) = coverage_out.as_deref() {
                let dump = boa_engine::vm::coverage::coverage_dump_json();
                std::fs::write(path, format!("{{\"static\":{{}},\"dynamic\":{dump}}}"))
                    .wrap_err_with(|| {
                        eyre!("could not write coverage file `{}`", path.display())
                    })?;
            }
            Ok(())
        }
        Cli::RunSingle {
            test262_path,
            test,
            strict,
            optimize,
            console,
            gc_stress,
        } => run_single(
            test262_path.as_path(),
            test.as_path(),
            strict,
            if optimize {
                OptimizerOptions::OPTIMIZE_ALL
            } else {
                OptimizerOptions::empty()
            },
            console,
            parse_gc_stress(gc_stress)?,
        ),
        Cli::Compare {
            base,
            new,
            markdown,
            fail_on,
            quarantine,
            triage,
        } => {
            let breached = compare_results(
                base.as_path(),
                new.as_path(),
                markdown,
                &fail_on,
                quarantine.as_deref(),
                triage.as_deref(),
            )?;
            if breached {
                std::process::exit(1);
            }
            Ok(())
        }
        Cli::CheckConfig {
            config: config_path,
            quarantine: quarantine_path,
            report,
        } => check_config(config_path.as_path(), quarantine_path.as_deref(), report),
        Cli::Report {
            run,
            test262_path,
            config: config_path,
            areas,
            output,
            check,
        } => {
            let config = load_config(config_path.as_path())?;
            let breached = report::generate_report(
                run.as_path(),
                test262_path.as_path(),
                &config,
                areas.as_deref(),
                output.as_path(),
                check.as_deref(),
            )?;
            if breached {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

/// Validates a `--gc-stress` cadence (shared by `run` and `run-single`).
fn parse_gc_stress(n: Option<u64>) -> Result<Option<NonZeroU64>> {
    n.map(|n| NonZeroU64::new(n).ok_or_else(|| eyre!("--gc-stress N must be nonzero")))
        .transpose()
}

/// Loads and parses the tester TOML config.
fn load_config(path: &Path) -> Result<Config> {
    let input = std::fs::read_to_string(path)
        .wrap_err_with(|| eyre!("could not read config file `{}`", path.display()))?;
    toml::from_str(&input).wrap_err_with(|| eyre!("invalid config file `{}`", path.display()))
}

/// Isolated-child entry point: runs one strictness attempt and prints one
/// outcome JSON line to stdout.
fn run_single(
    test262_path: &Path,
    test_path: &Path,
    strict: bool,
    optimizer_options: OptimizerOptions,
    console: bool,
    gc_stress: Option<NonZeroU64>,
) -> Result<()> {
    let harness = read_harness(test262_path).wrap_err("could not read harness")?;
    let test = read_test(test_path).wrap_err("could not read test")?;
    // The parent already handled ignored/unreadable tests and edition
    // filtering; the child unconditionally runs one quiet attempt.
    let (result, _) = test.run_once(
        &harness,
        strict,
        0,
        optimizer_options,
        console,
        test262_path,
        true,
        gc_stress,
    );
    let outcome = exec::SingleOutcome {
        outcome: result.result,
        // Truncate before serializing: the outcome travels as one stdout
        // line through a ~64 KiB OS pipe, so an untruncated
        // completion-value dump would block the child mid-write. The parent
        // truncates to this same bound, so stored text is unchanged.
        text: crash::truncate(result.result_text.to_string(), exec::MAX_RESULT_TEXT),
    };
    println!(
        "{}",
        serde_json::to_string(&outcome).wrap_err("could not serialize the outcome")?
    );
    Ok(())
}

/// Root-suite display name for stable cross-run test ids.
///
/// `compare` joins per-test outcomes by id, and ids root at the suite name —
/// so a `-s test/built-ins/Array` subset must root at `test/built-ins/Array`,
/// not `Array`. Returns `None` for `.` (whole-directory runs keep the
/// directory name).
fn suite_root_name(suite: &Path) -> Option<Box<str>> {
    let text = suite
        .to_str()?
        .trim_end_matches('/')
        .cow_replace('\\', "/")
        .into_owned();
    if text.is_empty() || text == "." {
        None
    } else {
        Some(text.into_boxed_str())
    }
}

/// Validates the tester config and quarantine file, exiting nonzero on any
/// violation.
fn check_config(config_path: &Path, quarantine_path: Option<&Path>, report: bool) -> Result<()> {
    let input = std::fs::read_to_string(config_path)
        .wrap_err_with(|| eyre!("could not read config file `{}`", config_path.display()))?;
    let config: Config = toml::from_str(&input)
        .wrap_err_with(|| eyre!("invalid config file `{}`", config_path.display()))?;

    let quarantine_default = PathBuf::from("test262_quarantine.toml");
    let quarantine_path = quarantine_path.or(if quarantine_default.exists() {
        Some(quarantine_default.as_path())
    } else {
        None
    });
    let quarantine: QuarantineConfig =
        quarantine_path.map_or(Ok(QuarantineConfig::default()), |path| {
            let input = std::fs::read_to_string(path)
                .wrap_err_with(|| eyre!("could not read quarantine file `{}`", path.display()))?;
            toml::from_str(&input)
                .wrap_err_with(|| eyre!("invalid quarantine file `{}`", path.display()))
        })?;

    let mut violations = audit::validate_config(config.ignored());
    violations.extend(audit::validate_quarantine(&quarantine));

    if report {
        audit::print_report(config.ignored(), &quarantine);
        println!();
    }

    if violations.is_empty() {
        println!("config OK");
        Ok(())
    } else {
        for violation in &violations {
            println!("violation: {violation}");
        }
        std::process::exit(1);
    }
}

/// Returns the commit hash and commit message of the provided branch name.
fn get_last_branch_commit(branch: &str, verbose: u8) -> Result<(String, String)> {
    if verbose > 1 {
        println!("Getting last commit on '{branch}' branch");
    }
    let result = Command::new("git")
        .arg("log")
        .args(["-n", "1"])
        .arg("--pretty=format:%H %s")
        .arg(branch)
        .current_dir(DEFAULT_TEST262_DIRECTORY)
        .output()?;

    if !result.status.success() {
        bail!(
            "test262 getting commit hash and message failed with return code {:?}",
            result.status.code()
        );
    }

    let output = std::str::from_utf8(&result.stdout)?.trim();

    let (hash, message) = output
        .split_once(' ')
        .expect("git log output to contain hash and message");

    Ok((hash.into(), message.into()))
}

fn reset_test262_commit(commit: &str, verbose: u8) -> Result<()> {
    if verbose != 0 {
        println!("Reset test262 to commit: {commit}...");
    }

    let result = Command::new("git")
        .arg("reset")
        .arg("--hard")
        .arg(commit)
        .current_dir(DEFAULT_TEST262_DIRECTORY)
        .status()?;

    if !result.success() {
        bail!(
            "test262 commit {commit} checkout failed with return code: {:?}",
            result.code()
        );
    }

    Ok(())
}

fn clone_test262(commit: Option<&str>, verbose: u8) -> Result<()> {
    const TEST262_REPOSITORY: &str = "https://github.com/tc39/test262";

    let update = commit.is_none();

    if Path::new(DEFAULT_TEST262_DIRECTORY).is_dir() {
        let (current_commit_hash, current_commit_message) =
            get_last_branch_commit("HEAD", verbose)?;

        if let Some(commit) = commit
            && current_commit_hash == commit
        {
            return Ok(());
        }

        if verbose != 0 {
            println!("Fetching latest test262 commits...");
        }
        let result = Command::new("git")
            .arg("fetch")
            .current_dir(DEFAULT_TEST262_DIRECTORY)
            .status()?;

        if !result.success() {
            bail!(
                "Test262 fetching latest failed with return code {:?}",
                result.code()
            );
        }

        if let Some(commit) = commit {
            println!("Test262 switching to commit {commit}...");
            reset_test262_commit(commit, verbose)?;
            return Ok(());
        }

        if verbose != 0 {
            println!("Checking latest Test262 with current HEAD...");
        }
        let (latest_commit_hash, latest_commit_message) =
            get_last_branch_commit("origin/main", verbose)?;

        if current_commit_hash != latest_commit_hash {
            if update {
                println!("Updating Test262 repository:");
            } else {
                println!(
                    "Warning Test262 repository is not in sync, use '--test262-commit latest' to automatically update it:"
                );
            }

            println!("    Current commit: {current_commit_hash} {current_commit_message}");
            println!("    Latest commit:  {latest_commit_hash} {latest_commit_message}");

            if update {
                reset_test262_commit(&latest_commit_hash, verbose)?;
            }
        }

        return Ok(());
    }

    println!("Cloning test262...");
    let result = Command::new("git")
        .arg("clone")
        .arg(TEST262_REPOSITORY)
        .arg(DEFAULT_TEST262_DIRECTORY)
        .status()?;

    if !result.success() {
        bail!(
            "Cloning Test262 repository failed with return code {:?}",
            result.code()
        );
    }

    if let Some(commit) = commit {
        if verbose != 0 {
            println!("Reset Test262 to commit: {commit}...");
        }

        reset_test262_commit(commit, verbose)?;
    }

    Ok(())
}

/// Runs the full test suite.
#[allow(clippy::too_many_arguments)]
fn run_test_suite(
    config: &Config,
    verbose: u8,
    parallel: bool,
    test262_path: &Path,
    suite: &Path,
    output: Option<&Path>,
    edition: SpecEdition,
    versioned: bool,
    optimizer_options: OptimizerOptions,
    console: bool,
    timeout: Option<u64>,
    gc_stress: Option<NonZeroU64>,
) -> Result<()> {
    if let Some(path) = output {
        if path.exists() {
            if !path.is_dir() {
                bail!("the output path must be a directory.");
            }
        } else {
            std::fs::create_dir_all(path).wrap_err("could not create the output directory")?;
        }
    }

    if verbose != 0 {
        println!("Loading the test suite...");
    }
    let harness = read_harness(test262_path).wrap_err("could not read harness")?;

    if suite.to_string_lossy().ends_with(".js") {
        let test = read_test(&test262_path.join(suite)).wrap_err_with(|| {
            let suite = suite.display();
            format!("could not read the test {suite}")
        })?;

        if test.edition <= edition {
            if verbose != 0 {
                println!("Test loaded, starting...");
            }
            let (_, crash) = test.run(
                &harness,
                verbose,
                optimizer_options,
                console,
                test262_path,
                timeout,
                gc_stress,
            );
            if let Some(record) = crash {
                eprintln!(
                    "abnormal outcome on `{}` [{}]: {}",
                    test.path.display(),
                    record.signature,
                    record.message
                );
            }
        } else {
            println!(
                "Minimum spec edition of test is bigger than the specified edition. Skipping."
            );
        }

        println!();
    } else {
        let mut test_suite = read_suite(&test262_path.join(suite), config.ignored(), false)
            .wrap_err_with(|| {
                let suite = suite.display();
                format!("could not read the suite {suite}")
            })?;

        // Name the root suite by its test262-relative path so per-test ids
        // (`test/built-ins/Array/from`) match across full and subset runs.
        if let Some(top) = suite_root_name(suite) {
            test_suite.name = top;
        }

        if verbose != 0 {
            println!("Test suite loaded, starting tests...");
        }
        let (results, crashes) = test_suite.run(
            &harness,
            verbose,
            parallel,
            edition,
            optimizer_options,
            console,
            test262_path,
            timeout,
            gc_stress,
        );

        if versioned {
            let mut table = comfy_table::Table::new();
            table.load_style(comfy_table::presets::UTF8_HORIZONTAL_ONLY);
            table.set_header(vec![
                "Edition", "Total", "Passed", "Ignored", "Failed", "Panics", "Timeouts", "Crashes",
                "Harness", "%",
            ]);
            for column in table.column_iter_mut().skip(1) {
                column.set_cell_alignment(comfy_table::CellAlignment::Right);
            }
            for (v, stats) in SpecEdition::all_editions()
                .filter(|v| *v <= edition)
                .map(|v| {
                    let stats = results.versioned_stats.get(v).unwrap_or(results.stats);
                    (v, stats)
                })
            {
                let conformance = (stats.passed as f64 / stats.total as f64) * 100.0;
                let conformance = format!("{conformance:.2}");
                table.add_row(vec![
                    v.to_string(),
                    stats.total.to_string(),
                    stats.passed.to_string(),
                    stats.ignored.to_string(),
                    stats.failed().to_string(),
                    stats.panic.to_string(),
                    stats.timeout.to_string(),
                    stats.crash.to_string(),
                    stats.harness_error.to_string(),
                    conformance,
                ]);
            }
            println!("\n\nResults\n");
            println!("{table}");
        } else {
            let stats = results.stats;
            println!("\n\nResults ({edition}):");
            println!("Total tests: {}", stats.total);
            println!("Passed tests: {}", stats.passed.to_string().green());
            println!("Ignored tests: {}", stats.ignored.to_string().yellow());
            println!("Failed tests: {}", stats.failed().to_string().red());
            println!("Panics: {}", stats.panic.to_string().red());
            println!("Timeouts: {}", stats.timeout.to_string().red());
            println!("Crashes: {}", stats.crash.to_string().red());
            println!("Harness errors: {}", stats.harness_error.to_string().red());
            println!(
                "Conformance: {:.2}%",
                (stats.passed as f64 / stats.total as f64) * 100.0
            );
        }

        if let Some(output) = output {
            write_json(results, &crashes, output, verbose, test262_path)
                .wrap_err("could not write the results to the output JSON file")?;
        }
    }

    Ok(())
}

/// All the harness include files.
#[derive(Debug, Clone)]
struct Harness {
    assert: HarnessFile,
    sta: HarnessFile,
    doneprint_handle: HarnessFile,
    includes: FxHashMap<Box<str>, HarnessFile>,
}

#[derive(Debug, Clone)]
struct HarnessFile {
    content: Box<str>,
    path: Box<Path>,
}

/// Represents a test suite.
#[derive(Debug, Clone)]
struct TestSuite {
    name: Box<str>,
    path: Box<Path>,
    suites: Box<[TestSuite]>,
    tests: Box<[Test]>,
}

/// Shared outcome taxonomy + counters (single home: `boa_outcomes`, so WPT
/// and differential harnesses count the same outcomes without duplicating
/// the taxonomy).
pub(crate) use boa_outcomes::{Statistics, TestOutcomeResult};

/// Represents tests statistics separated by ECMAScript edition
#[derive(Default, Debug, Copy, Clone, Serialize)]
struct VersionedStats {
    es5: Statistics,
    es6: Statistics,
    es7: Statistics,
    es8: Statistics,
    es9: Statistics,
    es10: Statistics,
    es11: Statistics,
    es12: Statistics,
    es13: Statistics,
    es14: Statistics,
    es15: Statistics,
    es16: Statistics,
    es17: Statistics,
}

impl<'de> Deserialize<'de> for VersionedStats {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Inner {
            es5: Statistics,
            es6: Statistics,
            es7: Statistics,
            es8: Statistics,
            es9: Statistics,
            es10: Statistics,
            es11: Statistics,
            es12: Statistics,
            es13: Statistics,
            #[serde(default)]
            es14: Option<Statistics>,
            #[serde(default)]
            es15: Option<Statistics>,
            #[serde(default)]
            es16: Option<Statistics>,
            #[serde(default)]
            es17: Option<Statistics>,
        }

        let inner = Inner::deserialize(deserializer)?;

        let Inner {
            es5,
            es6,
            es7,
            es8,
            es9,
            es10,
            es11,
            es12,
            es13,
            es14,
            es15,
            es16,
            es17,
        } = inner;
        let es14 = es14.unwrap_or(es13);
        let es15 = es15.unwrap_or(es14);
        let es16 = es16.unwrap_or(es15);
        let es17 = es17.unwrap_or(es16);

        Ok(Self {
            es5,
            es6,
            es7,
            es8,
            es9,
            es10,
            es11,
            es12,
            es13,
            es14,
            es15,
            es16,
            es17,
        })
    }
}

impl VersionedStats {
    /// Applies `f` to all the statistics for which its edition is bigger or equal
    /// than `min_edition`.
    fn apply(&mut self, min_edition: SpecEdition, f: fn(&mut Statistics)) {
        for edition in SpecEdition::all_editions().filter(|&edition| min_edition <= edition) {
            if let Some(stats) = self.get_mut(edition) {
                f(stats);
            }
        }
    }

    /// Gets the statistics corresponding to `edition`, returning `None` if `edition`
    /// is `SpecEdition::ESNext`.
    const fn get(&self, edition: SpecEdition) -> Option<Statistics> {
        let stats = match edition {
            SpecEdition::ES5 => self.es5,
            SpecEdition::ES6 => self.es6,
            SpecEdition::ES7 => self.es7,
            SpecEdition::ES8 => self.es8,
            SpecEdition::ES9 => self.es9,
            SpecEdition::ES10 => self.es10,
            SpecEdition::ES11 => self.es11,
            SpecEdition::ES12 => self.es12,
            SpecEdition::ES13 => self.es13,
            SpecEdition::ES14 => self.es14,
            SpecEdition::ES15 => self.es15,
            SpecEdition::ES16 => self.es16,
            SpecEdition::ES17 => self.es17,
            SpecEdition::ESNext => return None,
        };
        Some(stats)
    }

    /// Gets a mutable reference to the statistics corresponding to `edition`, returning `None` if
    /// `edition` is `SpecEdition::ESNext`.
    fn get_mut(&mut self, edition: SpecEdition) -> Option<&mut Statistics> {
        let stats = match edition {
            SpecEdition::ES5 => &mut self.es5,
            SpecEdition::ES6 => &mut self.es6,
            SpecEdition::ES7 => &mut self.es7,
            SpecEdition::ES8 => &mut self.es8,
            SpecEdition::ES9 => &mut self.es9,
            SpecEdition::ES10 => &mut self.es10,
            SpecEdition::ES11 => &mut self.es11,
            SpecEdition::ES12 => &mut self.es12,
            SpecEdition::ES13 => &mut self.es13,
            SpecEdition::ES14 => &mut self.es14,
            SpecEdition::ES15 => &mut self.es15,
            SpecEdition::ES16 => &mut self.es16,
            SpecEdition::ES17 => &mut self.es17,
            SpecEdition::ESNext => return None,
        };
        Some(stats)
    }
}

impl Add for VersionedStats {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            es5: self.es5 + rhs.es5,
            es6: self.es6 + rhs.es6,
            es7: self.es7 + rhs.es7,
            es8: self.es8 + rhs.es8,
            es9: self.es9 + rhs.es9,
            es10: self.es10 + rhs.es10,
            es11: self.es11 + rhs.es11,
            es12: self.es12 + rhs.es12,
            es13: self.es13 + rhs.es13,
            es14: self.es14 + rhs.es14,
            es15: self.es15 + rhs.es15,
            es16: self.es16 + rhs.es16,
            es17: self.es17 + rhs.es17,
        }
    }
}

impl AddAssign for VersionedStats {
    fn add_assign(&mut self, rhs: Self) {
        self.es5 += rhs.es5;
        self.es6 += rhs.es6;
        self.es7 += rhs.es7;
        self.es8 += rhs.es8;
        self.es9 += rhs.es9;
        self.es10 += rhs.es10;
        self.es11 += rhs.es11;
        self.es12 += rhs.es12;
        self.es13 += rhs.es13;
        self.es14 += rhs.es14;
        self.es15 += rhs.es15;
        self.es16 += rhs.es16;
        self.es17 += rhs.es17;
    }
}

/// Outcome of a test suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SuiteResult {
    #[serde(rename = "n")]
    name: Box<str>,
    #[serde(rename = "a")]
    stats: Statistics,
    #[serde(rename = "av", default)]
    versioned_stats: VersionedStats,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[serde(rename = "s")]
    suites: Vec<SuiteResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[serde(rename = "t")]
    tests: Vec<TestResult>,
    #[serde(skip_serializing_if = "FxHashSet::is_empty", default)]
    #[serde(rename = "f")]
    features: FxHashSet<String>,
}

/// Outcome of a test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct TestResult {
    #[serde(rename = "n")]
    name: Box<str>,
    #[serde(rename = "v", default)]
    edition: SpecEdition,
    #[serde(skip)]
    result_text: Box<str>,
    #[serde(rename = "r")]
    result: TestOutcomeResult,
}

/// Represents a test.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Test {
    name: Box<str>,
    path: Box<Path>,
    description: Box<str>,
    esid: Option<Box<str>>,
    edition: SpecEdition,
    flags: TestFlags,
    information: Box<str>,
    expected_outcome: Outcome,
    features: FxHashSet<Box<str>>,
    includes: FxHashSet<Box<str>>,
    locale: Locale,
    ignored: bool,
}

impl Test {
    /// Creates a new test.
    fn new<N, C>(name: N, path: C, metadata: MetaData) -> Result<Self>
    where
        N: Into<Box<str>>,
        C: Into<Box<Path>>,
    {
        let edition = SpecEdition::from_test_metadata(&metadata)
            .map_err(|feats| eyre!("test metadata contained unknown features: {feats:?}"))?;

        Ok(Self {
            edition,
            name: name.into(),
            description: metadata.description,
            esid: metadata.esid,
            flags: metadata.flags.into(),
            information: metadata.info,
            features: metadata.features.into_vec().into_iter().collect(),
            expected_outcome: Outcome::from(metadata.negative),
            includes: metadata.includes.into_vec().into_iter().collect(),
            locale: metadata.locale,
            path: path.into(),
            ignored: false,
        })
    }

    /// Sets the test as ignored.
    #[inline]
    fn set_ignored(&mut self) {
        self.ignored = true;
    }

    /// Checks if this is a module test.
    #[inline]
    const fn is_module(&self) -> bool {
        self.flags.contains(TestFlags::MODULE)
    }
}

/// An outcome for a test.
#[derive(Debug, Default, Clone)]
enum Outcome {
    #[default]
    Positive,
    Negative {
        phase: Phase,
        error_type: ErrorType,
    },
}

impl From<Option<Negative>> for Outcome {
    fn from(neg: Option<Negative>) -> Self {
        neg.map(|neg| Self::Negative {
            phase: neg.phase,
            error_type: neg.error_type,
        })
        .unwrap_or_default()
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct TestFlags: u16 {
        const STRICT = 0b0_0000_0001;
        const NO_STRICT = 0b0_0000_0010;
        const MODULE = 0b0_0000_0100;
        const RAW = 0b0_0000_1000;
        const ASYNC = 0b0_0001_0000;
        const GENERATED = 0b0_0010_0000;
        const CAN_BLOCK_IS_FALSE = 0b0_0100_0000;
        const CAN_BLOCK_IS_TRUE = 0b0_1000_0000;
        const NON_DETERMINISTIC = 0b1_0000_0000;
    }
}

impl Default for TestFlags {
    fn default() -> Self {
        Self::STRICT | Self::NO_STRICT
    }
}

impl From<TestFlag> for TestFlags {
    fn from(flag: TestFlag) -> Self {
        match flag {
            TestFlag::OnlyStrict => Self::STRICT,
            TestFlag::NoStrict => Self::NO_STRICT,
            TestFlag::Module => Self::MODULE,
            TestFlag::Raw => Self::RAW,
            TestFlag::Async => Self::ASYNC,
            TestFlag::Generated => Self::GENERATED,
            TestFlag::CanBlockIsFalse => Self::CAN_BLOCK_IS_FALSE,
            TestFlag::CanBlockIsTrue => Self::CAN_BLOCK_IS_TRUE,
            TestFlag::NonDeterministic => Self::NON_DETERMINISTIC,
        }
    }
}

impl<T> From<T> for TestFlags
where
    T: AsRef<[TestFlag]>,
{
    fn from(flags: T) -> Self {
        let flags = flags.as_ref();
        if flags.is_empty() {
            Self::default()
        } else {
            let mut result = Self::empty();
            for flag in flags {
                result |= Self::from(*flag);
            }

            if !result.intersects(Self::default()) {
                result |= Self::default();
            }

            result
        }
    }
}

impl<'de> Deserialize<'de> for TestFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FlagsVisitor;

        impl<'de> Visitor<'de> for FlagsVisitor {
            type Value = TestFlags;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "a sequence of flags")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut flags = TestFlags::empty();
                while let Some(elem) = seq.next_element::<TestFlag>()? {
                    flags |= elem.into();
                }
                Ok(flags)
            }
        }

        struct RawFlagsVisitor;

        impl Visitor<'_> for RawFlagsVisitor {
            type Value = TestFlags;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "a flags number")
            }

            fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                TestFlags::from_bits(v).ok_or_else(|| {
                    E::invalid_value(Unexpected::Unsigned(v.into()), &"a valid flag number")
                })
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_seq(FlagsVisitor)
        } else {
            deserializer.deserialize_u16(RawFlagsVisitor)
        }
    }
}

/// Phase for an error.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Parse,
    Resolution,
    Runtime,
}

/// Locale information structure.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(transparent)]
#[allow(dead_code)]
struct Locale {
    locale: Box<[Box<str>]>,
}

#[cfg(test)]
mod tests {
    use super::{Config, IgnoreEntry, Ignored, QuarantineConfig, Statistics, TestOutcomeResult};

    fn entry(pattern: &str) -> IgnoreEntry {
        IgnoreEntry {
            pattern: pattern.into(),
            reason: "r".into(),
            link: "l".into(),
            owner: "o".into(),
            added: "2026-10-02".into(),
            expiry: "2099-01-01".into(),
            work_item: "w".into(),
        }
    }

    #[test]
    fn match_test_returns_first_substring_hit() {
        let ignored = Ignored {
            tests: vec![entry("aaa"), entry("test/x")],
            features: Vec::new(),
            flags: super::TestFlags::empty(),
        };
        assert_eq!(
            ignored
                .match_test(std::path::Path::new("test/x/y.js"))
                .map(|e| &*e.pattern),
            Some("test/x")
        );
        assert!(
            ignored
                .match_test(std::path::Path::new("other.js"))
                .is_none()
        );
    }

    #[test]
    fn match_feature_prefers_exact_over_prefix() {
        let ignored = Ignored {
            tests: Vec::new(),
            features: vec![entry("Intl"), entry("Intl.DisplayNames")],
            flags: super::TestFlags::empty(),
        };
        assert_eq!(
            ignored
                .match_feature("Intl.DisplayNames")
                .map(|e| &*e.pattern),
            Some("Intl.DisplayNames")
        );
        assert_eq!(
            ignored.match_feature("Intl.Locale").map(|e| &*e.pattern),
            Some("Intl")
        );
        assert!(ignored.match_feature("Temporal").is_none());
    }

    #[test]
    fn failed_excludes_all_abnormal_outcomes() {
        let stats = Statistics {
            total: 100,
            passed: 80,
            ignored: 5,
            panic: 3,
            timeout: 2,
            crash: 1,
            harness_error: 1,
        };
        assert_eq!(stats.failed(), 8);
    }

    #[test]
    fn failed_saturates_on_inconsistent_input() {
        let stats = Statistics {
            total: 2,
            passed: 5,
            ..Statistics::default()
        };
        assert_eq!(stats.failed(), 0);
    }

    #[test]
    fn stats_add_covers_new_counters() {
        let lhs = Statistics {
            timeout: 1,
            crash: 2,
            harness_error: 3,
            ..Statistics::default()
        };
        let rhs = Statistics {
            timeout: 4,
            crash: 5,
            harness_error: 6,
            ..Statistics::default()
        };
        let sum = lhs + rhs;
        assert_eq!(sum.timeout, 5);
        assert_eq!(sum.crash, 7);
        assert_eq!(sum.harness_error, 9);
    }

    #[test]
    fn stats_reads_pre_p1_schema() {
        // P0 baselines carry only t/o/i/p; the new counters must default to 0.
        let stats: Statistics =
            serde_json::from_str(r#"{"t":10,"o":8,"i":1,"p":0}"#).expect("valid stats");
        assert_eq!(stats.total, 10);
        assert_eq!(stats.timeout, 0);
        assert_eq!(stats.crash, 0);
        assert_eq!(stats.harness_error, 0);
        assert_eq!(stats.failed(), 1);
    }

    #[test]
    fn outcome_letters_round_trip() {
        let cases = [
            (TestOutcomeResult::Passed, "\"O\""),
            (TestOutcomeResult::Ignored, "\"I\""),
            (TestOutcomeResult::Failed, "\"F\""),
            (TestOutcomeResult::Panic, "\"P\""),
            (TestOutcomeResult::Timeout, "\"T\""),
            (TestOutcomeResult::Crash, "\"C\""),
            (TestOutcomeResult::HarnessError, "\"H\""),
        ];
        for (outcome, letter) in cases {
            let json = serde_json::to_string(&outcome).expect("serialize");
            assert_eq!(json, letter);
            let back: TestOutcomeResult = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, outcome);
        }
    }

    #[test]
    fn abnormal_marks_only_abnormal_outcomes() {
        assert!(!TestOutcomeResult::Passed.is_abnormal());
        assert!(!TestOutcomeResult::Ignored.is_abnormal());
        assert!(!TestOutcomeResult::Failed.is_abnormal());
        assert!(TestOutcomeResult::Panic.is_abnormal());
        assert!(TestOutcomeResult::Timeout.is_abnormal());
        assert!(TestOutcomeResult::Crash.is_abnormal());
        assert!(TestOutcomeResult::HarnessError.is_abnormal());
    }

    fn audited_entry(pattern: &str) -> IgnoreEntry {
        IgnoreEntry {
            pattern: pattern.into(),
            reason: "reason".into(),
            link: "https://example.com".into(),
            owner: "owner".into(),
            added: "2026-10-02".into(),
            expiry: "2099-01-01".into(),
            work_item: "P2.x".into(),
        }
    }

    #[test]
    fn test_matching_stays_substring_based() {
        let ignored = Ignored {
            tests: vec![audited_entry("test/staging/sm/regress")],
            ..Ignored::default()
        };
        assert!(ignored.contains_test(std::path::Path::new(
            "/root/test262/test/staging/sm/regress/regress-1.js"
        )));
        assert!(!ignored.contains_test(std::path::Path::new(
            "/root/test262/test/built-ins/Array/a.js"
        )));
    }

    #[test]
    fn feature_matching_keeps_prefix_fallback() {
        let ignored = Ignored {
            features: vec![audited_entry("Intl")],
            ..Ignored::default()
        };
        assert!(ignored.contains_feature("Intl"));
        assert!(ignored.contains_feature("Intl.DurationFormat"));
        assert!(!ignored.contains_feature("Temporal"));
    }

    #[test]
    fn quarantine_matches_test_ids_by_substring() {
        let quarantine = QuarantineConfig {
            quarantine: vec![audited_entry("test/language/types/number")],
        };
        assert!(quarantine.contains("test/language/types/number/bigint-attrs"));
        assert!(!quarantine.contains("test/built-ins/Array/from"));
    }

    #[test]
    fn repo_config_parses_and_validates() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test262_config.toml");
        let input = std::fs::read_to_string(&root).expect("read repo config");
        let config: Config = toml::from_str(&input).expect("parse repo config");
        assert!(!config.commit().is_empty());
        let violations = super::audit::validate_config(config.ignored());
        assert!(violations.is_empty(), "{violations:?}");
        assert!(!config.ignored().tests.is_empty());
        assert!(!config.ignored().features.is_empty());
    }

    #[test]
    fn suite_root_names_are_test262_relative() {
        assert_eq!(
            super::suite_root_name(std::path::Path::new("test")).as_deref(),
            Some("test")
        );
        assert_eq!(
            super::suite_root_name(std::path::Path::new("test/built-ins/Array")).as_deref(),
            Some("test/built-ins/Array")
        );
        assert_eq!(
            super::suite_root_name(std::path::Path::new("test/")).as_deref(),
            Some("test")
        );
        assert!(super::suite_root_name(std::path::Path::new(".")).is_none());
    }

    #[test]
    fn repo_quarantine_parses_and_validates() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test262_quarantine.toml");
        let input = std::fs::read_to_string(&root).expect("read repo quarantine");
        let quarantine: QuarantineConfig = toml::from_str(&input).expect("parse repo quarantine");
        let violations = super::audit::validate_quarantine(&quarantine);
        assert!(violations.is_empty(), "{violations:?}");
    }
}
