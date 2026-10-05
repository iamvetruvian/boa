//! Aggregates WPT `records.jsonl` into `summary.json` and enforces the
//! zero-untriaged gate: any file that neither passes nor matches a live
//! audited ignore entry fails with exit 1.
//!
//! Usage:
//!   wpt-report <records.jsonl> [--config <test_wpt_config.toml>] [--out <dir>]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use boa_wpt::collect::{aggregate, utc_now_rfc3339, FileOutcome, WptSummary, SCHEMA_VERSION};

fn usage() -> ! {
    eprintln!("usage: wpt-report <records.jsonl> [--config <toml>] [--out <dir>]");
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut records_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config_path = Some(args.next().unwrap_or_else(|| usage()).into()),
            "--out" => out_dir = Some(args.next().unwrap_or_else(|| usage()).into()),
            _ if arg.starts_with('-') => usage(),
            _ if records_path.is_none() => records_path = Some(arg.into()),
            _ => usage(),
        }
    }
    let records_path = records_path.unwrap_or_else(|| usage());
    let out_dir = out_dir.unwrap_or_else(|| {
        records_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf()
    });

    let input = std::fs::read_to_string(&records_path)
        .unwrap_or_else(|err| panic!("could not read {}: {err}", records_path.display()));
    let mut records = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: FileOutcome = serde_json::from_str(line)
            .unwrap_or_else(|err| panic!("line {}: bad record: {err}", index + 1));
        records.push(record);
    }

    let config_path = config_path.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_wpt_config.toml")
    });
    let config_input = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("could not read {}: {err}", config_path.display()));
    let config: boa_wpt::collect::WptConfig =
        toml::from_str(&config_input).expect("invalid WPT config");

    let (counts, breaches) = aggregate(&records, &config);
    let mut files: BTreeMap<String, FileOutcome> = BTreeMap::new();
    for record in records {
        files.insert(record.file.clone(), record);
    }
    let summary = WptSummary {
        schema_version: SCHEMA_VERSION,
        tool: "wpt-report".to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        updated: utc_now_rfc3339(),
        wpt_rev: config.rev.clone().unwrap_or_default(),
        counts,
        files,
    };
    std::fs::create_dir_all(&out_dir).expect("could not create out dir");
    std::fs::write(
        out_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).expect("could not serialize summary"),
    )
    .expect("could not write summary.json");

    println!(
        "wpt: {} files ({} passed, {} ignored, {} failed, {} panics, {} timeouts, {} harness errors)",
        counts.total,
        counts.passed,
        counts.ignored,
        counts.failed(),
        counts.panic,
        counts.timeout,
        counts.harness_error,
    );
    if breaches.is_empty() {
        println!("wpt gate OK: zero untriaged files");
    } else {
        println!("BREACH: wpt: {} untriaged file(s):", breaches.len());
        for breach in &breaches {
            println!("  - {breach}");
        }
        std::process::exit(1);
    }
}
