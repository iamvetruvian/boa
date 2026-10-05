//! End-to-end tests for the differential pipeline: `run` against the pinned
//! oracle, `triage --check` gating, bulk verdicts, verdict export, and
//! `--verdicts` preload.
//!
//! These tests drive the real `boa_differential` binary and require the
//! fetched oracle (`BOA_DIFF_ORACLE`, see `scripts/fetch-oracle.sh`); without
//! it they report a skip instead of failing, since no oracle preference is
//! expressible in-test. CI fetches the oracle before running them.

// The binary under test is driven through `CARGO_BIN_EXE_boa_differential`;
// this target intentionally uses only `std`.
#![allow(unused_crate_dependencies, clippy::too_many_lines)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn differential() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boa_differential"))
}

fn unique_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir =
        std::env::temp_dir().join(format!("boa_diff_e2e_{}_{tag}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Oracle binary path, or `None` when the e2e tests must skip.
fn oracle_path() -> Option<PathBuf> {
    let path = std::env::var_os("BOA_DIFF_ORACLE").map(PathBuf::from)?;
    if path.is_file() { Some(path) } else { None }
}

/// Builds a two-case adversarial corpus: one agreeing case and one
/// must-flag case (derived global-`toStringTag` string, which the
/// normalizer deliberately compares strictly).
fn mini_corpus(root: &Path) -> PathBuf {
    let adv = root.join("adv");
    std::fs::create_dir_all(&adv).expect("adv dir");
    std::fs::write(adv.join("agree.js"), "print(40 + 2);\nvar e2e_x = 1;\n").expect("agree");
    std::fs::write(
        adv.join("tag.js"),
        "Object.prototype.toString.call(globalThis);\n",
    )
    .expect("tag");
    let manifest = root.join("corpus.toml");
    std::fs::write(
        &manifest,
        format!(
            "schema_version = 1\ntool = \"boa_differential\"\nname = \"e2e\"\nstrict_variants = false\n\n[adversarial]\ndir = \"{}\"\n",
            adv.display(),
        ),
    )
    .expect("manifest");
    manifest
}

/// Reads the pinned oracle version from the fetch script (single pin source).
fn pinned_version() -> String {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fetch =
        std::fs::read_to_string(repo.join("scripts/fetch-oracle.sh")).expect("fetch script");
    for line in fetch.lines() {
        if let Some(version) = line.strip_prefix("ORACLE_VERSION=\"") {
            return version.strip_suffix('"').expect("quote").to_owned();
        }
    }
    panic!("ORACLE_VERSION not found in fetch-oracle.sh");
}

#[test]
fn full_loop_flags_triages_gates_and_preloads() {
    let Some(oracle) = oracle_path() else {
        eprintln!("skip: BOA_DIFF_ORACLE not set (run scripts/fetch-oracle.sh)");
        return;
    };
    let version = pinned_version();
    let root = unique_temp_dir("loop");
    let manifest = mini_corpus(&root);
    let out = root.join("out");

    // Run: one agree, one mismatch.
    let status = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            &version,
            "--output",
            out.to_str().expect("utf8"),
            "--serial",
        ])
        .status()
        .expect("run");
    assert!(status.success());
    let results = std::fs::read_to_string(out.join("diff-results.json")).expect("results");
    assert!(results.contains("\"agree\": 1"), "{results}");
    assert!(results.contains("\"mismatch\": 1"), "{results}");

    // Gate breaches on the untriaged mismatch.
    let queue = out.join("triage-queue.json");
    let gate = differential()
        .args([
            "triage",
            "--queue",
            queue.to_str().expect("utf8"),
            "--check",
        ])
        .status()
        .expect("gate");
    assert_eq!(gate.code(), Some(1));

    // Bulk-verdict by area, then the gate passes.
    let verdict = differential()
        .args([
            "triage",
            "--queue",
            queue.to_str().expect("utf8"),
            "--set-verdict",
            "oracle-quirk",
            "--area",
            "adversarial",
            "--note",
            "e2e: derived global tag string",
        ])
        .status()
        .expect("verdict");
    assert!(verdict.success());
    let gate = differential()
        .args([
            "triage",
            "--queue",
            queue.to_str().expect("utf8"),
            "--check",
        ])
        .status()
        .expect("gate");
    assert!(gate.success());

    // Export + fresh preload: the gate passes without re-triage.
    let verdicts = root.join("verdicts.toml");
    let export = differential()
        .args([
            "triage",
            "--queue",
            queue.to_str().expect("utf8"),
            "--export",
            verdicts.to_str().expect("utf8"),
        ])
        .status()
        .expect("export");
    assert!(export.success());
    let out2 = root.join("out2");
    let rerun = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            &version,
            "--verdicts",
            verdicts.to_str().expect("utf8"),
            "--output",
            out2.to_str().expect("utf8"),
            "--serial",
        ])
        .output()
        .expect("rerun");
    assert!(rerun.status.success());
    let log = String::from_utf8_lossy(&rerun.stdout);
    assert!(log.contains("pre-triaged"), "{log}");
    // Pre-triaged signatures skip re-minimization (marked, never silent).
    assert!(log.contains("skipped (pre-triaged signatures)"), "{log}");
    let rerun_results =
        std::fs::read_to_string(out2.join("diff-results.json")).expect("rerun results");
    assert!(
        rerun_results.contains("\"minimized_by\": \"verdict-skipped-v1\""),
        "{rerun_results}"
    );
    let gate = differential()
        .args([
            "triage",
            "--queue",
            out2.join("triage-queue.json").to_str().expect("utf8"),
            "--check",
        ])
        .status()
        .expect("gate");
    assert!(gate.success());
}

#[test]
fn prototype_poisoning_stays_observable() {
    let Some(oracle) = oracle_path() else {
        eprintln!("skip: BOA_DIFF_ORACLE not set (run scripts/fetch-oracle.sh)");
        return;
    };
    // A non-writable indexed property on Array.prototype used to break the
    // driver's own push/sort on both sides at once (prototype-chain Set
    // lookup); the epilogue's null-prototype containers must stay immune.
    let root = unique_temp_dir("prot");
    let adv = root.join("adv");
    std::fs::create_dir_all(&adv).expect("adv dir");
    std::fs::write(
        adv.join("frozen.js"),
        "Object.defineProperty(Array.prototype, \"0\", { value: 7, writable: false });\nvar frozen_ok = [].concat([1]);\n",
    )
    .expect("frozen");
    let manifest = root.join("corpus.toml");
    std::fs::write(
        &manifest,
        format!(
            "schema_version = 1\ntool = \"boa_differential\"\nname = \"e2e-prot\"\nstrict_variants = false\n\n[adversarial]\ndir = \"{}\"\n",
            adv.display(),
        ),
    )
    .expect("manifest");
    let out = root.join("out");
    let run = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            &pinned_version(),
            "--output",
            out.to_str().expect("utf8"),
            "--serial",
        ])
        .output()
        .expect("run");
    assert!(run.status.success());
    let results = std::fs::read_to_string(out.join("diff-results.json")).expect("results");
    assert!(results.contains("\"agree\": 1"), "{results}");
    assert!(!results.contains("OneSided"), "{results}");
}

#[test]
fn refuses_unpinned_oracle_and_foreign_verdicts() {
    let Some(oracle) = oracle_path() else {
        eprintln!("skip: BOA_DIFF_ORACLE not set (run scripts/fetch-oracle.sh)");
        return;
    };
    let root = unique_temp_dir("refuse");
    let manifest = mini_corpus(&root);

    // Wrong expected version: hard refusal, no run.
    let run = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            "definitely-not-pinned",
            "--output",
            root.join("out").to_str().expect("utf8"),
        ])
        .output()
        .expect("run");
    assert!(!run.status.success());
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(log.contains("unpinned engine"), "{log}");

    // Verdicts file for a different oracle: hard refusal.
    let verdicts = root.join("foreign.toml");
    std::fs::write(
        &verdicts,
        "schema_version = 1\ntool = \"boa_differential\"\noracle_kind = \"node\"\noracle_version = \"v0\"\n",
    )
    .expect("foreign verdicts");
    let run = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            &pinned_version(),
            "--verdicts",
            verdicts.to_str().expect("utf8"),
            "--output",
            root.join("out2").to_str().expect("utf8"),
        ])
        .output()
        .expect("run");
    assert!(!run.status.success());
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert!(log.contains("foreign verdicts"), "{log}");
}

#[test]
fn canblock_true_agrees_and_false_excludes() {
    let Some(oracle) = oracle_path() else {
        eprintln!("skip: BOA_DIFF_ORACLE not set (run scripts/fetch-oracle.sh)");
        return;
    };
    // Both sides are blockable (jsshell main thread blocks; Boa side sets
    // `[[CanBlock]]`), so a finite-timeout `Atomics.wait` agrees, while the
    // unprovidable `CanBlockIsFalse` case is excluded with its reason.
    let root = unique_temp_dir("canblock");
    let fake = root.join("t262");
    std::fs::create_dir_all(fake.join("harness")).expect("harness");
    std::fs::write(fake.join("harness/assert.js"), "var assert = {};\n").expect("assert");
    std::fs::write(fake.join("harness/sta.js"), "var sta = 1;\n").expect("sta");
    std::fs::create_dir_all(fake.join("case")).expect("slice");
    std::fs::write(
        fake.join("case/block-false.js"),
        "/*---\nflags: [CanBlockIsFalse]\n---*/\nvar a = 1;\n",
    )
    .expect("write");
    std::fs::write(
        fake.join("case/block-true.js"),
        "/*---\nflags: [CanBlockIsTrue]\n---*/\nAtomics.wait(new Int32Array(new SharedArrayBuffer(16)), 0, 0, -1);\n",
    )
    .expect("write");
    let manifest = root.join("corpus.toml");
    std::fs::write(
        &manifest,
        format!(
            "schema_version = 1\ntool = \"boa_differential\"\nname = \"canblock\"\nstrict_variants = false\n\n[test262]\nroot = \"{}\"\nslices = [\"case\"]\n",
            fake.display(),
        ),
    )
    .expect("manifest");
    let out = root.join("out");
    let status = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            &pinned_version(),
            "--output",
            out.to_str().expect("utf8"),
            "--serial",
        ])
        .status()
        .expect("run");
    assert!(status.success());
    let results = std::fs::read_to_string(out.join("diff-results.json")).expect("results");
    assert!(results.contains("\"agree\": 1"), "{results}");
    assert!(
        results.contains("\"reason\": \"canblock-false\""),
        "{results}"
    );
}

#[test]
fn frozen_global_and_tojson_poisoning_stay_observable() {
    let Some(oracle) = oracle_path() else {
        eprintln!("skip: BOA_DIFF_ORACLE not set (run scripts/fetch-oracle.sh)");
        return;
    };
    // Two programs that used to blind both sides at once: freezing the
    // global object silently breaks every sloppy driver write after the
    // freeze (the driver kept state in `var` globals), and an inherited
    // Object.prototype.toJSON turns JSON.stringify markers unparsable.
    // The driver keeps state in IIFE locals and pre-serializes markers,
    // so both cases must observe and agree.
    let root = unique_temp_dir("harden");
    let adv = root.join("adv");
    std::fs::create_dir_all(&adv).expect("adv dir");
    std::fs::write(
        adv.join("freeze.js"),
        "Object.freeze(globalThis);\nvar hard_frozen = 1;\n",
    )
    .expect("freeze");
    std::fs::write(
        adv.join("tojson.js"),
        "Object.prototype.toJSON = function () { return 2; };\nvar hard_json = 2;\n",
    )
    .expect("tojson");
    let manifest = root.join("corpus.toml");
    std::fs::write(
        &manifest,
        format!(
            "schema_version = 1\ntool = \"boa_differential\"\nname = \"e2e-harden\"\nstrict_variants = false\n\n[adversarial]\ndir = \"{}\"\n",
            adv.display(),
        ),
    )
    .expect("manifest");
    let out = root.join("out");
    let run = differential()
        .args([
            "run",
            "--corpus",
            manifest.to_str().expect("utf8"),
            "--oracle",
            oracle.to_str().expect("utf8"),
            "--expected-version",
            &pinned_version(),
            "--output",
            out.to_str().expect("utf8"),
            "--serial",
        ])
        .output()
        .expect("run");
    assert!(run.status.success());
    let results = std::fs::read_to_string(out.join("diff-results.json")).expect("results");
    assert!(results.contains("\"agree\": 2"), "{results}");
    assert!(!results.contains("OneSided"), "{results}");
}
