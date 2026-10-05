//! End-to-end tests for subprocess isolation (`run --timeout`), the
//! `run-single` child protocol, and `compare` exit codes.
//!
//! These tests drive the real `boa_tester` binary against a synthetic
//! mini-Test262 corpus, so a hang (`while (true) {}`) proves the timeout
//! machinery instead of hanging the test run.

// The binary under test is driven through `CARGO_BIN_EXE_boa_tester`; this
// target intentionally uses only `std`.
#![allow(unused_crate_dependencies)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn tester() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boa_tester"))
}

fn unique_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "boa_tester_iso_{}_{}_{nanos}",
        std::process::id(),
        tag
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Builds a synthetic Test262 corpus: harness files plus one passing, one
/// hanging, and one huge-completion-value test. Returns
/// `(test262_root, config_path)`.
fn mini_corpus(root: &Path) -> (PathBuf, PathBuf) {
    let test262 = root.join("test262");
    std::fs::create_dir_all(test262.join("harness")).expect("harness dir");
    std::fs::create_dir_all(test262.join("test")).expect("test dir");
    for name in ["assert.js", "sta.js", "doneprintHandle.js"] {
        std::fs::write(test262.join("harness").join(name), "// stub\n").expect("write");
    }
    std::fs::write(
        test262.join("test").join("pass.js"),
        "/*---\ndescription: passes\n---*/\nvar x = 1;\n",
    )
    .expect("write");
    std::fs::write(
        test262.join("test").join("hang.js"),
        "/*---\ndescription: hangs forever\n---*/\nwhile (true) {}\n",
    )
    .expect("write");
    // Completion value (~128 KiB) dwarfs the 64 KiB OS pipe buffer: without
    // child-side truncation + concurrent parent draining, the isolated child
    // blocks mid-write and is misreported as a timeout.
    std::fs::write(
        test262.join("test").join("chatty.js"),
        "/*---\ndescription: huge completion value\n---*/\n'0123456789abcdef'.repeat(8192);\n",
    )
    .expect("write");
    // Lets `get_test262_commit` fall back without a git checkout.
    std::fs::create_dir_all(test262.join(".git/refs/heads")).expect("git dir");
    std::fs::write(test262.join(".git/refs/heads/main"), "deadbeef\n").expect("write");

    let config = root.join("config.toml");
    std::fs::write(&config, "commit = \"\"\n[ignored]\nflags = []\n").expect("write");
    (test262, config)
}

#[test]
fn timeout_kills_hangs_and_counts_them() {
    let root = unique_temp_dir("timeout");
    let (test262, config) = mini_corpus(&root);
    let out = root.join("out");

    let status = tester()
        .arg("run")
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-s")
        .arg("test")
        .arg("-o")
        .arg(&out)
        .arg("--timeout")
        .arg("3")
        .arg("-d")
        .status()
        .expect("run tester");
    assert!(status.success(), "run exits 0 (recording, not gating)");

    let latest = std::fs::read_to_string(out.join("latest.json")).expect("read latest");
    assert!(latest.contains(r#""r":"T""#), "hang counted as Timeout");
    assert!(latest.contains(r#""r":"O""#), "pass counted as Passed");
    assert!(latest.contains(r#""to":1"#), "timeout stats counter set");

    let crashes = std::fs::read_to_string(out.join("crashes.json")).expect("read crashes");
    assert!(
        crashes.contains("timeout@"),
        "timeout crash record stored: {crashes}"
    );

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

#[test]
fn large_child_output_completes_without_timeout() {
    let root = unique_temp_dir("chatty");
    let (test262, config) = mini_corpus(&root);
    let out = root.join("out");

    let status = tester()
        .arg("run")
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-s")
        .arg("test")
        .arg("-o")
        .arg(&out)
        .arg("--timeout")
        .arg("5")
        .arg("-d")
        .status()
        .expect("run tester");
    assert!(status.success(), "run exits 0 (recording, not gating)");

    let latest = std::fs::read_to_string(out.join("latest.json")).expect("read latest");
    let at = latest
        .find(r#""n":"chatty""#)
        .expect("chatty entry present");
    let entry: String = latest.chars().skip(at).take(120).collect();
    assert!(
        entry.contains(r#""r":"O""#),
        "chatty passes instead of deadlocking into a timeout: {entry}"
    );

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

#[test]
fn run_single_prints_exactly_one_outcome_line() {
    let root = unique_temp_dir("single");
    let (test262, _) = mini_corpus(&root);

    let output = tester()
        .arg("run-single")
        .arg("--test262-path")
        .arg(&test262)
        .arg("--test")
        .arg(test262.join("test").join("pass.js"))
        .output()
        .expect("run child");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1, "single JSON line: {stdout:?}");
    assert!(stdout.contains(r#""outcome":"O""#), "{stdout:?}");

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

#[test]
fn report_builds_matrix_and_gates_trend() {
    let root = unique_temp_dir("report");
    let (test262, config) = mini_corpus(&root);
    let out = root.join("out");

    let status = tester()
        .arg("run")
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-s")
        .arg("test")
        .arg("-o")
        .arg(&out)
        .arg("--timeout")
        .arg("5")
        .arg("-d")
        .status()
        .expect("run tester");
    assert!(status.success());

    let areas = root.join("areas.toml");
    std::fs::write(
        &areas,
        "[areas.other]\nlayer = \"test\"\nwork_item = \"P2.e2e\"\nstatus = \"open\"\nnotes = \"synthetic\"\n",
    )
    .expect("write areas");
    let matrix_dir = root.join("matrix");
    let status = tester()
        .arg("report")
        .arg(&out)
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("--areas")
        .arg(&areas)
        .arg("-o")
        .arg(&matrix_dir)
        .status()
        .expect("run report");
    assert!(status.success());

    let json_path = matrix_dir.join("conformance-gap.json");
    let json = std::fs::read_to_string(&json_path).expect("read matrix");
    assert!(json.contains(r#""suite_root": "test""#), "{json}");
    assert!(json.contains(r#""t": 3"#), "{json}");
    assert!(json.contains(r#""test/hang""#), "hang is a gap entry");
    assert!(!json.contains(r#""test/pass""#), "passes are not entries");
    assert!(json.contains("P2.e2e"), "curated triage joined");
    let md = std::fs::read_to_string(matrix_dir.join("conformance-gap.md")).expect("read md");
    assert!(md.contains("# Conformance gap matrix"), "{md}");

    // Checking against itself passes.
    let status = tester()
        .arg("report")
        .arg(&out)
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-o")
        .arg(&matrix_dir)
        .arg("--check")
        .arg(&json_path)
        .status()
        .expect("run report check");
    assert_eq!(status.code(), Some(0), "self-check passes");

    // An inflated previous pass count breaches the ratchet.
    let tampered = match json.split_once(r#""o": 2"#) {
        Some((head, tail)) => format!("{head}\"o\": 3{tail}"),
        None => panic!("tamper anchor missing"),
    };
    assert_ne!(tampered, json, "tamper landed");
    std::fs::write(root.join("prev.json"), &tampered).expect("write prev");
    let status = tester()
        .arg("report")
        .arg(&out)
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-o")
        .arg(&matrix_dir)
        .arg("--check")
        .arg(root.join("prev.json"))
        .status()
        .expect("run report check");
    assert_eq!(status.code(), Some(1), "ratchet breach exits 1");

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

/// Minimal `latest.json` with one test per `(name, outcome-letter)`.
fn fixture(tests: &[(&str, &str)]) -> String {
    let count = |want: &str| tests.iter().filter(|(_, o)| *o == want).count().to_string();
    let stats = format!(
        "{{\"t\":{},\"o\":{},\"i\":{},\"p\":{}}}",
        tests.len(),
        count("O"),
        count("I"),
        count("P")
    );
    let versioned = [
        "es5", "es6", "es7", "es8", "es9", "es10", "es11", "es12", "es13",
    ]
    .iter()
    .map(|edition| format!("{edition:?}:{stats}"))
    .collect::<Vec<_>>()
    .join(",");
    let entries = tests
        .iter()
        .map(|(name, outcome)| format!(r#"{{"n":{name:?},"v":5,"r":{outcome:?}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"c\":\"\",\"u\":\"\",\"r\":{{\"n\":\"test\",\"a\":{stats},\"av\":{{{versioned}}},\
         \"s\":[{{\"n\":\"lang\",\"a\":{stats},\"av\":{{{versioned}}},\"t\":[{entries}]}}]}}}}"
    )
}

#[test]
fn compare_strict_mode_exits_nonzero_on_breach() {
    let root = unique_temp_dir("compare");
    std::fs::write(
        root.join("base.json"),
        fixture(&[("a", "O"), ("b", "O"), ("c", "O")]),
    )
    .expect("write");
    std::fs::write(
        root.join("new.json"),
        fixture(&[("a", "C"), ("b", "T"), ("c", "I")]),
    )
    .expect("write");

    let status = tester()
        .arg("compare")
        .arg(root.join("base.json"))
        .arg(root.join("new.json"))
        .arg("--fail-on=crash,timeout,new-skip")
        .status()
        .expect("run compare");
    assert_eq!(status.code(), Some(1), "breach exits 1");

    let status = tester()
        .arg("compare")
        .arg(root.join("base.json"))
        .arg(root.join("new.json"))
        .status()
        .expect("run compare");
    assert_eq!(status.code(), Some(0), "advisory compare exits 0");

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

#[test]
fn gc_stress_run_matches_normal_run() {
    // The production gating procedure, end to end: the same mini corpus
    // (pass + hang + huge-completion) run normally and under `--gc-stress`
    // (isolated children, so the flag's argv plumbing is covered too) must
    // compare clean under `--fail-on=regression`.
    let root = unique_temp_dir("gcstress");
    let (test262, config) = mini_corpus(&root);
    let normal = root.join("normal");
    let stressed = root.join("stressed");

    for (out, stressed_args) in [(&normal, false), (&stressed, true)] {
        let mut run = tester();
        run.arg("run")
            .arg("--test262-path")
            .arg(&test262)
            .arg("-c")
            .arg(&config)
            .arg("-s")
            .arg("test")
            .arg("-o")
            .arg(out)
            .arg("--timeout")
            .arg("10")
            .arg("-d");
        if stressed_args {
            run.arg("--gc-stress");
        }
        let status = run.status().expect("run tester");
        assert!(status.success(), "run exits 0 (recording, not gating)");
    }

    let status = tester()
        .arg("compare")
        .arg(&normal)
        .arg(&stressed)
        .arg("--fail-on=regression")
        .status()
        .expect("run compare");
    assert_eq!(
        status.code(),
        Some(0),
        "stressed run compares clean against normal"
    );

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

#[test]
fn gc_stress_covers_in_process_and_child_paths() {
    let root = unique_temp_dir("gcpaths");
    let (test262, config) = mini_corpus(&root);

    // In-process single test under stress (no `--timeout`, no children).
    let status = tester()
        .arg("run")
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-s")
        .arg("test/pass.js")
        .arg("--gc-stress")
        .arg("50")
        .status()
        .expect("run tester");
    assert!(status.success(), "in-process stressed run exits 0");

    // The isolated child honors an explicit cadence directly.
    let output = tester()
        .arg("run-single")
        .arg("--test262-path")
        .arg(&test262)
        .arg("--test")
        .arg(test262.join("test").join("pass.js"))
        .arg("--gc-stress")
        .arg("7")
        .output()
        .expect("run child");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1, "single JSON line: {stdout:?}");
    assert!(stdout.contains(r#""outcome":"O""#), "{stdout:?}");

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}

#[test]
fn gc_stress_zero_is_rejected() {
    let root = unique_temp_dir("gczero");
    let (test262, config) = mini_corpus(&root);

    let output = tester()
        .arg("run")
        .arg("--test262-path")
        .arg(&test262)
        .arg("-c")
        .arg(&config)
        .arg("-s")
        .arg("test/pass.js")
        .arg("--gc-stress")
        .arg("0")
        .output()
        .expect("run tester");
    assert_ne!(output.status.code(), Some(0), "run rejects zero cadence");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonzero"),
        "run explains itself: {stderr:?}"
    );

    let output = tester()
        .arg("run-single")
        .arg("--test262-path")
        .arg(&test262)
        .arg("--test")
        .arg(test262.join("test").join("pass.js"))
        .arg("--gc-stress")
        .arg("0")
        .output()
        .expect("run child");
    assert_ne!(
        output.status.code(),
        Some(0),
        "run-single rejects zero cadence"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonzero"),
        "run-single explains itself: {stderr:?}"
    );

    std::fs::remove_dir_all(&root).expect("remove temp dir");
}
