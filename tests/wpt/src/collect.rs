//! Counted WPT outcomes (P2.5): per-file verdicts, ignore list, JSON stores.
//!
//! Every executed WPT file appends one [`FileOutcome`] record (JSON line) to
//! `$WPT_OUT/records.jsonl` instead of panicking the Rust test on failure.
//! The `wpt-report` binary aggregates records into `summary.json` (P1.1
//! store conventions) and enforces the zero-untriaged gate: any file that
//! neither passes nor matches an audited ignore entry fails the gate.
//!
//! Outcome codes and counters come from `boa_outcomes`, shared with the
//! Test262 tester — the taxonomy is depended on, never duplicated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use boa_engine::value::TryFromJs;
use boa_engine::{js_error, Context, Finalize, JsData, JsResult, JsValue, Trace};
use boa_outcomes::{Statistics, TestOutcomeResult};
use serde::{Deserialize, Serialize};

/// The test status JavaScript type from WPT. This is defined in the test harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestStatus {
    Pass = 0,
    Fail = 1,
    Timeout = 2,
    NotRun = 3,
    PreconditionFailed = 4,
}

impl std::fmt::Display for TestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => write!(f, "PASS"),
            Self::Fail => write!(f, "FAIL"),
            Self::Timeout => write!(f, "TIMEOUT"),
            Self::NotRun => write!(f, "NOTRUN"),
            Self::PreconditionFailed => write!(f, "PRECONDITION FAILED"),
        }
    }
}

impl TryFromJs for TestStatus {
    fn try_from_js(value: &JsValue, context: &mut Context) -> JsResult<Self> {
        match value.to_u32(context) {
            Ok(0) => Ok(Self::Pass),
            Ok(1) => Ok(Self::Fail),
            Ok(2) => Ok(Self::Timeout),
            Ok(3) => Ok(Self::NotRun),
            Ok(4) => Ok(Self::PreconditionFailed),
            _ => Err(js_error!("Invalid test status")),
        }
    }
}

/// One subtest verdict collected from `result_callback__`.
#[derive(Debug, Clone)]
pub struct SubtestRecord {
    pub name: String,
    pub status: TestStatus,
    pub message: String,
}

/// Per-context subtest collector, stored as context data.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub struct SubtestCollector(
    // SAFETY: plain strings need no tracing.
    #[unsafe_ignore_trace] std::rc::Rc<std::cell::RefCell<Vec<SubtestRecord>>>,
);

impl SubtestCollector {
    pub fn new() -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(Vec::new())))
    }

    pub fn push(&self, record: SubtestRecord) {
        self.0.borrow_mut().push(record);
    }

    pub fn records(&self) -> Vec<SubtestRecord> {
        self.0.borrow().clone()
    }
}

impl Default for SubtestCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Computes the file outcome from its subtests: any failure (including
/// `NOTRUN` and `PRECONDITION FAILED`, which never demonstrate
/// conformance) fails the file; otherwise any timeout times it out.
pub fn file_outcome(subtests: &[SubtestRecord]) -> (TestOutcomeResult, Vec<String>) {
    let mut failed = Vec::new();
    let mut timed_out = false;
    for sub in subtests {
        match sub.status {
            TestStatus::Pass => {}
            TestStatus::Timeout => timed_out = true,
            TestStatus::Fail | TestStatus::NotRun | TestStatus::PreconditionFailed => {
                failed.push(sub.name.clone());
            }
        }
    }
    if !failed.is_empty() {
        (TestOutcomeResult::Failed, failed)
    } else if timed_out {
        (TestOutcomeResult::Timeout, Vec::new())
    } else {
        (TestOutcomeResult::Passed, Vec::new())
    }
}

/// One audited ignore entry (subset port of the Test262 ignore schema:
/// `tests` patterns plus justification; `link`/`work_item` optional).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WptIgnoreEntry {
    pub pattern: String,
    pub reason: String,
    pub owner: String,
    pub expiry: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub work_item: String,
}

/// The WPT tester config: pin plus ignore list.
#[derive(Debug, Clone, Deserialize)]
pub struct WptConfig {
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub ignored: WptIgnored,
}

/// Ignore list: substring patterns over WPT-relative paths.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WptIgnored {
    #[serde(default)]
    pub tests: Vec<WptIgnoreEntry>,
}

impl WptIgnored {
    /// First entry whose pattern is a substring of the path.
    pub fn match_test(&self, wpt_path: &str) -> Option<&WptIgnoreEntry> {
        self.tests
            .iter()
            .find(|entry| wpt_path.contains(&entry.pattern))
    }
}

/// Loads the WPT config once per process.
pub fn config() -> &'static WptConfig {
    static CONFIG: OnceLock<WptConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_wpt_config.toml");
        let input = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("could not read {}: {err}", path.display()));
        toml::from_str(&input).expect("invalid test_wpt_config.toml")
    })
}

/// WPT-relative path with forward slashes for stable ids and matching.
pub fn wpt_relative(path: &Path, wpt_root: &Path) -> String {
    path.strip_prefix(wpt_root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The ignore entry attributed to a skipped file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreRef {
    pub pattern: String,
    pub reason: String,
    pub work_item: String,
}

/// One file's verdict: a line in `records.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOutcome {
    pub file: String,
    pub outcome: TestOutcomeResult,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore: Option<IgnoreRef>,
}

impl FileOutcome {
    pub fn ignored(file: String, entry: &WptIgnoreEntry) -> Self {
        Self {
            file,
            outcome: TestOutcomeResult::Ignored,
            failed: Vec::new(),
            message: None,
            ignore: Some(IgnoreRef {
                pattern: entry.pattern.clone(),
                reason: entry.reason.clone(),
                work_item: entry.work_item.clone(),
            }),
        }
    }
}

/// Appends one record line, creating the output dir. Records from parallel
/// test threads serialize on a process-wide lock so lines never interleave.
pub fn append_record(out_dir: &Path, record: &FileOutcome) {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();
    std::fs::create_dir_all(out_dir).expect("could not create WPT_OUT");
    let line = serde_json::to_string(record).expect("could not serialize record");
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out_dir.join("records.jsonl"))
        .expect("could not open records.jsonl");
    writeln!(file, "{line}").expect("could not write record");
}

/// Output dir: `$WPT_OUT`, else a `wpt-results` dir under the workspace target dir.
pub fn out_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WPT_OUT") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/wpt-results")
}

/// Aggregates records into counts plus zero-untriaged breaches: any file
/// that neither passes nor carries a live ignore entry breaches, as does an
/// ignored file whose pattern no longer exists in the config. Last record
/// wins per file.
pub fn aggregate(records: &[FileOutcome], config: &WptConfig) -> (Statistics, Vec<String>) {
    let mut files: BTreeMap<&str, &FileOutcome> = BTreeMap::new();
    for record in records {
        files.insert(record.file.as_str(), record);
    }
    let mut counts = Statistics::default();
    let mut breaches = Vec::new();
    for (file, record) in &files {
        counts.total += 1;
        match record.outcome {
            TestOutcomeResult::Passed => counts.passed += 1,
            TestOutcomeResult::Ignored => {
                counts.ignored += 1;
                let stale = record.ignore.as_ref().is_some_and(|ignore| {
                    !config
                        .ignored
                        .tests
                        .iter()
                        .any(|entry| entry.pattern == ignore.pattern)
                });
                if stale {
                    breaches.push(format!("stale ignore on {file}"));
                }
            }
            TestOutcomeResult::Failed => breaches.push(format!("{file}: failed")),
            TestOutcomeResult::Panic => breaches.push(format!("{file}: panicked")),
            TestOutcomeResult::Timeout => breaches.push(format!("{file}: timed out")),
            TestOutcomeResult::Crash => breaches.push(format!("{file}: crashed")),
            TestOutcomeResult::HarnessError => {
                counts.harness_error += 1;
                breaches.push(format!("{file}: harness error"));
            }
        }
    }
    (counts, breaches)
}

/// The aggregated `summary.json` (P1.1 store conventions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WptSummary {
    pub schema_version: u8,
    pub tool: String,
    pub tool_version: String,
    pub updated: String,
    pub wpt_rev: String,
    pub counts: Statistics,
    pub files: BTreeMap<String, FileOutcome>,
}

pub const SCHEMA_VERSION: u8 = 1;

pub fn utc_now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("could not format time")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(name: &str, status: TestStatus) -> SubtestRecord {
        SubtestRecord {
            name: name.to_string(),
            status,
            message: String::new(),
        }
    }

    #[test]
    fn outcome_precedence_is_fail_then_timeout() {
        let (o, failed) = file_outcome(&[sub("a", TestStatus::Pass)]);
        assert_eq!(o, TestOutcomeResult::Passed);
        assert!(failed.is_empty());
        let (o, failed) =
            file_outcome(&[sub("a", TestStatus::Timeout), sub("b", TestStatus::Fail)]);
        assert_eq!(o, TestOutcomeResult::Failed);
        assert_eq!(failed, vec!["b".to_string()]);
        let (o, _) = file_outcome(&[sub("a", TestStatus::Timeout)]);
        assert_eq!(o, TestOutcomeResult::Timeout);
        let (o, _) = file_outcome(&[sub("a", TestStatus::NotRun)]);
        assert_eq!(o, TestOutcomeResult::Failed);
        let (o, _) = file_outcome(&[sub("a", TestStatus::PreconditionFailed)]);
        assert_eq!(o, TestOutcomeResult::Failed);
    }

    #[test]
    fn ignore_matches_first_substring_hit() {
        let config = WptConfig {
            rev: None,
            ignored: WptIgnored {
                tests: vec![
                    WptIgnoreEntry {
                        pattern: "aaa".to_string(),
                        reason: "r".to_string(),
                        owner: "o".to_string(),
                        expiry: "2099-01-01".to_string(),
                        link: String::new(),
                        work_item: String::new(),
                    },
                    WptIgnoreEntry {
                        pattern: "console/".to_string(),
                        reason: "r".to_string(),
                        owner: "o".to_string(),
                        expiry: "2099-01-01".to_string(),
                        link: String::new(),
                        work_item: String::new(),
                    },
                ],
            },
        };
        assert_eq!(
            config
                .ignored
                .match_test("console/foo.any.js")
                .map(|e| e.pattern.as_str()),
            Some("console/")
        );
        assert!(config.ignored.match_test("url/foo.any.js").is_none());
    }

    fn record(file: &str, outcome: TestOutcomeResult) -> FileOutcome {
        FileOutcome {
            file: file.to_string(),
            outcome,
            failed: Vec::new(),
            message: None,
            ignore: None,
        }
    }

    #[test]
    fn gate_breaches_on_untriaged_and_stale_ignores() {
        let config = WptConfig {
            rev: None,
            ignored: WptIgnored { tests: Vec::new() },
        };
        let records = vec![
            record("a.any.js", TestOutcomeResult::Passed),
            record("b.any.js", TestOutcomeResult::Failed),
            record("c.any.js", TestOutcomeResult::Timeout),
        ];
        let (counts, breaches) = aggregate(&records, &config);
        assert_eq!(counts.total, 3);
        assert_eq!(counts.passed, 1);
        assert_eq!(breaches.len(), 2);

        let mut stale = record("d.any.js", TestOutcomeResult::Ignored);
        stale.ignore = Some(IgnoreRef {
            pattern: "gone".to_string(),
            reason: String::new(),
            work_item: String::new(),
        });
        let (_, breaches) = aggregate(&[stale], &config);
        assert_eq!(breaches.len(), 1);
        assert!(breaches[0].contains("stale ignore"));
    }

    #[test]
    fn wpt_relative_uses_forward_slashes() {
        let root = Path::new("/wpt");
        assert_eq!(
            wpt_relative(Path::new("/wpt/console/a.any.js"), root),
            "console/a.any.js"
        );
    }
}
