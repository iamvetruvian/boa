//! Conformance-gap matrix (`report` subcommand, P2.1).
//!
//! Joins per-test outcomes from a run's `latest.json` with live Test262
//! metadata (features, flags, edition, `esid`), attributes ignored tests to
//! their config entries, groups gap tests into spec areas, and writes the
//! machine-readable `conformance-gap.json` (P1.1 store conventions) plus the
//! human-readable `conformance-gap.md`. With `--check`, the freshly built
//! matrix is diffed against a previous one to enforce the trend ratchet:
//! passes never decrease, failures/abnormals/ignores never grow.
//!
//! Design notes:
//!
//! - Area *triage* (suspected layer, owning work item, status) is curated in
//!   `docs/gap-areas.toml` and joined at render time, so regenerating the
//!   matrix never wipes human judgment.
//! - All maps serialize through `BTreeMap`, so regeneration is
//!   byte-deterministic apart from the `updated` timestamp.
//! - Only gap tests (non-passed outcomes) become `entries`; the matrix is
//!   the gap, not the corpus.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::{
    Config, IgnoreEntry, Statistics, Test, TestFlags, TestOutcomeResult, TestSuite,
    edition::SpecEdition,
    read::read_suite,
    results::{
        CRASHES_FILE_NAME, LATEST_FILE_NAME, OutcomeMap, ResultInfo, flatten_suite, utc_now_rfc3339,
    },
};

/// Matrix output file names.
const MATRIX_JSON: &str = "conformance-gap.json";
const MATRIX_MD: &str = "conformance-gap.md";

/// Current gap-matrix schema version (P1.1 conventions: readers reject
/// anything else loudly).
const SCHEMA_VERSION: u8 = 1;

/// Curated area triage: suspected layer, owning work item, workflow status.
#[derive(Debug, Clone, Deserialize)]
struct AreaTriage {
    layer: String,
    work_item: String,
    status: AreaStatus,
    #[serde(default)]
    notes: String,
}

/// Workflow status of an area in the curated triage file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum AreaStatus {
    Open,
    InProgress,
    Closed,
    Deferred,
}

impl std::fmt::Display for AreaStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Open => "open",
            Self::InProgress => "in-progress",
            Self::Closed => "closed",
            Self::Deferred => "deferred",
        };
        f.write_str(text)
    }
}

/// The curated `gap-areas.toml` file: area name -> triage record.
#[derive(Debug, Default, Deserialize)]
struct AreaFile {
    #[serde(default)]
    areas: FxHashMap<String, AreaTriage>,
}

/// The ignore entry attributed to an ignored test.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IgnoreRef {
    pattern: String,
    reason: String,
    work_item: String,
}

/// One gap-test entry: a failing/ignored/abnormal test with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GapEntry {
    /// Outcome letter (`F/I/P/T/C/H`), shared tester codes.
    #[serde(rename = "r")]
    outcome: TestOutcomeResult,
    area: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    features: Vec<String>,
    #[serde(rename = "v")]
    edition: SpecEdition,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    esid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    ignore: Option<IgnoreRef>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    signature: Option<String>,
}

/// Per-area rollup: machine counts plus curated triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AreaSummary {
    total: usize,
    passed: usize,
    failed: usize,
    ignored: usize,
    abnormal: usize,
    layer: String,
    work_item: String,
    status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
}

/// The gap matrix: full-run counts plus every gap test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GapMatrix {
    schema_version: u8,
    tool: String,
    tool_version: String,
    updated: String,
    test262_commit: String,
    suite_root: String,
    counts: Statistics,
    unmatched_no_outcome: usize,
    unmatched_no_metadata: usize,
    areas: BTreeMap<String, AreaSummary>,
    entries: BTreeMap<String, GapEntry>,
}

/// Maps a metadata suite path (`test/built-ins/Array/from`, no test name)
/// to its spec area.
fn area_of_suite(suite: &str) -> String {
    let mut parts = suite.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("test"), Some("built-ins"), Some(builtin)) => format!("builtin:{builtin}"),
        (Some("test"), Some("built-ins"), None) => "builtin:misc".to_string(),
        (Some("test"), Some("language"), Some(seg)) => format!("language:{seg}"),
        (Some("test"), Some("language"), None) => "language:misc".to_string(),
        (Some("test"), Some("intl402"), Some(seg)) => format!("intl:{seg}"),
        (Some("test"), Some("intl402"), None) => "intl:misc".to_string(),
        (Some("test"), Some("staging"), Some(seg)) => format!("staging:{seg}"),
        (Some("test"), Some("staging"), None) => "staging:misc".to_string(),
        (Some("test"), Some("annexB"), Some(seg)) => format!("annexB:{seg}"),
        (Some("test"), Some("annexB"), None) => "annexB:misc".to_string(),
        (Some("test"), Some("harness"), _) => "harness".to_string(),
        _ => "other".to_string(),
    }
}

/// Effective run flags of a test, in fixed order.
fn flag_names(flags: TestFlags) -> Vec<String> {
    const NAMES: [(TestFlags, &str); 9] = [
        (TestFlags::STRICT, "strict"),
        (TestFlags::NO_STRICT, "no_strict"),
        (TestFlags::MODULE, "module"),
        (TestFlags::RAW, "raw"),
        (TestFlags::ASYNC, "async"),
        (TestFlags::GENERATED, "generated"),
        (TestFlags::CAN_BLOCK_IS_FALSE, "can_block_is_false"),
        (TestFlags::CAN_BLOCK_IS_TRUE, "can_block_is_true"),
        (TestFlags::NON_DETERMINISTIC, "non_deterministic"),
    ];
    NAMES
        .iter()
        .filter(|(bit, _)| flags.contains(*bit))
        .map(|(_, name)| (*name).to_string())
        .collect()
}

/// Attributes an ignored test to its config entry, mirroring `read_suite`
/// precedence: test-path match first, then the first (sorted) matching
/// feature, then the flags list. Returns `None` when nothing matches (e.g.
/// an edition-filtered run), which the matrix records honestly.
fn attribute_ignore(test: &Test, config: &Config) -> Option<IgnoreRef> {
    let ignored = config.ignored();
    if let Some(entry) = ignored.match_test(&test.path) {
        return Some(ignore_ref(entry));
    }
    let mut features: Vec<&str> = test.features.iter().map(|f| &**f).collect();
    features.sort_unstable();
    for feature in features {
        if let Some(entry) = ignored.match_feature(feature) {
            return Some(ignore_ref(entry));
        }
    }
    if ignored.contains_any_flag(test.flags) {
        return Some(IgnoreRef {
            pattern: "[flags]".to_string(),
            reason: "ignored by flags list".to_string(),
            work_item: String::new(),
        });
    }
    None
}

fn ignore_ref(entry: &IgnoreEntry) -> IgnoreRef {
    IgnoreRef {
        pattern: entry.pattern.to_string(),
        reason: entry.reason.to_string(),
        work_item: entry.work_item.to_string(),
    }
}

/// Loads `test id -> crash signature` from a `crashes.json` next to the
/// results file. Missing or unparsable files yield an empty map: runs
/// without crash accounting (like the P0 baseline) still produce a matrix.
fn load_crash_signatures(latest_path: &Path) -> FxHashMap<String, String> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        records: Vec<Record>,
    }
    #[derive(Deserialize)]
    struct Record {
        #[serde(default)]
        test: String,
        #[serde(default)]
        signature: String,
    }

    let dir = if latest_path.is_dir() {
        latest_path.to_path_buf()
    } else {
        latest_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let file = dir.join(CRASHES_FILE_NAME);
    let Ok(input) = fs::File::open(file) else {
        return FxHashMap::default();
    };
    let Ok(parsed): Result<File, _> = serde_json::from_reader(input) else {
        return FxHashMap::default();
    };
    parsed
        .records
        .into_iter()
        .filter(|r| !r.test.is_empty() && !r.signature.is_empty())
        .map(|r| (r.test, r.signature))
        .collect()
}

/// Builds the gap matrix from a run, writes `conformance-gap.json` +
/// `conformance-gap.md` into `out_dir`, and — when `check` holds a previous
/// matrix — enforces the trend ratchet. Returns `true` when the ratchet
/// breached (the caller exits nonzero).
pub(crate) fn generate_report(
    run: &Path,
    test262_root: &Path,
    config: &Config,
    areas_path: Option<&Path>,
    out_dir: &Path,
    check: Option<&Path>,
) -> Result<bool> {
    let latest_path = if run.is_dir() {
        run.join(LATEST_FILE_NAME)
    } else {
        run.to_path_buf()
    };
    let info: ResultInfo = serde_json::from_reader(
        fs::File::open(&latest_path)
            .wrap_err_with(|| eyre!("could not open `{}`", latest_path.display()))?,
    )
    .wrap_err("could not read the run results")?;
    let mut outcomes = OutcomeMap::default();
    flatten_suite(&info.results, &mut outcomes);
    let signatures = load_crash_signatures(&latest_path);

    let metadata_root = test262_root.join("test");
    if !metadata_root.is_dir() {
        return Err(eyre!(
            "test262 checkout has no `test/` dir: `{}`",
            test262_root.display()
        ));
    }
    let suite = read_suite(&metadata_root, config.ignored(), false)
        .wrap_err("could not read test metadata")?;

    let triage = load_area_file(areas_path)?;
    let mut acc = Accumulator::default();
    walk_metadata(&suite, "test", &outcomes, &signatures, config, &mut acc);

    let matrix = GapMatrix {
        schema_version: SCHEMA_VERSION,
        tool: "boa_tester".to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        updated: utc_now_rfc3339()?,
        test262_commit: info.test262_commit.to_string(),
        suite_root: info.results.name.to_string(),
        counts: info.results.stats,
        unmatched_no_outcome: acc.unmatched_no_outcome,
        unmatched_no_metadata: outcomes.len() - acc.matched,
        areas: summarize_areas(&acc, &triage),
        entries: acc.entries,
    };

    // Warn about curated areas that matched nothing (usually a typo).
    for name in triage.areas.keys() {
        if !matrix.areas.contains_key(name) {
            eprintln!("warning: curated area `{name}` matched no tests");
        }
    }

    fs::create_dir_all(out_dir)
        .wrap_err_with(|| eyre!("could not create `{}`", out_dir.display()))?;
    fs::write(
        out_dir.join(MATRIX_JSON),
        serde_json::to_string_pretty(&matrix).wrap_err("could not serialize matrix")?,
    )
    .wrap_err("could not write the matrix JSON")?;
    fs::write(out_dir.join(MATRIX_MD), render_markdown(&matrix))
        .wrap_err("could not write the matrix markdown")?;
    println!(
        "gap matrix: {} entries ({} failed, {} ignored, {} abnormal) in {} areas",
        matrix.entries.len(),
        matrix.counts.failed(),
        matrix.counts.ignored,
        matrix.counts.panic
            + matrix.counts.timeout
            + matrix.counts.crash
            + matrix.counts.harness_error,
        matrix.areas.len(),
    );

    let Some(check_path) = check else {
        return Ok(false);
    };
    let prev = GapMatrix::load(check_path)?;
    let violations = check_trend(&prev, &matrix);
    if violations.is_empty() {
        println!(
            "trend OK: {} passed (was {}), {} failed (was {}), {} ignored (was {})",
            matrix.counts.passed,
            prev.counts.passed,
            matrix.counts.failed(),
            prev.counts.failed(),
            matrix.counts.ignored,
            prev.counts.ignored,
        );
        return Ok(false);
    }
    println!(
        "TREND BREACH: {} violation(s) vs `{}`:",
        violations.len(),
        check_path.display()
    );
    for violation in &violations {
        println!("  - {violation}");
    }
    Ok(true)
}

/// Working state for the metadata walk.
#[derive(Default)]
struct Accumulator {
    entries: BTreeMap<String, GapEntry>,
    /// Per-area `(total, passed, failed, ignored, abnormal)`.
    areas: BTreeMap<String, (usize, usize, usize, usize, usize)>,
    unmatched_no_outcome: usize,
    matched: usize,
}

/// Walks the metadata tree, joining each test with its outcome.
#[allow(clippy::too_many_arguments)]
fn walk_metadata(
    suite: &TestSuite,
    suite_path: &str,
    outcomes: &OutcomeMap,
    signatures: &FxHashMap<String, String>,
    config: &Config,
    acc: &mut Accumulator,
) {
    let area = area_of_suite(suite_path);
    for test in &suite.tests {
        let id = format!("{suite_path}/{}", test.name);
        let slot = acc.areas.entry(area.clone()).or_default();
        slot.0 += 1;
        let Some(outcome) = outcomes.get(&id).copied() else {
            acc.unmatched_no_outcome += 1;
            continue;
        };
        acc.matched += 1;
        match outcome {
            TestOutcomeResult::Passed => {
                slot.1 += 1;
                continue;
            }
            TestOutcomeResult::Failed => slot.2 += 1,
            TestOutcomeResult::Ignored => slot.3 += 1,
            _ if outcome.is_abnormal() => slot.4 += 1,
            _ => {}
        }
        let mut features: Vec<String> = test.features.iter().map(ToString::to_string).collect();
        features.sort();
        acc.entries.insert(
            id.clone(),
            GapEntry {
                outcome,
                area: area.clone(),
                features,
                edition: test.edition,
                flags: flag_names(test.flags),
                esid: test.esid.as_deref().map(str::to_string),
                ignore: (outcome == TestOutcomeResult::Ignored)
                    .then(|| attribute_ignore(test, config))
                    .flatten(),
                signature: signatures.get(&id).cloned(),
            },
        );
    }
    for child in &suite.suites {
        walk_metadata(
            child,
            &format!("{suite_path}/{}", child.name),
            outcomes,
            signatures,
            config,
            acc,
        );
    }
}

/// Rolls per-area counts together with curated triage.
fn summarize_areas(acc: &Accumulator, triage: &AreaFile) -> BTreeMap<String, AreaSummary> {
    acc.areas
        .iter()
        .map(|(area, (total, passed, failed, ignored, abnormal))| {
            let (layer, work_item, status, notes) = triage.areas.get(area).map_or(
                if failed + ignored + abnormal == 0 {
                    (
                        "—".to_string(),
                        "—".to_string(),
                        "clean".to_string(),
                        String::new(),
                    )
                } else {
                    (
                        "unknown".to_string(),
                        "P2.untriaged".to_string(),
                        "open".to_string(),
                        String::new(),
                    )
                },
                |t| {
                    (
                        t.layer.clone(),
                        t.work_item.clone(),
                        t.status.to_string(),
                        t.notes.clone(),
                    )
                },
            );
            (
                area.clone(),
                AreaSummary {
                    total: *total,
                    passed: *passed,
                    failed: *failed,
                    ignored: *ignored,
                    abnormal: *abnormal,
                    layer,
                    work_item,
                    status,
                    notes,
                },
            )
        })
        .collect()
}

/// Loads the curated area file; `None` (or a missing file) means every area
/// renders untriaged.
fn load_area_file(path: Option<&Path>) -> Result<AreaFile> {
    let Some(path) = path else {
        return Ok(AreaFile::default());
    };
    if !path.exists() {
        eprintln!(
            "warning: area file `{}` not found; all areas render untriaged",
            path.display()
        );
        return Ok(AreaFile::default());
    }
    let input = fs::read_to_string(path)
        .wrap_err_with(|| eyre!("could not read area file `{}`", path.display()))?;
    toml::from_str(&input).wrap_err_with(|| eyre!("invalid area file `{}`", path.display()))
}

impl GapMatrix {
    /// Loads a previously written matrix, rejecting unknown schema versions.
    fn load(path: &Path) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_reader(
            fs::File::open(path).wrap_err_with(|| eyre!("could not open `{}`", path.display()))?,
        )
        .wrap_err("could not read the previous matrix")?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(SCHEMA_VERSION)) {
            let got = version.map_or_else(|| "missing".to_string(), |v| format!("v{v}"));
            return Err(eyre!(
                "unsupported matrix schema {got} (want v{SCHEMA_VERSION})"
            ));
        }
        serde_json::from_value(value).wrap_err("could not parse the previous matrix")
    }
}

/// Enforces the trend ratchet: passes never decrease; failures, ignores,
/// and every abnormal counter never grow; both matrices must cover the same
/// run (same suite root and total). Returns one message per violation.
fn check_trend(prev: &GapMatrix, new: &GapMatrix) -> Vec<String> {
    if prev.suite_root != new.suite_root {
        return vec![format!(
            "not comparable: suite root `{}` vs `{}`",
            prev.suite_root, new.suite_root
        )];
    }
    if prev.counts.total != new.counts.total {
        return vec![format!(
            "not comparable: total {} vs {} (full run vs subset?)",
            prev.counts.total, new.counts.total
        )];
    }
    let mut violations = Vec::new();
    if new.counts.passed < prev.counts.passed {
        violations.push(format!(
            "pass count regressed: {} -> {}",
            prev.counts.passed, new.counts.passed
        ));
    }
    if new.counts.failed() > prev.counts.failed() {
        violations.push(format!(
            "failures increased: {} -> {}",
            prev.counts.failed(),
            new.counts.failed()
        ));
    }
    if new.counts.ignored > prev.counts.ignored {
        violations.push(format!(
            "ignored grew: {} -> {}",
            prev.counts.ignored, new.counts.ignored
        ));
    }
    for (label, a, b) in [
        ("panics", prev.counts.panic, new.counts.panic),
        ("timeouts", prev.counts.timeout, new.counts.timeout),
        ("crashes", prev.counts.crash, new.counts.crash),
        (
            "harness errors",
            prev.counts.harness_error,
            new.counts.harness_error,
        ),
    ] {
        if b > a {
            violations.push(format!("{label} increased: {a} -> {b}"));
        }
    }
    if new.unmatched_no_outcome != 0 {
        violations.push(format!(
            "matrix is incomplete: {} tests lack outcomes",
            new.unmatched_no_outcome
        ));
    }
    if new.unmatched_no_metadata != 0 {
        violations.push(format!(
            "matrix is incomplete: {} outcomes lack metadata",
            new.unmatched_no_metadata
        ));
    }
    violations
}

/// Renders the human-readable matrix companion.
fn render_markdown(matrix: &GapMatrix) -> String {
    use std::fmt::Write as _;

    let mut md = String::new();
    let counts = &matrix.counts;
    let abnormal = counts.panic + counts.timeout + counts.crash + counts.harness_error;
    let _ = writeln!(md, "# Conformance gap matrix");
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "Generated by `boa_tester report` ({}, {}) from suite root `{}` at Test262 `{}`.",
        matrix.tool_version, matrix.updated, matrix.suite_root, matrix.test262_commit
    );
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "Regenerate: `boa_tester report --run <run-dir> -o docs --areas docs/gap-areas.toml`."
    );
    let _ = writeln!(
        md,
        "Area triage (layer/work item/status) is curated in `docs/gap-areas.toml` and never overwritten."
    );
    let _ = writeln!(md);
    let _ = writeln!(md, "## Totals");
    let _ = writeln!(md);
    md.push_str(
        "| Total | Passed | Failed | Ignored | Panics | Timeouts | Crashes | Harness errs |\n",
    );
    md.push_str(
        "|-------|--------|--------|---------|--------|----------|---------|--------------|\n",
    );
    let _ = writeln!(
        md,
        "| {} | {} | {} | {} | {} | {} | {} | {} |",
        counts.total,
        counts.passed,
        counts.failed(),
        counts.ignored,
        counts.panic,
        counts.timeout,
        counts.crash,
        counts.harness_error,
    );
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "Gap entries: {} ({} failed, {} ignored, {} abnormal; {:.2}% conformance). Unmatched: {} without outcomes, {} without metadata.",
        matrix.entries.len(),
        counts.failed(),
        counts.ignored,
        abnormal,
        100.0 * counts.passed as f64 / counts.total.max(1) as f64,
        matrix.unmatched_no_outcome,
        matrix.unmatched_no_metadata,
    );
    let _ = writeln!(md);
    let _ = writeln!(md, "## Areas (by gap size)");
    let _ = writeln!(md);
    md.push_str(
        "| Area | Total | Passed | Failed | Ignored | Abnormal | Layer | Work item | Status |\n",
    );
    md.push_str(
        "|------|-------|--------|--------|---------|----------|-------|-----------|--------|\n",
    );
    let mut areas: Vec<(&String, &AreaSummary)> = matrix.areas.iter().collect();
    areas.sort_by(|a, b| {
        let gap_a = a.1.failed + a.1.ignored + a.1.abnormal;
        let gap_b = b.1.failed + b.1.ignored + b.1.abnormal;
        gap_b.cmp(&gap_a).then_with(|| a.0.cmp(b.0))
    });
    for (area, summary) in areas {
        let _ = writeln!(
            md,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            area,
            summary.total,
            summary.passed,
            summary.failed,
            summary.ignored,
            summary.abnormal,
            summary.layer,
            summary.work_item,
            summary.status,
        );
    }
    let _ = writeln!(md);
    let _ = writeln!(md, "## Top gap features");
    let _ = writeln!(md);
    md.push_str("| Feature | Gap tests |\n");
    md.push_str("|---------|-----------|\n");
    let mut features: FxHashMap<&str, usize> = FxHashMap::default();
    for entry in matrix.entries.values() {
        for feature in &entry.features {
            *features.entry(feature).or_default() += 1;
        }
    }
    let mut features: Vec<(&str, usize)> = features.into_iter().collect();
    features.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (feature, count) in features.into_iter().take(20) {
        let _ = writeln!(md, "| {feature} | {count} |");
    }
    let _ = writeln!(md);
    let _ = writeln!(md, "## Ignore attribution");
    let _ = writeln!(md);
    md.push_str("| Pattern | Tests | Work item |\n");
    md.push_str("|---------|-------|-----------|\n");
    let mut patterns: FxHashMap<(&str, &str), usize> = FxHashMap::default();
    for entry in matrix.entries.values() {
        if let Some(ignore) = &entry.ignore {
            *patterns
                .entry((&ignore.pattern, &ignore.work_item))
                .or_default() += 1;
        }
    }
    let mut patterns: Vec<((&str, &str), usize)> = patterns.into_iter().collect();
    patterns.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for ((pattern, work_item), count) in patterns {
        let _ = writeln!(md, "| `{pattern}` | {count} | {work_item} |");
    }
    let noted: Vec<(&String, &AreaSummary)> = matrix
        .areas
        .iter()
        .filter(|(_, summary)| !summary.notes.is_empty())
        .collect();
    if !noted.is_empty() {
        let _ = writeln!(md);
        let _ = writeln!(md, "## Area notes");
        let _ = writeln!(md);
        for (area, summary) in noted {
            let _ = writeln!(md, "- **{}** ({}): {}", area, summary.status, summary.notes);
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_derive_from_suite_path() {
        let cases = [
            ("test/built-ins/Array/from", "builtin:Array"),
            ("test/built-ins/Array", "builtin:Array"),
            ("test/language/statements/try", "language:statements"),
            ("test/language/expressions", "language:expressions"),
            ("test/intl402/NumberFormat", "intl:NumberFormat"),
            ("test/staging/sm/regress", "staging:sm"),
            ("test/annexB/language", "annexB:language"),
            ("test/harness", "harness"),
            ("test/harness/deep/dir", "harness"),
            ("test/built-ins", "builtin:misc"),
            ("test/language", "language:misc"),
            ("test", "other"),
            ("something/else", "other"),
        ];
        for (suite, want) in cases {
            assert_eq!(area_of_suite(suite), want, "{suite}");
        }
    }

    #[test]
    fn flag_names_use_fixed_order() {
        assert_eq!(
            flag_names(TestFlags::ASYNC | TestFlags::MODULE),
            vec!["module".to_string(), "async".to_string()]
        );
        assert!(flag_names(TestFlags::empty()).is_empty());
        assert_eq!(
            flag_names(TestFlags::STRICT | TestFlags::NO_STRICT).len(),
            2
        );
    }

    fn matrix_with(counts: Statistics) -> GapMatrix {
        GapMatrix {
            schema_version: SCHEMA_VERSION,
            tool: "boa_tester".to_string(),
            tool_version: "test".to_string(),
            updated: "2026-10-02T00:00:00Z".to_string(),
            test262_commit: String::new(),
            suite_root: "test".to_string(),
            counts,
            unmatched_no_outcome: 0,
            unmatched_no_metadata: 0,
            areas: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }

    fn stats(passed: usize, failed_extra: usize, ignored: usize) -> Statistics {
        // failed() derives from the other counters; express failures that way.
        Statistics {
            total: passed + failed_extra + ignored,
            passed,
            ignored,
            ..Statistics::default()
        }
    }

    #[test]
    fn trend_check_passes_on_improvement_or_parity() {
        let prev = matrix_with(stats(100, 5, 10));
        let same = matrix_with(stats(100, 5, 10));
        assert!(check_trend(&prev, &same).is_empty());
        let better = matrix_with(stats(105, 3, 7));
        assert!(check_trend(&prev, &better).is_empty());
    }

    #[test]
    fn trend_check_fails_every_ratchet() {
        let prev = matrix_with(stats(100, 5, 10));
        let regressed = matrix_with(stats(99, 6, 10));
        assert!(
            check_trend(&prev, &regressed)
                .iter()
                .any(|v| v.contains("pass count regressed")),
            "pass decrease breaches"
        );
        let more_failed = matrix_with(stats(100, 6, 9));
        assert!(
            check_trend(&prev, &more_failed)
                .iter()
                .any(|v| v.contains("failures increased")),
            "failure growth breaches"
        );
        let more_ignored = matrix_with(stats(100, 4, 11));
        assert!(
            check_trend(&prev, &more_ignored)
                .iter()
                .any(|v| v.contains("ignored grew")),
            "ignore growth breaches"
        );
        for (label, counts) in [
            ("panics", stats(100, 5, 10)),
            ("timeouts", stats(100, 5, 10)),
            ("crashes", stats(100, 5, 10)),
            ("harness errors", stats(100, 5, 10)),
        ] {
            let mut worse = matrix_with(counts);
            match label {
                "panics" => worse.counts.panic = 1,
                "timeouts" => worse.counts.timeout = 1,
                "crashes" => worse.counts.crash = 1,
                _ => worse.counts.harness_error = 1,
            }
            assert!(
                check_trend(&prev, &worse).iter().any(|v| v.contains(label)),
                "{label} growth breaches"
            );
        }
    }

    #[test]
    fn trend_check_rejects_incomparable_and_incomplete() {
        let prev = matrix_with(stats(100, 5, 10));
        let mut subset = matrix_with(stats(10, 1, 1));
        subset.suite_root = "test/built-ins/Array".to_string();
        assert!(
            check_trend(&prev, &subset)
                .iter()
                .any(|v| v.contains("not comparable"))
        );
        let mut other_total = matrix_with(stats(100, 5, 10));
        other_total.counts.total = 999;
        assert!(
            check_trend(&prev, &other_total)
                .iter()
                .any(|v| v.contains("not comparable"))
        );
        let mut incomplete = matrix_with(stats(100, 5, 10));
        incomplete.unmatched_no_outcome = 3;
        assert!(
            check_trend(&prev, &incomplete)
                .iter()
                .any(|v| v.contains("lack outcomes"))
        );
        incomplete.unmatched_no_outcome = 0;
        incomplete.unmatched_no_metadata = 2;
        assert!(
            check_trend(&prev, &incomplete)
                .iter()
                .any(|v| v.contains("lack metadata"))
        );
    }

    #[test]
    fn matrix_loader_rejects_unknown_schema() {
        let dir = std::env::temp_dir().join(format!("boa_matrix_{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("gap.json");
        fs::write(&path, r#"{"schema_version":99}"#).expect("write");
        let err = GapMatrix::load(&path).expect_err("must reject v99");
        assert!(err.to_string().contains("v99"), "{err:?}");
        fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn gap_entries_use_shared_outcome_letters() {
        let entry = GapEntry {
            outcome: TestOutcomeResult::Timeout,
            area: "language:statements".to_string(),
            features: vec!["a".to_string()],
            edition: SpecEdition::ES5,
            flags: Vec::new(),
            esid: None,
            ignore: None,
            signature: None,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains(r#""r":"T""#), "{json}");
        assert!(json.contains(r#""v":5"#), "{json}");
        let back: GapEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.outcome, TestOutcomeResult::Timeout);
    }
}
