//! Audit lint for the Test262 ignore list and flakiness quarantine.
//!
//! Every ignore entry must carry a machine-readable justification (reason,
//! link, owner, dates, work item). [`validate_config`] collects violations
//! for the `check-config` subcommand and CI; [`print_report`] renders the
//! age/area report.

use time::macros::format_description;

use crate::{IgnoreEntry, Ignored, QuarantineConfig};

/// Parses a `YYYY-MM-DD` calendar date.
fn parse_date(text: &str) -> Option<time::Date> {
    const FORMAT: &[time::format_description::FormatItem<'static>] =
        format_description!("[year]-[month]-[day]");
    time::Date::parse(text, &FORMAT).ok()
}

/// Today's UTC date.
fn today() -> time::Date {
    time::OffsetDateTime::now_utc().date()
}

/// Validates all ignore entries, returning one message per violation.
pub(crate) fn validate_config(ignored: &Ignored) -> Vec<String> {
    let mut violations = Vec::new();
    validate_entries("ignored.tests", &ignored.tests, false, &mut violations);
    validate_entries(
        "ignored.features",
        &ignored.features,
        false,
        &mut violations,
    );
    violations
}

/// Validates quarantine entries. Unlike ignores, quarantine expiries must be
/// concrete dates: known-flaky tests are time-boxed, never open-ended.
pub(crate) fn validate_quarantine(quarantine: &QuarantineConfig) -> Vec<String> {
    let mut violations = Vec::new();
    validate_entries("quarantine", &quarantine.quarantine, true, &mut violations);
    violations
}

/// Validates one entry list, pushing violations into `out`.
fn validate_entries(
    section: &str,
    entries: &[IgnoreEntry],
    require_date_expiry: bool,
    out: &mut Vec<String>,
) {
    let now = today();
    let mut seen = std::collections::HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let here = format!("{section}[{index}] ('{}')", entry.pattern);
        for (field, value) in [
            ("pattern", &entry.pattern),
            ("reason", &entry.reason),
            ("link", &entry.link),
            ("owner", &entry.owner),
            ("added", &entry.added),
            ("expiry", &entry.expiry),
            ("work_item", &entry.work_item),
        ] {
            if value.trim().is_empty() {
                out.push(format!("{here}: field `{field}` is empty"));
            }
        }
        if !seen.insert(entry.pattern.clone()) {
            out.push(format!("{here}: duplicate pattern"));
        }
        match parse_date(&entry.added) {
            Some(date) if date > now => {
                out.push(format!("{here}: `added` {} is in the future", entry.added));
            }
            Some(_) => {}
            None => out.push(format!(
                "{here}: `added` '{}' is not a YYYY-MM-DD date",
                entry.added
            )),
        }
        validate_expiry(&here, entry, require_date_expiry, now, out);
    }
}

/// Validates an entry's `expiry` field.
fn validate_expiry(
    here: &str,
    entry: &IgnoreEntry,
    require_date_expiry: bool,
    now: time::Date,
    out: &mut Vec<String>,
) {
    if let Some(reference) = entry.expiry.strip_prefix("re-review:") {
        if require_date_expiry {
            out.push(format!(
                "{here}: quarantine expiry must be a date, not '{}'",
                entry.expiry
            ));
        } else if reference.trim().is_empty() {
            out.push(format!("{here}: `re-review:` needs a commit or issue ref"));
        }
        return;
    }
    match parse_date(&entry.expiry) {
        Some(date) if date < now => {
            out.push(format!("{here}: expired on {}", entry.expiry));
        }
        Some(_) => {}
        None => out.push(format!(
            "{here}: `expiry` '{}' is neither a YYYY-MM-DD date nor `re-review:<ref>`",
            entry.expiry
        )),
    }
}

/// Prints the age/area report for ignore and quarantine entries.
pub(crate) fn print_report(ignored: &Ignored, quarantine: &QuarantineConfig) {
    let now = today();
    println!("Ignore-list age/area report (oldest first):\n");
    println!(
        "| {:<28} | {:<52} | {:>5} | {:<16} | {:<14} |",
        "area", "pattern", "age_d", "expiry", "owner"
    );
    println!(
        "|{:-<30}|{:-<54}|{:-<7}|{:-<18}|{:-<16}|",
        "", "", "", "", ""
    );
    let mut rows: Vec<(String, &IgnoreEntry)> = ignored
        .tests
        .iter()
        .map(|entry| (area_of(entry), entry))
        .chain(
            ignored
                .features
                .iter()
                .map(|entry| (String::from("feature"), entry)),
        )
        .collect();
    rows.sort_by(|(_, a), (_, b)| a.added.cmp(&b.added));
    for (area, entry) in &rows {
        let age = parse_date(&entry.added).map_or(String::from("?"), |date| {
            (now - date).whole_days().to_string()
        });
        println!(
            "| {:<28} | {:<52} | {:>5} | {:<16} | {:<14} |",
            truncate(area, 28),
            truncate(&entry.pattern, 52),
            age,
            truncate(&entry.expiry, 16),
            truncate(&entry.owner, 14),
        );
    }
    println!(
        "\n{} test entries, {} feature entries, {} quarantined tests.",
        ignored.tests.len(),
        ignored.features.len(),
        quarantine.quarantine.len()
    );
    for entry in &quarantine.quarantine {
        let left = parse_date(&entry.expiry).map_or(String::from("?"), |date| {
            (date - now).whole_days().to_string()
        });
        println!(
            "quarantined: {} (expires in {left}d, owner {})",
            entry.pattern, entry.owner
        );
    }
}

/// Area bucket for a test-pattern entry: the first three path segments.
fn area_of(entry: &IgnoreEntry) -> String {
    let segments: Vec<&str> = entry.pattern.split('/').collect();
    segments
        .iter()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .join("/")
}

/// Truncates to `max` chars.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        text.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_date, validate_entries};

    use crate::IgnoreEntry;

    fn entry(pattern: &str) -> IgnoreEntry {
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
    fn accepts_a_fully_justified_entry() {
        let mut violations = Vec::new();
        validate_entries("tests", &[entry("test/a.js")], false, &mut violations);
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn rejects_empty_fields_duplicates_and_bad_dates() {
        let mut bad = entry("test/a.js");
        bad.reason = "  ".into();
        bad.added = "yesterday".into();
        let mut out = Vec::new();
        validate_entries(
            "tests",
            &[bad, entry("test/a.js"), entry("test/a.js")],
            false,
            &mut out,
        );
        assert!(
            out.iter().any(|v| v.contains("`reason` is empty")),
            "{out:?}"
        );
        assert!(
            out.iter().any(|v| v.contains("not a YYYY-MM-DD date")),
            "{out:?}"
        );
        assert!(
            out.iter().any(|v| v.contains("duplicate pattern")),
            "{out:?}"
        );
    }

    #[test]
    fn rejects_expired_entries() {
        let mut expired = entry("test/old.js");
        expired.expiry = "2020-01-01".into();
        let mut out = Vec::new();
        validate_entries("tests", &[expired], false, &mut out);
        assert!(out.iter().any(|v| v.contains("expired on")), "{out:?}");
    }

    #[test]
    fn accepts_re_review_expiry_except_in_quarantine() {
        let mut entry = entry("test/q.js");
        entry.expiry = "re-review:abc123".into();
        let mut out = Vec::new();
        validate_entries("tests", &[entry.clone()], false, &mut out);
        assert!(out.is_empty(), "{out:?}");
        validate_entries("quarantine", &[entry], true, &mut out);
        assert!(out.iter().any(|v| v.contains("must be a date")), "{out:?}");
    }

    #[test]
    fn parses_calendar_dates() {
        assert!(parse_date("2026-02-29").is_none());
        assert!(parse_date("2024-02-29").is_some());
        assert!(parse_date("2026-13-01").is_none());
        assert!(parse_date("not-a-date").is_none());
    }
}
