//! P5.4 opcode/operand coverage matrix driver.
//!
//! Runs the shared battery (plus optional seed files) through the
//! `vm-coverage` instrumented engine and writes one JSON report with
//! per-program static/dynamic views and the merged matrix.
//!
//! Usage (from `tests/fuzz`):
//! `cargo run --example matrix -- <out.json> [--programs N] [--start S]
//! [--seeds] [--files-from LIST] [--static-only] [extra.js ...]`
//!
//! - `--programs N` (default 2000): battery programs `start..start+N`.
//! - `--seeds`: also feed `seeds/*.js` and
//!   `../regression/cases/*/repro.js` (sorted; the canonical bug-corpus).
//! - `--probes`: also feed `probes/*.js` (sorted; committed gap probes —
//!   each names the cells it exists to cover).
//! - `--files-from LIST`: also feed the newline-separated paths in LIST
//!   (for feeds too large for argv, e.g. Test262).
//! - `--static-only`: skip the dynamic leg (static cells only, empty
//!   dynamic views, `outcome: "static-only"`).
//! - `--optimize`: compile and run with `OPTIMIZE_ALL` (default is
//!   unoptimized). Needed for optimizer-emitted shapes (`StoreNan` from
//!   folded `0/0`); unoptimized feeds can never produce them.
//! - `extra.js`: additional files (read errors skip + record).
//!
//! Merge mode (`--merge OUT.json IN1.json ...`, no engine run) combines
//! matrix reports (`merged` arrays) and tester coverage files (`static` /
//! `dynamic` maps) from disjoint feeds into one merged matrix. Both
//! columns sum; `battery_version` is the max present. Non-empty
//! `malformed` writes the output for forensics but exits nonzero.
//!
//! Static and dynamic both run unoptimized (dead branches eliminated by
//! the optimizer would otherwise hide executed cells, and the two views
//! must describe the same bytecode). Battery programs must parse — a
//! parse failure there panics loud (the generator is valid by
//! construction); file inputs that fail to parse/compile skip with a
//! recorded reason. An invalid compiled block panics via `verify_hook`
//! (a compiler bug, stop-the-line), as does an engine panic mid-run
//! (rerun narrowed with `--start`/`--programs`; progress prints to
//! stderr). A non-empty `malformed` list writes the report for forensics
//! but exits nonzero: the engine never emits those shapes.

use boa_engine::optimizer::OptimizerOptions;
use boa_engine::vm::coverage::{coverage_clear, coverage_dump, emitted_cells};
use boa_engine::{Context, Script, Source};
use boa_fuzz::battery::battery_program;
use boa_fuzz::matrix::{
    check_ratchet, merge_matrix, merge_rows, parse_justifications, MatrixRow, MergeReport,
    BATTERY_VERSION,
};
use boa_fuzz::semantic::{eval_outcome_drained_with_optimizer, verify_hook};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// One measured program: static cell set plus dynamic histogram.
#[derive(Serialize)]
struct ProgramRow {
    id: String,
    outcome: String,
    static_cells: Vec<String>,
    dynamic: BTreeMap<String, u64>,
}

/// An input that never ran, with its reason.
#[derive(Serialize)]
struct Skipped {
    id: String,
    reason: String,
}

/// Top-level report. `merged` comes from [`merge_matrix`]; `malformed`
/// and `battery_panics` must be empty (non-empty exits nonzero after
/// writing). File panics land in their program row's `outcome` and exit 0:
/// the hostile corpus is expected to contain crashers.
#[derive(Serialize)]
struct Report {
    battery_version: u32,
    programs: Vec<ProgramRow>,
    skipped: Vec<Skipped>,
    merged: Vec<MatrixRow>,
    malformed: Vec<String>,
    battery_panics: Vec<String>,
}

/// Compile `source` unoptimized and return its deduplicated static cells.
///
/// `must_parse` (battery programs) panics loud on parse/compile failure;
/// file inputs return the reason for skip + record.
fn static_cells(
    source: &str,
    must_parse: bool,
    id: &str,
    optimize: bool,
) -> Result<Vec<String>, String> {
    let mut context = Context::default();
    if !optimize {
        context.set_optimizer_options(OptimizerOptions::empty());
    }
    let script = match Script::parse(Source::from_bytes(source), None, &mut context) {
        Ok(script) => script,
        Err(err) => {
            let reason = format!("parse: {err}");
            if must_parse {
                panic!("battery program {id} failed to parse: {reason}");
            }
            return Err(reason);
        }
    };
    let block = match script.codeblock(&mut context) {
        Ok(block) => block,
        Err(err) => {
            let reason = format!("compile: {err}");
            if must_parse {
                panic!("battery program {id} failed to compile: {reason}");
            }
            return Err(reason);
        }
    };
    // Invalid bytecode is a compiler bug: crash loud, never measure it.
    verify_hook(&block);
    let mut cells = emitted_cells(&block);
    cells.sort();
    cells.dedup();
    Ok(cells)
}

/// Execute `source` unoptimized and return its histogram plus outcome.
///
/// The outcome class (`completed` or the error class) explains `err-*`
/// cells in the histogram. Partial counts from fuel-cut programs are real
/// executions and kept; battery programs terminate within fuel (tested).
fn dynamic_histogram(source: &str, optimize: bool) -> (BTreeMap<String, u64>, String) {
    coverage_clear();
    // Drained (tester parity): async continuations execute, so promise and
    // async-generator paths are visible instead of cut at the first await.
    let options = if optimize {
        OptimizerOptions::OPTIMIZE_ALL
    } else {
        OptimizerOptions::empty()
    };
    let outcome = eval_outcome_drained_with_optimizer(source, options);
    let histogram = coverage_dump().into_iter().collect();
    let status = outcome
        .error_class
        .unwrap_or_else(|| String::from("completed"));
    (histogram, status)
}

/// Run [`dynamic_histogram`] on an isolated worker thread.
///
/// The regression/file corpus contains crashers by design (that is its
/// job), so a dynamic-leg panic on a file input is recorded, not fatal.
/// The worker stack is 32 MB to match the main thread (`ulimit -s 32768`
/// on the dev machine); Boa's recursion limit is count-based, so the size
/// is moot for engine limits and only avoids spurious native overflows on
/// hostile inputs. The histogram is process-global and the driver joins
/// each worker before the next, so attribution stays exact. Returns
/// `None` on a worker panic with the payload rendered for forensics.
fn dynamic_isolated(source: &str, optimize: bool) -> Option<(BTreeMap<String, u64>, String)> {
    let owned = source.to_owned();
    let joined = std::thread::Builder::new()
        .name(String::from("matrix-dynamic"))
        .stack_size(32 << 20)
        .spawn(move || dynamic_histogram(&owned, optimize))
        .expect("spawn matrix worker");
    match joined.join() {
        Ok(pair) => Some(pair),
        Err(payload) => {
            let mut message = panic_message(payload);
            message.truncate(200);
            eprintln!("matrix: dynamic leg panicked: {message}");
            None
        }
    }
}

/// Best-effort render of a caught panic payload.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast::<String>()
        .map(|boxed| *boxed)
        .or_else(|payload| payload.downcast::<&str>().map(|boxed| (*boxed).to_owned()))
        .unwrap_or_else(|_| String::from("<non-string panic>"))
}

/// Collect `dir`'s `.js` files (sorted) or `repro.js` files one level down.
fn js_files(dir: &str, one_level_down: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return paths;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if one_level_down.is_empty() {
            if path.extension().is_some_and(|ext| ext == "js") {
                paths.push(path.display().to_string());
            }
        } else if path.is_dir() {
            let repro = path.join(one_level_down);
            if repro.is_file() {
                paths.push(repro.display().to_string());
            }
        }
    }
    paths.sort();
    paths
}

fn usage(program: &str) -> ! {
    eprintln!(
        "usage: {program} <out.json> [--programs N] [--start S] [--seeds] [--probes] [--files-from LIST] [--static-only] [--optimize] [extra.js ...]"
    );
    std::process::exit(2);
}

/// Extract `(cell, static_files, dynamic)` rows from a matrix report
/// (`merged` array) or a tester coverage file (`static`/`dynamic` maps).
fn report_inputs(path: &str) -> Vec<(String, u64, u64)> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("matrix: cannot read {path}: {err}"));
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("matrix: invalid JSON in {path}: {err}"));
    if let Some(merged) = value.get("merged").and_then(serde_json::Value::as_array) {
        return merged
            .iter()
            .map(|row| {
                let row: MatrixRow = serde_json::from_value(row.clone())
                    .unwrap_or_else(|err| panic!("matrix: invalid row in {path}: {err}"));
                (row.cell, row.static_files, row.dynamic)
            })
            .collect();
    }
    let mut inputs = Vec::new();
    for (key, column) in [("static", 0), ("dynamic", 1)] {
        let map = value
            .get(key)
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("matrix: {path} has neither `merged` nor `{key}`"));
        for (cell, count) in map {
            let count = count
                .as_u64()
                .unwrap_or_else(|| panic!("matrix: bad count in {path}"));
            if column == 0 {
                inputs.push((cell.clone(), count, 0));
            } else {
                inputs.push((cell.clone(), 0, count));
            }
        }
    }
    inputs
}

/// Report's `battery_version`, if present (tester files have none).
fn report_version(path: &str) -> u32 {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("matrix: cannot read {path}: {err}"));
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("matrix: invalid JSON in {path}: {err}"));
    value
        .get("battery_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32
}

fn run_merge(out: &str, inputs: &[String]) {
    let mut rows: Vec<(String, u64, u64)> = Vec::new();
    let mut version = 0;
    for path in inputs {
        rows.extend(report_inputs(path));
        version = version.max(report_version(path));
    }
    let (merged, malformed) = merge_rows(&rows);
    let report = MergeReport {
        battery_version: version,
        sources: inputs.to_vec(),
        merged,
        malformed: malformed.clone(),
    };
    let json = serde_json::to_string_pretty(&report).expect("serialize merge report");
    std::fs::write(out, &json).expect("write merge report");
    eprintln!(
        "matrix: merged {} inputs -> {} rows -> {out}",
        inputs.len(),
        report.merged.len()
    );
    if !malformed.is_empty() {
        eprintln!("matrix: MALFORMED CELLS: {malformed:?}");
        std::process::exit(1);
    }
}

fn run_check(ratchet_path: &str, just_path: &str, deep_path: &str, fresh_paths: &[String]) {
    let read_json = |path: &str| {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("matrix: cannot read {path}: {err}"));
        serde_json::from_str::<MergeReport>(&text)
            .unwrap_or_else(|err| panic!("matrix: invalid report in {path}: {err}"))
    };
    let ratchet = read_json(ratchet_path);
    let deep = read_json(deep_path);
    let just_text = std::fs::read_to_string(just_path)
        .unwrap_or_else(|err| panic!("matrix: cannot read {just_path}: {err}"));
    let just = parse_justifications(&just_text)
        .unwrap_or_else(|err| panic!("matrix: invalid justifications in {just_path}: {err}"));
    if !ratchet.malformed.is_empty() || !deep.malformed.is_empty() {
        println!("matrix: RATCHET FAIL (1 violation):");
        println!("  committed report has malformed cells (regen ratchet)");
        std::process::exit(1);
    }
    let mut rows: Vec<(String, u64, u64)> = Vec::new();
    let mut version = 0;
    for path in fresh_paths {
        rows.extend(report_inputs(path));
        version = version.max(report_version(path));
    }
    let (fresh, malformed) = merge_rows(&rows);
    let mut failures = check_ratchet(&ratchet, &just, &deep, &fresh, version);
    for cell in malformed {
        failures.push(format!("malformed fresh cell: {cell}"));
    }
    failures.sort();
    if failures.is_empty() {
        println!(
            "matrix: RATCHET PASS ({} cells, {} justifications)",
            fresh.len(),
            just.zero.len() + just.deep.len()
        );
    } else {
        println!("matrix: RATCHET FAIL ({} violations):", failures.len());
        for failure in &failures {
            println!("  {failure}");
        }
        std::process::exit(1);
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).is_some_and(|arg| arg == "--merge") {
        if argv.len() < 4 {
            eprintln!(
                "usage: {} --merge <out.json> <in1.json> [in2.json ...]",
                argv[0]
            );
            std::process::exit(2);
        }
        run_merge(&argv[2], &argv[3..]);
        return;
    }
    if argv.get(1).is_some_and(|arg| arg == "--check") {
        let mut ratchet = None;
        let mut justifications = None;
        let mut deep = None;
        let mut fresh: Vec<String> = Vec::new();
        let mut index = 2;
        while index < argv.len() {
            match argv[index].as_str() {
                "--ratchet" => {
                    index += 1;
                    ratchet = argv.get(index).cloned();
                }
                "--justifications" => {
                    index += 1;
                    justifications = argv.get(index).cloned();
                }
                "--deep" => {
                    index += 1;
                    deep = argv.get(index).cloned();
                }
                path => fresh.push(path.to_owned()),
            }
            index += 1;
        }
        match (ratchet, justifications, deep) {
            (Some(ratchet), Some(justifications), Some(deep)) if !fresh.is_empty() => {
                run_check(&ratchet, &justifications, &deep, &fresh);
            }
            _ => {
                eprintln!(
                    "usage: {} --check --ratchet R --justifications J --deep D <fresh1.json> [fresh2.json ...]",
                    argv[0]
                );
                std::process::exit(2);
            }
        }
        return;
    }
    let mut out: Option<String> = None;
    let mut programs: usize = 2000;
    let mut start: usize = 0;
    let mut seeds = false;
    let mut probes = false;
    let mut files_from: Vec<String> = Vec::new();
    let mut static_only = false;
    let mut optimize = false;
    let mut extras: Vec<String> = Vec::new();
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--programs" => {
                index += 1;
                programs = argv
                    .get(index)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage(&argv[0]));
            }
            "--start" => {
                index += 1;
                start = argv
                    .get(index)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage(&argv[0]));
            }
            "--seeds" => seeds = true,
            "--probes" => probes = true,
            "--static-only" => static_only = true,
            "--optimize" => optimize = true,
            "--files-from" => {
                index += 1;
                files_from.push(argv.get(index).cloned().unwrap_or_else(|| usage(&argv[0])));
            }
            flag if flag.starts_with("--") => usage(&argv[0]),
            path => {
                if out.is_none() {
                    out = Some(path.to_owned());
                } else {
                    extras.push(path.to_owned());
                }
            }
        }
        index += 1;
    }
    let Some(out) = out else { usage(&argv[0]) };

    // Feed: battery range, then canonical seeds, then extras (all ids unique).
    let mut inputs: Vec<(String, String)> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    for seed in start..start.saturating_add(programs) {
        inputs.push((format!("battery-{seed:05}"), battery_program(seed)));
    }
    let mut file_paths: Vec<String> = extras;
    if seeds {
        file_paths.extend(js_files("seeds", ""));
        file_paths.extend(js_files("../regression/cases", "repro.js"));
    }
    if probes {
        file_paths.extend(js_files("probes", ""));
    }
    for list in files_from {
        match std::fs::read_to_string(&list) {
            Ok(text) => file_paths.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned),
            ),
            Err(err) => {
                eprintln!("matrix: cannot read --files-from {list}: {err}");
                std::process::exit(2);
            }
        }
    }
    for path in file_paths {
        match std::fs::read_to_string(&path) {
            Ok(source) => inputs.push((format!("file:{path}"), source)),
            Err(err) => skipped.push(Skipped {
                id: format!("file:{path}"),
                reason: format!("read: {err}"),
            }),
        }
    }

    let mut rows: Vec<ProgramRow> = Vec::new();
    let mut statics: Vec<Vec<String>> = Vec::new();
    let mut dynamics: Vec<HashMap<String, u64>> = Vec::new();
    let mut battery_panics: Vec<String> = Vec::new();
    for (done, (id, source)) in inputs.iter().enumerate() {
        if done % 500 == 0 {
            eprintln!("matrix: {done}/{} ...", inputs.len());
        }
        let must_parse = !id.starts_with("file:");
        let cells = match static_cells(source, must_parse, id, optimize) {
            Ok(cells) => cells,
            Err(reason) => {
                eprintln!("matrix: skip {id}: {reason}");
                skipped.push(Skipped {
                    id: id.clone(),
                    reason,
                });
                continue;
            }
        };
        if static_only {
            statics.push(cells.clone());
            dynamics.push(HashMap::new());
            rows.push(ProgramRow {
                id: id.clone(),
                outcome: String::from("static-only"),
                static_cells: cells,
                dynamic: BTreeMap::new(),
            });
            continue;
        }
        let Some((histogram, outcome)) = dynamic_isolated(source, optimize) else {
            // Panicking programs keep their static evidence with an empty
            // dynamic view; battery panics fail the run at the end.
            eprintln!("matrix: {id} dynamic leg panicked (recorded)");
            if must_parse {
                battery_panics.push(id.clone());
            }
            statics.push(cells.clone());
            dynamics.push(HashMap::new());
            rows.push(ProgramRow {
                id: id.clone(),
                outcome: String::from("panic: engine panicked during evaluation"),
                static_cells: cells,
                dynamic: BTreeMap::new(),
            });
            continue;
        };
        statics.push(cells.clone());
        dynamics.push(histogram.iter().map(|(k, v)| (k.clone(), *v)).collect());
        rows.push(ProgramRow {
            id: id.clone(),
            outcome,
            static_cells: cells,
            dynamic: histogram,
        });
    }

    let (merged, malformed) = merge_matrix(&statics, &dynamics);
    let report = Report {
        battery_version: BATTERY_VERSION,
        programs: rows,
        skipped,
        merged,
        malformed: malformed.clone(),
        battery_panics: battery_panics.clone(),
    };
    let json = serde_json::to_string_pretty(&report).expect("serialize matrix report");
    std::fs::write(&out, &json).expect("write matrix report");
    eprintln!(
        "matrix: {} programs, {} matrix rows, {} skipped -> {out}",
        report.programs.len(),
        report.merged.len(),
        report.skipped.len()
    );
    if !malformed.is_empty() {
        eprintln!("matrix: MALFORMED CELLS (engine invariant violated): {malformed:?}");
    }
    if !battery_panics.is_empty() {
        eprintln!(
            "matrix: BATTERY PANICS (valid-by-construction input panicked): {battery_panics:?}"
        );
    }
    if !malformed.is_empty() || !battery_panics.is_empty() {
        std::process::exit(1);
    }
}
