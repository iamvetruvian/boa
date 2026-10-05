//! Triage queue: every mismatch with its spec-anchored verdict.
//!
//! The queue is a P1.1-convention JSON store (`schema_version`, `tool`,
//! `tool_version`, UTC `updated`). Entries key on stable case ids; a `run`
//! merges fresh mismatches into the existing queue, carrying verdicts
//! forward only when the field diffs are unchanged (a changed mismatch is
//! re-triaged, never silently kept). `triage --check` breaches on any
//! `verdict: null` entry — the zero-untriaged gate.
//!
//! Verdicts are spec-anchored by construction: majority vote is never a
//! verdict. Each variant carries the evidence its closure needs.

use std::{collections::BTreeMap, path::Path};

use boa_outcomes::TestOutcomeResult;
use serde::{Deserialize, Serialize};

use crate::{capture::RawObservation, normalize::FieldDiff};

/// Queue schema version (readers reject anything else loudly).
pub const QUEUE_SCHEMA_VERSION: u8 = 1;

/// Oracle identity recorded with every queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRecord {
    /// Adapter kind (`jsshell`, `node`).
    pub kind: String,
    /// Binary path used for the run.
    pub binary: String,
    /// Probed version string (checked against the pin at run time).
    pub version: String,
}

/// Spec-anchored verdict closing a mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Verdict {
    /// Boa is wrong: cites the spec section, links the regression entry.
    BoaBug {
        /// Full `https://tc39.es/…` fragment URL.
        spec: String,
        /// Regression-DB entry id (empty until the fix-loop files it).
        #[serde(default)]
        regression: String,
    },
    /// Pinned-oracle quirk: documented, re-checked on oracle upgrades.
    OracleQuirk {
        /// Pinned oracle the quirk was observed on (`kind version`).
        oracle: String,
        /// Why the oracle side is at fault (or legitimately divergent).
        note: String,
    },
    /// Genuinely ambiguous spec text: filing + audited expiry.
    SpecAmbiguity {
        /// Full `https://tc39.es/…` fragment URL.
        spec: String,
        /// Upstream issue/test link (empty until filed; humans file).
        #[serde(default)]
        filing: String,
        /// Re-review trigger: `YYYY-MM-DD` or `re-review:<ref>`.
        expiry: String,
    },
}

/// One queued mismatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    /// Stable case id.
    pub id: String,
    /// Case origin (`test262`, `regression`, `adversarial`).
    pub origin: String,
    /// Suspected area ([`area_of`] mapping).
    pub area: String,
    /// Boa side outcome.
    pub boa_outcome: TestOutcomeResult,
    /// Oracle side outcome.
    pub oracle_outcome: TestOutcomeResult,
    /// Boa side summary (empty when observed normally).
    pub boa_text: String,
    /// Oracle side summary (empty when observed normally).
    pub oracle_text: String,
    /// Normalized field diffs (empty for abnormal asymmetries).
    #[serde(default)]
    pub field_diffs: Vec<FieldDiff>,
    /// Minimized mismatch-preserving program.
    pub minimized_program: String,
    /// Minimizer that produced it (`line-reduce-v2`, later `tmin`).
    pub minimized_by: String,
    /// Minimized test-body line count (harness excluded; the drill budget
    /// asserts every entry fits [`MINIMIZED_BODY_BUDGET`](crate::reduce::MINIMIZED_BODY_BUDGET)).
    pub minimized_lines: usize,
    /// Boa observation when the driver emitted valid markers.
    #[serde(default)]
    pub boa_observation: Option<RawObservation>,
    /// Oracle observation when the driver emitted valid markers.
    #[serde(default)]
    pub oracle_observation: Option<RawObservation>,
    /// Closing verdict (`None` = untriaged, breaches `--check`).
    #[serde(default)]
    pub verdict: Option<Verdict>,
}

/// The triage-queue store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageQueue {
    /// Schema version (must be [`QUEUE_SCHEMA_VERSION`]).
    pub schema_version: u8,
    /// Writer identity.
    pub tool: String,
    /// Writer version.
    pub tool_version: String,
    /// UTC RFC 3339 update time.
    pub updated: String,
    /// Oracle identity for this queue.
    pub oracle: OracleRecord,
    /// Corpus name that produced the queue.
    pub corpus: String,
    /// Mismatches keyed by case id.
    #[serde(default)]
    pub entries: BTreeMap<String, QueueEntry>,
}

impl TriageQueue {
    /// Creates an empty queue for an oracle + corpus.
    pub fn empty(oracle: OracleRecord, corpus: &str) -> Self {
        Self {
            schema_version: QUEUE_SCHEMA_VERSION,
            tool: "boa_differential".to_owned(),
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            updated: String::new(),
            oracle,
            corpus: corpus.to_owned(),
            entries: BTreeMap::new(),
        }
    }

    /// Loads a queue, rejecting unknown schemas loudly.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("could not read queue: {err}"))?;
        let queue: Self =
            serde_json::from_str(&text).map_err(|err| format!("invalid queue: {err}"))?;
        if queue.schema_version != QUEUE_SCHEMA_VERSION {
            return Err(format!("unsupported queue schema {}", queue.schema_version));
        }
        Ok(queue)
    }

    /// Saves the queue with a fresh timestamp.
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.updated = utc_now_rfc3339().map_err(|err| format!("could not stamp time: {err}"))?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| format!("could not encode queue: {err}"))?;
        std::fs::write(path, text).map_err(|err| format!("could not write queue: {err}"))?;
        Ok(())
    }

    /// Ids without a verdict (breach `--check`).
    pub fn untriaged(&self) -> Vec<&str> {
        self.entries
            .values()
            .filter(|entry| entry.verdict.is_none())
            .map(|entry| entry.id.as_str())
            .collect()
    }

    /// Sets `verdict` on entries matching `area` and/or id `pattern`.
    ///
    /// At least one filter is required; returns the matched count.
    pub fn set_verdict(
        &mut self,
        area: Option<&str>,
        pattern: Option<&str>,
        fields: Option<&[String]>,
        verdict: &Verdict,
    ) -> usize {
        let mut matched = 0;
        for entry in self.entries.values_mut() {
            if area.is_some_and(|area| entry.area != area) {
                continue;
            }
            if pattern.is_some_and(|pattern| !entry.id.contains(pattern)) {
                continue;
            }
            // Field-set filter is exact: mismatch clusters ARE field sets,
            // so triage notes stay precise per cluster.
            if fields.is_some_and(|fields| {
                let mut entry_fields: Vec<&str> = entry
                    .field_diffs
                    .iter()
                    .map(|diff| diff.field.as_str())
                    .collect();
                entry_fields.sort_unstable();
                let mut wanted: Vec<&str> = fields.iter().map(String::as_str).collect();
                wanted.sort_unstable();
                entry_fields != wanted
            }) {
                continue;
            }
            entry.verdict = Some(verdict.clone());
            matched += 1;
        }
        matched
    }

    /// Resets `oracle-quirk` verdicts to untriaged (oracle-upgrade drill).
    ///
    /// Oracle quirks are pinned-oracle-specific by definition; a re-pin must
    /// re-confirm each one against the new oracle instead of inheriting it.
    /// Returns the reset count.
    pub fn reset_oracle_quirks(&mut self) -> usize {
        let mut reset = 0;
        for entry in self.entries.values_mut() {
            if matches!(entry.verdict, Some(Verdict::OracleQuirk { .. })) {
                entry.verdict = None;
                reset += 1;
            }
        }
        reset
    }
}

/// Maps a case id to its suspected area.
///
/// Test262 ids reuse the P2 path-prefix mapping (`builtin:<B>`,
/// `language:<seg>`, `intl:<seg>`, `staging:<seg>`, `annexB:<seg>`);
/// other origins map to their origin name.
pub fn area_of(case_id: &str) -> String {
    let id = case_id.split('#').next().unwrap_or(case_id);
    if let Some(rest) = id.strip_prefix("regression:") {
        let _ = rest;
        return "regression".to_owned();
    }
    if let Some(rest) = id.strip_prefix("adversarial:") {
        let _ = rest;
        return "adversarial".to_owned();
    }
    let mut parts = id.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("test"), Some("built-ins"), Some(builtin)) => format!("builtin:{builtin}"),
        (Some("test"), Some("language"), Some(seg)) => format!("language:{seg}"),
        (Some("test"), Some("intl402"), Some(seg)) => format!("intl:{seg}"),
        (Some("test"), Some("staging"), Some(seg)) => format!("staging:{seg}"),
        (Some("test"), Some("annexB"), Some(seg)) => format!("annexB:{seg}"),
        (Some("test"), _, _) => "test:misc".to_owned(),
        _ => "other".to_owned(),
    }
}

/// Current UTC time formatted as RFC 3339 (tester `results.rs` precedent).
fn utc_now_rfc3339() -> Result<String, time::error::Format> {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc().format(&Rfc3339)
}

/// One committed verdict: a triaged mismatch signature.
///
/// Verdicts persist across runs in `corpora/verdicts.toml` (curated,
/// reviewable, mirroring the P2 `gap-areas.toml` precedent); `run
/// --verdicts` pre-applies them to fresh mismatches with identical
/// signatures, so CI gates on zero *untriaged* rather than zero mismatches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictRecord {
    /// Stable case id.
    pub id: String,
    /// Suspected area (informational; matching uses `id` + signature).
    pub area: String,
    /// Boa side outcome when triaged (letter code).
    pub boa_outcome: TestOutcomeResult,
    /// Oracle side outcome when triaged (letter code).
    pub oracle_outcome: TestOutcomeResult,
    /// Sorted field-diff names when triaged.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Verdict kind (`boa-bug`, `oracle-quirk`, `spec-ambiguity`).
    pub kind: String,
    /// Spec URL (`boa-bug`, `spec-ambiguity`).
    #[serde(default)]
    pub spec: String,
    /// Regression-DB id (`boa-bug`).
    #[serde(default)]
    pub regression: String,
    /// Pinned oracle observed on (`oracle-quirk`).
    #[serde(default)]
    pub oracle: String,
    /// Quirk note (`oracle-quirk`).
    #[serde(default)]
    pub note: String,
    /// Upstream filing link (`spec-ambiguity`).
    #[serde(default)]
    pub filing: String,
    /// Re-review trigger (`spec-ambiguity`).
    #[serde(default)]
    pub expiry: String,
}

impl VerdictRecord {
    /// Builds the [`Verdict`] (validates kind + required fields).
    pub fn to_verdict(&self) -> Result<Verdict, String> {
        match self.kind.as_str() {
            "boa-bug" => {
                if self.spec.is_empty() {
                    return Err(format!("{}: boa-bug requires spec", self.id));
                }
                Ok(Verdict::BoaBug {
                    spec: self.spec.clone(),
                    regression: self.regression.clone(),
                })
            }
            "oracle-quirk" => {
                if self.note.is_empty() {
                    return Err(format!("{}: oracle-quirk requires note", self.id));
                }
                Ok(Verdict::OracleQuirk {
                    oracle: self.oracle.clone(),
                    note: self.note.clone(),
                })
            }
            "spec-ambiguity" => {
                if self.spec.is_empty() || self.expiry.is_empty() {
                    return Err(format!(
                        "{}: spec-ambiguity requires spec + expiry",
                        self.id
                    ));
                }
                Ok(Verdict::SpecAmbiguity {
                    spec: self.spec.clone(),
                    filing: self.filing.clone(),
                    expiry: self.expiry.clone(),
                })
            }
            kind => Err(format!("{}: unknown verdict kind {kind:?}", self.id)),
        }
    }

    /// Builds a record from a triaged queue entry.
    fn from_entry(entry: &QueueEntry, verdict: &Verdict) -> Self {
        let mut fields: Vec<String> = entry
            .field_diffs
            .iter()
            .map(|diff| diff.field.clone())
            .collect();
        fields.sort();
        let (kind, spec, regression, oracle, note, filing, expiry) = match verdict {
            Verdict::BoaBug { spec, regression } => (
                "boa-bug",
                spec.clone(),
                regression.clone(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ),
            Verdict::OracleQuirk { oracle, note } => (
                "oracle-quirk",
                String::new(),
                String::new(),
                oracle.clone(),
                note.clone(),
                String::new(),
                String::new(),
            ),
            Verdict::SpecAmbiguity {
                spec,
                filing,
                expiry,
            } => (
                "spec-ambiguity",
                spec.clone(),
                String::new(),
                String::new(),
                String::new(),
                filing.clone(),
                expiry.clone(),
            ),
        };
        Self {
            id: entry.id.clone(),
            area: entry.area.clone(),
            boa_outcome: entry.boa_outcome,
            oracle_outcome: entry.oracle_outcome,
            fields,
            kind: kind.to_owned(),
            spec,
            regression,
            oracle,
            note,
            filing,
            expiry,
        }
    }
}

/// Committed verdict store (`corpora/verdicts.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictFile {
    /// Schema version (must be 1).
    pub schema_version: u8,
    /// Writer identity (must be `boa_differential`).
    pub tool: String,
    /// Oracle kind the verdicts were triaged against.
    pub oracle_kind: String,
    /// Oracle version the verdicts were triaged against.
    pub oracle_version: String,
    /// Committed verdicts.
    #[serde(default)]
    pub verdicts: Vec<VerdictRecord>,
}

impl VerdictFile {
    /// Loads a verdict file, rejecting unknown schemas loudly.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("could not read verdicts: {err}"))?;
        let file: Self =
            toml::from_str(&text).map_err(|err| format!("invalid verdicts file: {err}"))?;
        if file.schema_version != 1 {
            return Err(format!(
                "unsupported verdicts schema {}",
                file.schema_version
            ));
        }
        if file.tool != "boa_differential" {
            return Err(format!(
                "verdicts tool is {:?}, expected \"boa_differential\"",
                file.tool
            ));
        }
        Ok(file)
    }

    /// Exports the queue's triaged entries.
    pub fn export(queue: &TriageQueue) -> Self {
        let mut verdicts: Vec<VerdictRecord> = queue
            .entries
            .values()
            .filter_map(|entry| {
                entry
                    .verdict
                    .as_ref()
                    .map(|verdict| VerdictRecord::from_entry(entry, verdict))
            })
            .collect();
        verdicts.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            schema_version: 1,
            tool: "boa_differential".to_owned(),
            oracle_kind: queue.oracle.kind.clone(),
            oracle_version: queue.oracle.version.clone(),
            verdicts,
        }
    }

    /// Saves the file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text =
            toml::to_string(self).map_err(|err| format!("could not encode verdicts: {err}"))?;
        std::fs::write(path, text).map_err(|err| format!("could not write verdicts: {err}"))?;
        Ok(())
    }

    /// Finds the verdict matching a fresh mismatch signature.
    ///
    /// Matches on id + both side outcomes + sorted field names (values are
    /// engine text and may legitimately shift; the shape must not).
    pub fn lookup(
        &self,
        id: &str,
        boa_outcome: TestOutcomeResult,
        oracle_outcome: TestOutcomeResult,
        fields: &[String],
    ) -> Option<&VerdictRecord> {
        let mut fields = fields.to_vec();
        fields.sort();
        self.verdicts.iter().find(|record| {
            record.id == id
                && record.boa_outcome == boa_outcome
                && record.oracle_outcome == oracle_outcome
                && record.fields == fields
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OracleRecord, TriageQueue, Verdict, VerdictFile, area_of};

    fn queue() -> TriageQueue {
        TriageQueue::empty(
            OracleRecord {
                kind: "jsshell".to_owned(),
                binary: "js".to_owned(),
                version: "v".to_owned(),
            },
            "ci",
        )
    }

    #[test]
    fn area_mapping_covers_origins() {
        assert_eq!(area_of("test/built-ins/Array/from"), "builtin:Array");
        assert_eq!(
            area_of("test/language/expressions/x#strict"),
            "language:expressions"
        );
        assert_eq!(area_of("test/intl402/NumberFormat/y"), "intl:NumberFormat");
        assert_eq!(area_of("regression:promise-try-no-wrap"), "regression");
        assert_eq!(area_of("adversarial:choke.js"), "adversarial");
        assert_eq!(area_of("bogus"), "other");
    }

    #[test]
    fn verdict_filters_require_a_match() {
        use super::QueueEntry;
        use boa_outcomes::TestOutcomeResult;
        let mut queue = queue();
        queue.entries.insert(
            "a".to_owned(),
            QueueEntry {
                id: "a".to_owned(),
                origin: "test262".to_owned(),
                area: "builtin:Array".to_owned(),
                boa_outcome: TestOutcomeResult::Passed,
                oracle_outcome: TestOutcomeResult::Passed,
                boa_text: String::new(),
                oracle_text: String::new(),
                field_diffs: Vec::new(),
                minimized_program: String::new(),
                minimized_by: String::new(),
                minimized_lines: 0,
                boa_observation: None,
                oracle_observation: None,
                verdict: None,
            },
        );
        let verdict = Verdict::BoaBug {
            spec: "https://tc39.es/x".to_owned(),
            regression: String::new(),
        };
        assert_eq!(
            queue.set_verdict(Some("builtin:Map"), None, None, &verdict),
            0
        );
        assert_eq!(
            queue.set_verdict(Some("builtin:Array"), None, None, &verdict),
            1
        );
        assert!(queue.untriaged().is_empty());
    }

    #[test]
    fn verdict_fields_filter_matches_exact_sets() {
        use super::QueueEntry;
        use crate::normalize::FieldDiff;
        use boa_outcomes::TestOutcomeResult;
        let entry = |id: &str, fields: &[&str]| QueueEntry {
            id: id.to_owned(),
            origin: "test262".to_owned(),
            area: "language:expressions".to_owned(),
            boa_outcome: TestOutcomeResult::Passed,
            oracle_outcome: TestOutcomeResult::Passed,
            boa_text: String::new(),
            oracle_text: String::new(),
            field_diffs: fields
                .iter()
                .map(|field| FieldDiff {
                    field: (*field).to_owned(),
                    boa: String::new(),
                    oracle: String::new(),
                })
                .collect(),
            minimized_program: String::new(),
            minimized_by: String::new(),
            minimized_lines: 0,
            boa_observation: None,
            oracle_observation: None,
            verdict: None,
        };
        let mut queue = queue();
        queue.entries.insert(
            "order-only".to_owned(),
            entry("order-only", &["state-order"]),
        );
        queue.entries.insert(
            "combo".to_owned(),
            entry("combo", &["state-order", "value"]),
        );
        let verdict = Verdict::OracleQuirk {
            oracle: "jsshell v".to_owned(),
            note: "n".to_owned(),
        };
        // Exact set: the combo entry must not match a subset filter.
        let order_only = ["state-order".to_owned()];
        assert_eq!(
            queue.set_verdict(None, None, Some(&order_only), &verdict),
            1
        );
        assert!(queue.entries["order-only"].verdict.is_some());
        assert!(queue.entries["combo"].verdict.is_none());
        // Order-independent full set matches the combo.
        let combo = ["value".to_owned(), "state-order".to_owned()];
        assert_eq!(queue.set_verdict(None, None, Some(&combo), &verdict), 1);
        assert!(queue.untriaged().is_empty());
    }

    #[test]
    fn verdict_export_lookup_round_trip() {
        use super::QueueEntry;
        use crate::normalize::FieldDiff;
        use boa_outcomes::TestOutcomeResult;
        let mut queue = queue();
        queue.entries.insert(
            "case-1".to_owned(),
            QueueEntry {
                id: "case-1".to_owned(),
                origin: "test262".to_owned(),
                area: "language:expressions".to_owned(),
                boa_outcome: TestOutcomeResult::Passed,
                oracle_outcome: TestOutcomeResult::Passed,
                boa_text: String::new(),
                oracle_text: String::new(),
                field_diffs: vec![FieldDiff {
                    field: "value".to_owned(),
                    boa: "a".to_owned(),
                    oracle: "b".to_owned(),
                }],
                minimized_program: String::new(),
                minimized_by: String::new(),
                minimized_lines: 0,
                boa_observation: None,
                oracle_observation: None,
                verdict: Some(Verdict::OracleQuirk {
                    oracle: "jsshell v".to_owned(),
                    note: "quirk".to_owned(),
                }),
            },
        );
        let file = VerdictFile::export(&queue);
        assert_eq!(file.verdicts.len(), 1);
        let record = file
            .lookup(
                "case-1",
                TestOutcomeResult::Passed,
                TestOutcomeResult::Passed,
                &["value".to_owned()],
            )
            .expect("lookup");
        assert!(matches!(
            record.to_verdict(),
            Ok(Verdict::OracleQuirk { .. })
        ));
        // Changed signature (new field) does not match.
        assert!(
            file.lookup(
                "case-1",
                TestOutcomeResult::Passed,
                TestOutcomeResult::Passed,
                &["value".to_owned(), "state".to_owned()],
            )
            .is_none()
        );
        // TOML round trip.
        let text = toml::to_string(&file).expect("encode");
        let back: VerdictFile = toml::from_str(&text).expect("decode");
        assert_eq!(back.verdicts.len(), 1);
    }

    #[test]
    fn oracle_quirks_reset_leaves_other_verdicts() {
        use super::QueueEntry;
        use boa_outcomes::TestOutcomeResult;
        let entry = |id: &str, verdict: Option<Verdict>| QueueEntry {
            id: id.to_owned(),
            origin: "test262".to_owned(),
            area: "a".to_owned(),
            boa_outcome: TestOutcomeResult::Passed,
            oracle_outcome: TestOutcomeResult::Passed,
            boa_text: String::new(),
            oracle_text: String::new(),
            field_diffs: Vec::new(),
            minimized_program: String::new(),
            minimized_by: String::new(),
            minimized_lines: 0,
            boa_observation: None,
            oracle_observation: None,
            verdict,
        };
        let mut queue = queue();
        queue.entries.insert(
            "q".to_owned(),
            entry(
                "q",
                Some(Verdict::OracleQuirk {
                    oracle: "o".to_owned(),
                    note: "n".to_owned(),
                }),
            ),
        );
        queue.entries.insert(
            "b".to_owned(),
            entry(
                "b",
                Some(Verdict::BoaBug {
                    spec: "s".to_owned(),
                    regression: String::new(),
                }),
            ),
        );
        assert_eq!(queue.reset_oracle_quirks(), 1);
        assert_eq!(queue.untriaged(), ["q"]);
    }
}
