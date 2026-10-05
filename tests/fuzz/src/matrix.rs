//! Opcode/operand coverage matrix (P5.4): cell parsing and static/dynamic merge.
//!
//! The engine emits two views of the same vocabulary: the static scan
//! ([`emitted_cells`](boa_engine::vm::coverage::emitted_cells), base opcodes
//! plus statically-known refinements) and the dynamic histogram (base plus
//! all refinements, with counts). This module parses cells into their
//! `(op, refinement)` fingerprint and merges per-program views into rows.
//!
//! Merge semantics: `static_files` counts programs whose static scan
//! emitted the cell (deduped per program — a file either emits a cell or
//! not); `dynamic` sums executions across programs. Either side may be
//! zero: `taken`/`err-*` cells are dynamic-only by construction, and a
//! statically-emitted but never-executed cell is dead-code signal, not an
//! error. Malformed cells are collected (never silently dropped) so the
//! caller can bail loud with every offender named.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Battery generator version, pinned into every report.
///
/// Bump when [`battery_program`](crate::battery::battery_program) changes
/// output for any seed: ratchets compare same-version reports only, and a
/// version mismatch fails the ratchet rather than comparing across
/// generator versions.
pub const BATTERY_VERSION: u32 = 1;

/// A parsed coverage cell: base opcode plus optional refinement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellParts {
    /// Base opcode name (`Call`).
    pub op: String,
    /// Refinement fragment (`argv-2`), if any.
    pub refinement: Option<String>,
}

/// Parse a coverage cell at its single `|`.
///
/// - `Call` → `(Call, None)`; `Call|argv-2` → `(Call, Some(argv-2))`.
/// - Empty input, an empty op, an empty refinement, or more than one `|`
///   is malformed → `None`. The engine never emits those shapes, so a
///   rejection names corrupt/hand-edited input, and the caller bails loud.
pub fn parse_cell(cell: &str) -> Option<CellParts> {
    let (op, refinement) = match cell.split_once('|') {
        None => (cell, None),
        Some((op, refinement)) => {
            if refinement.contains('|') {
                return None;
            }
            (op, Some(refinement))
        }
    };
    if op.is_empty() || refinement == Some("") {
        return None;
    }
    Some(CellParts {
        op: op.to_owned(),
        refinement: refinement.map(str::to_owned),
    })
}

/// Merged-only report: the `--merge` output shape, reused for the committed
/// ratchet and the deep-evidence file (both are floor/presence sources, so
/// one shape serves all three).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeReport {
    /// Battery generator version pinned by the report.
    pub battery_version: u32,
    /// Input paths merged (or `["ratchet"]` for the committed file).
    pub sources: Vec<String>,
    /// Merged rows, sorted by cell.
    pub merged: Vec<MatrixRow>,
    /// Malformed cells encountered (must be empty; non-empty fails).
    pub malformed: Vec<String>,
}

/// One merged matrix row: a cell with its static and dynamic evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixRow {
    /// Full cell (`Call|argv-2`).
    pub cell: String,
    /// Base opcode (`Call`).
    pub op: String,
    /// Refinement fragment, if any.
    pub refinement: Option<String>,
    /// Programs whose static scan emitted the cell.
    pub static_files: u64,
    /// Total dynamic executions across programs.
    pub dynamic: u64,
}

/// Merge per-program static cell sets with per-program dynamic histograms.
///
/// `statics[i]` holds program `i`'s static cells; `dynamics[i]` its
/// histogram. Returns rows sorted by cell plus every malformed cell
/// encountered (the caller bails loud when that list is non-empty).
pub fn merge_matrix(
    statics: &[Vec<String>],
    dynamics: &[HashMap<String, u64>],
) -> (Vec<MatrixRow>, Vec<String>) {
    let mut static_counts: HashMap<&str, u64> = HashMap::new();
    for cells in statics {
        let mut seen = HashSet::new();
        for cell in cells {
            if seen.insert(cell.as_str()) {
                *static_counts.entry(cell.as_str()).or_default() += 1;
            }
        }
    }
    let mut dynamic_counts: HashMap<&str, u64> = HashMap::new();
    for histogram in dynamics {
        for (cell, count) in histogram {
            *dynamic_counts.entry(cell.as_str()).or_default() += count;
        }
    }

    let mut cells: Vec<&str> = static_counts.keys().copied().collect();
    for cell in dynamic_counts.keys() {
        if !static_counts.contains_key(cell) {
            cells.push(cell);
        }
    }
    cells.sort_unstable();

    let mut rows = Vec::with_capacity(cells.len());
    let mut malformed = Vec::new();
    for cell in cells {
        let Some(parts) = parse_cell(cell) else {
            malformed.push(cell.to_owned());
            continue;
        };
        rows.push(MatrixRow {
            cell: cell.to_owned(),
            op: parts.op,
            refinement: parts.refinement,
            static_files: static_counts.get(cell).copied().unwrap_or(0),
            dynamic: dynamic_counts.get(cell).copied().unwrap_or(0),
        });
    }
    (rows, malformed)
}

/// Merge already-merged rows from disjoint feeds (battery, seeds, Test262).
///
/// Each input is `(cell, static_files, dynamic)`; feeds are disjoint file
/// sets, so both columns sum. Returns rows sorted by cell plus every
/// malformed cell encountered (the caller bails loud when non-empty).
pub fn merge_rows(inputs: &[(String, u64, u64)]) -> (Vec<MatrixRow>, Vec<String>) {
    let mut static_counts: HashMap<&str, u64> = HashMap::new();
    let mut dynamic_counts: HashMap<&str, u64> = HashMap::new();
    for (cell, static_files, dynamic) in inputs {
        *static_counts.entry(cell.as_str()).or_default() += static_files;
        *dynamic_counts.entry(cell.as_str()).or_default() += dynamic;
    }
    let mut cells: Vec<&str> = static_counts.keys().copied().collect();
    cells.sort_unstable();
    let mut rows = Vec::with_capacity(cells.len());
    let mut malformed = Vec::new();
    for cell in cells {
        let Some(parts) = parse_cell(cell) else {
            malformed.push(cell.to_owned());
            continue;
        };
        rows.push(MatrixRow {
            cell: cell.to_owned(),
            op: parts.op,
            refinement: parts.refinement,
            static_files: static_counts[cell],
            dynamic: dynamic_counts[cell],
        });
    }
    (rows, malformed)
}

/// Committed ratchet justifications (`justifications.toml`).
///
/// - `[zero]`: every ratchet cell with `static_files == 0` and `dynamic ==
///   0` needs an entry, keyed by exact cell — except refinements of a
///   zero base, which inherit the base's entry (a zero `Call|argv-7` with
///   zero `Call` is covered by the `Call` entry).
/// - `[deep]`: cells with evidence ONLY in the deep file
///   (`test262-dynamic.json`, periodic regen, never gated). Each must be
///   present there and absent from deterministic fresh runs (present in
///   fresh means promote to a ratchet row).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Justifications {
    /// Exact-cell reasons for zero-floor ratchet cells (plus zero bases).
    pub zero: HashMap<String, String>,
    /// Deep-only cells (presence-pinned in the deep file).
    pub deep: HashMap<String, String>,
}

/// Parse justifications TOML. Unknown top-level tables and non-string
/// values are errors (a typo'd table would silently void justifications).
pub fn parse_justifications(text: &str) -> Result<Justifications, String> {
    let table: toml::Table = toml::from_str(text).map_err(|err| format!("invalid TOML: {err}"))?;
    let mut just = Justifications::default();
    for (name, value) in &table {
        let target = match name.as_str() {
            "zero" => &mut just.zero,
            "deep" => &mut just.deep,
            unknown => return Err(format!("unknown justifications table [{unknown}]")),
        };
        let entries = value
            .as_table()
            .ok_or_else(|| format!("[{name}] must be a string table"))?;
        for (cell, reason) in entries {
            let reason = reason
                .as_str()
                .ok_or_else(|| format!("[{name}] {cell:?} reason must be a string"))?;
            target.insert(cell.clone(), reason.to_owned());
        }
    }
    Ok(just)
}

/// Check fresh deterministic rows against the committed ratchet.
///
/// Returns one human-readable line per violation (empty = pass). Rules, in
/// report order: battery version match; no defensive cells (`|k-mistype`,
/// `|k-oob` — corruption alarms, unjustifiable); no new cells; no stale
/// cells; every floor met (`static_files`/`dynamic` at least the
/// committed minimum); every zero cell justified; no stale
/// justifications; every `[deep]` cell present in `deep` and absent from
/// fresh (and outside the ratchet entirely).
pub fn check_ratchet(
    ratchet: &MergeReport,
    just: &Justifications,
    deep: &MergeReport,
    fresh: &[MatrixRow],
    fresh_version: u32,
) -> Vec<String> {
    let mut failures = Vec::new();
    if fresh_version != ratchet.battery_version {
        failures.push(format!(
            "battery version mismatch: fresh={fresh_version} ratchet={}",
            ratchet.battery_version
        ));
    }
    let ratchet_map: HashMap<&str, &MatrixRow> = ratchet
        .merged
        .iter()
        .map(|row| (row.cell.as_str(), row))
        .collect();
    let fresh_map: HashMap<&str, &MatrixRow> =
        fresh.iter().map(|row| (row.cell.as_str(), row)).collect();
    let deep_cells: HashSet<&str> = deep.merged.iter().map(|row| row.cell.as_str()).collect();

    for row in fresh {
        if row.cell.ends_with("|k-mistype") || row.cell.ends_with("|k-oob") {
            failures.push(format!(
                "defensive cell observed (corruption alarm): {}",
                row.cell
            ));
        }
        if !ratchet_map.contains_key(row.cell.as_str()) {
            failures.push(format!(
                "new cell (add to ratchet with floors): {}",
                row.cell
            ));
        }
    }
    for row in &ratchet.merged {
        let Some(seen) = fresh_map.get(row.cell.as_str()) else {
            failures.push(format!("stale cell (regen ratchet): {}", row.cell));
            continue;
        };
        if seen.static_files < row.static_files {
            failures.push(format!(
                "floor violation: {} static {} < {}",
                row.cell, seen.static_files, row.static_files
            ));
        }
        if seen.dynamic < row.dynamic {
            failures.push(format!(
                "floor violation: {} dynamic {} < {}",
                row.cell, seen.dynamic, row.dynamic
            ));
        }
    }
    // Zero cells: exact entry, or base entry when the base itself is zero.
    for row in &ratchet.merged {
        if row.static_files != 0 || row.dynamic != 0 {
            continue;
        }
        if just.zero.contains_key(&row.cell) {
            continue;
        }
        let inherited = parse_cell(&row.cell).is_some_and(|parts| {
            parts.refinement.is_some()
                && just.zero.contains_key(&parts.op)
                && ratchet_map
                    .get(parts.op.as_str())
                    .is_some_and(|base| base.static_files == 0 && base.dynamic == 0)
        });
        if !inherited {
            failures.push(format!("unjustified zero cell: {}", row.cell));
        }
    }
    let mut zero_keys: Vec<&String> = just.zero.keys().collect();
    zero_keys.sort();
    for key in zero_keys {
        let covered = ratchet_map
            .get(key.as_str())
            .is_some_and(|row| row.static_files == 0 && row.dynamic == 0);
        if !covered {
            failures.push(format!("stale justification: {key}"));
        }
    }
    let mut deep_keys: Vec<&String> = just.deep.keys().collect();
    deep_keys.sort();
    for key in deep_keys {
        if ratchet_map.contains_key(key.as_str()) {
            failures.push(format!("deep cell in ratchet (move to [zero]): {key}"));
        }
        if !deep_cells.contains(key.as_str()) {
            failures.push(format!("deep evidence missing: {key}"));
        }
        if fresh_map.contains_key(key.as_str()) {
            failures.push(format!(
                "promote to ratchet (now in deterministic feeds): {key}"
            ));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cell_accepts_base_and_refined() {
        assert_eq!(
            parse_cell("Call"),
            Some(CellParts {
                op: String::from("Call"),
                refinement: None,
            })
        );
        assert_eq!(
            parse_cell("Call|argv-2"),
            Some(CellParts {
                op: String::from("Call"),
                refinement: Some(String::from("argv-2")),
            })
        );
    }

    #[test]
    fn parse_cell_rejects_malformed() {
        for bad in ["", "|taken", "Call|", "a|b|c", "|"] {
            assert_eq!(parse_cell(bad), None, "cell {bad:?} must be rejected");
        }
    }

    #[test]
    fn merge_unions_static_and_dynamic_names() {
        let statics = vec![
            vec![String::from("Call"), String::from("Jump")],
            vec![String::from("Call"), String::from("Call")],
        ];
        let dynamics = vec![
            HashMap::from([(String::from("Call"), 3), (String::from("Call|argv-2"), 3)]),
            HashMap::new(),
        ];
        let (rows, malformed) = merge_matrix(&statics, &dynamics);
        assert!(malformed.is_empty());
        // Sorted by cell; per-program static dedup (Call in 2 files, not 3).
        assert_eq!(
            rows.iter()
                .map(|row| (row.cell.as_str(), row.static_files, row.dynamic))
                .collect::<Vec<_>>(),
            vec![("Call", 2, 3), ("Call|argv-2", 0, 3), ("Jump", 1, 0),]
        );
        assert_eq!(rows[1].op, "Call");
        assert_eq!(rows[1].refinement, Some(String::from("argv-2")));
    }

    #[test]
    fn merge_collects_malformed_without_dropping_rows() {
        let statics = vec![vec![String::from("Call")]];
        let dynamics = vec![HashMap::from([(String::from("bad||cell"), 1)])];
        let (rows, malformed) = merge_matrix(&statics, &dynamics);
        assert_eq!(malformed, vec![String::from("bad||cell")]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cell, "Call");
    }

    #[test]
    fn merge_empty_inputs_yield_no_rows() {
        let (rows, malformed) = merge_matrix(&[], &[]);
        assert!(rows.is_empty());
        assert!(malformed.is_empty());
    }

    #[test]
    fn merge_rows_sums_disjoint_feeds() {
        let inputs = vec![
            (String::from("Call"), 2, 3),
            (String::from("Call"), 5, 7),
            (String::from("Jump|taken"), 0, 4),
        ];
        let (rows, malformed) = merge_rows(&inputs);
        assert!(malformed.is_empty());
        assert_eq!(
            rows.iter()
                .map(|row| (row.cell.as_str(), row.static_files, row.dynamic))
                .collect::<Vec<_>>(),
            vec![("Call", 7, 10), ("Jump|taken", 0, 4)]
        );
    }

    #[test]
    fn merge_rows_collects_malformed() {
        let inputs = vec![(String::from("bad||cell"), 1, 1)];
        let (rows, malformed) = merge_rows(&inputs);
        assert_eq!(malformed, vec![String::from("bad||cell")]);
        assert!(rows.is_empty());
    }

    fn row(cell: &str, static_files: u64, dynamic: u64) -> MatrixRow {
        let parts = parse_cell(cell).expect("test cell must parse");
        MatrixRow {
            cell: String::from(cell),
            op: parts.op,
            refinement: parts.refinement,
            static_files,
            dynamic,
        }
    }

    fn report(version: u32, rows: Vec<MatrixRow>) -> MergeReport {
        MergeReport {
            battery_version: version,
            sources: vec![String::from("test")],
            merged: rows,
            malformed: Vec::new(),
        }
    }

    #[test]
    fn parse_justifications_accepts_both_tables() {
        let just = parse_justifications(
            "[zero]\nCall = \"never emitted\"\n[deep]\nImportMeta = \"module-only\"\n",
        )
        .expect("valid justifications must parse");
        assert_eq!(
            just.zero.get("Call").map(String::as_str),
            Some("never emitted")
        );
        assert_eq!(
            just.deep.get("ImportMeta").map(String::as_str),
            Some("module-only")
        );
    }

    #[test]
    fn parse_justifications_rejects_unknown_tables_and_shapes() {
        assert!(parse_justifications("[zeros]\nCall = \"x\"\n").is_err());
        assert!(parse_justifications("[zero]\nCall = 7\n").is_err());
        assert!(parse_justifications("not toml [[[").is_err());
        // Missing tables are empty (no zeros, no deep-only cells).
        assert_eq!(
            parse_justifications("").expect("empty must parse"),
            Justifications::default()
        );
    }

    #[test]
    fn check_passes_when_floors_met_and_zeros_justified() {
        let ratchet = report(1, vec![row("Call", 2, 3), row("Jump", 0, 0)]);
        let just = parse_justifications("[zero]\nJump = \"unreachable\"\n").expect("parse");
        let deep = report(1, Vec::new());
        let fresh = vec![row("Call", 2, 5), row("Jump", 0, 0)];
        assert!(check_ratchet(&ratchet, &just, &deep, &fresh, 1).is_empty());
    }

    #[test]
    fn check_reports_version_floors_new_and_stale() {
        let ratchet = report(1, vec![row("Call", 2, 3), row("Jump", 1, 1)]);
        let just = Justifications::default();
        let deep = report(1, Vec::new());
        // Call regresses below floors; Jump vanishes; Nop appears.
        let fresh = vec![row("Call", 1, 3), row("Nop", 1, 1)];
        let failures = check_ratchet(&ratchet, &just, &deep, &fresh, 2);
        assert!(
            failures.iter().any(|f| f.contains("version mismatch")),
            "{failures:?}"
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("new cell") && f.contains("Nop")),
            "{failures:?}"
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("stale cell") && f.contains("Jump")),
            "{failures:?}"
        );
        assert!(
            failures
                .iter()
                .any(|f| f.contains("floor violation") && f.contains("Call")),
            "{failures:?}"
        );
    }

    #[test]
    fn check_zero_refinement_inherits_zero_base_entry() {
        let ratchet = report(
            1,
            vec![
                row("Call", 0, 0),
                row("Call|argv-7", 0, 0),
                row("Jump", 5, 5),
                row("Jump|taken", 0, 0),
            ],
        );
        // Base entry covers the zero refinement of the zero base; the
        // refinement of the live base needs its own entry.
        let just =
            parse_justifications("[zero]\nCall = \"dead\"\n\"Jump|taken\" = \"never taken\"\n")
                .expect("parse");
        let deep = report(1, Vec::new());
        let fresh = ratchet.merged.clone();
        assert!(check_ratchet(&ratchet, &just, &deep, &fresh, 1).is_empty());

        let bare = parse_justifications("[zero]\nCall = \"dead\"\n").expect("parse");
        let failures = check_ratchet(&ratchet, &bare, &deep, &fresh, 1);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("unjustified zero cell"),
            "{failures:?}"
        );

        let stale = parse_justifications(
            "[zero]\nCall = \"dead\"\nJump = \"wrong\"\n\"Jump|taken\" = \"x\"\n",
        )
        .expect("parse");
        let failures = check_ratchet(&ratchet, &stale, &deep, &fresh, 1);
        assert_eq!(failures, vec![String::from("stale justification: Jump")]);
    }

    #[test]
    fn check_enforces_deep_presence_absence_and_promotion() {
        let ratchet = report(1, vec![row("Call", 1, 1)]);
        let deep = report(1, vec![row("ImportMeta", 0, 9)]);
        let fresh = vec![row("Call", 1, 1)];
        let just = parse_justifications("[deep]\nImportMeta = \"module-only\"\n").expect("parse");
        assert!(check_ratchet(&ratchet, &just, &deep, &fresh, 1).is_empty());

        // Evidence vanished from the deep file.
        let empty_deep = report(1, Vec::new());
        let failures = check_ratchet(&ratchet, &just, &empty_deep, &fresh, 1);
        assert!(
            failures.iter().any(|f| f.contains("deep evidence missing")),
            "{failures:?}"
        );

        // Deep cell appears in deterministic fresh: promote (plus new-cell,
        // since it is outside the ratchet).
        let promoted = vec![row("Call", 1, 1), row("ImportMeta", 1, 1)];
        let failures = check_ratchet(&ratchet, &just, &deep, &promoted, 1);
        assert!(
            failures.iter().any(|f| f.contains("promote to ratchet")),
            "{failures:?}"
        );
        assert!(
            failures.iter().any(|f| f.contains("new cell")),
            "{failures:?}"
        );
    }

    #[test]
    fn check_fails_defensive_cells() {
        let ratchet = report(1, vec![row("Call", 1, 1)]);
        let just = Justifications::default();
        let deep = report(1, Vec::new());
        let fresh = vec![row("Call", 1, 1), row("StoreLiteral|k-oob", 0, 1)];
        let failures = check_ratchet(&ratchet, &just, &deep, &fresh, 1);
        assert!(
            failures.iter().any(|f| f.contains("defensive cell")),
            "{failures:?}"
        );
    }
}
