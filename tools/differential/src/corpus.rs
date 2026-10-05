//! Seed-corpus manifests and case loading.
//!
//! A v1 corpus = pinned Test262 slices + regression-DB reproducers + a
//! curated adversarial set. Every exclusion is counted and reported by
//! reason — nothing is silently skipped:
//!
//! - Test262 slices exclude module/async tests (the v1 driver is
//!   synchronous Classic scripts), `$262`-dependent tests (host API absent
//!   on the oracle side), time/random-dependent tests (frozen vs real
//!   clocks can never agree literally), driver-token collisions,
//!   `CanBlockIsFalse` tests (the pinned oracle's main thread always
//!   blocks, so `[[CanBlock]]=false` is unprovidable there and comparison
//!   is uninformative), and — mechanically, via `exclude_from_gap_matrix`
//!   — the P2-matrix-tracked red set (already triaged in
//!   `docs/conformance-gap.md`; differential v1 is an agreement ratchet on
//!   green territory, not a second copy of the gap list).
//! - Regression repros run with an identical pure-JS `assert` prelude on
//!   both sides (the DB harness's injected globals, inlined); repros that
//!   pin engine-defined behavior are excluded by manifest id (`exclude_ids`,
//!   reason `engine-specific`).
//!
//! Manifest paths resolve against the process working directory; run from
//! the repository root (same convention as the tester).

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use cow_utils::CowUtils;
use serde::Deserialize;

use crate::compose::check_program_clean;

/// Corpus manifest (`corpora/*.toml`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest schema version (must be 1).
    pub schema_version: u8,
    /// Writer identity (must be `boa_differential`).
    pub tool: String,
    /// Corpus name for records and logs.
    pub name: String,
    /// Also run a `"use strict";`-prefixed variant of every eligible case.
    ///
    /// Caveat (verified symmetric on both engines): strict eval code gets a
    /// fresh declarative environment (`EvalDeclarationInstantiation` step 3a),
    /// so strict variants never add `var`/function bindings to `globalThis`
    /// and their state dumps are always empty. Strict variants therefore
    /// compare completion/kind/console only — still valuable (e.g. the
    /// strict-assign and arguments-mapping probes), but never state.
    #[serde(default)]
    pub strict_variants: bool,
    /// Test262 slices (optional section).
    #[serde(default)]
    pub test262: Option<Test262Section>,
    /// Regression reproducers (optional section).
    #[serde(default)]
    pub regression: Option<RegressionSection>,
    /// Curated adversarial directory (optional section).
    #[serde(default)]
    pub adversarial: Option<AdversarialSection>,
}

/// Test262 slice selection.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test262Section {
    /// Test262 checkout root.
    pub root: PathBuf,
    /// Suite dirs relative to `root` (e.g. `test/language/expressions`).
    pub slices: Vec<PathBuf>,
    /// Gap-matrix JSON whose entry ids are mechanically excluded as
    /// P2-tracked (optional but expected for green-slice corpora).
    #[serde(default)]
    pub exclude_from_gap_matrix: Option<PathBuf>,
}

/// Regression-DB selection.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegressionSection {
    /// `regressions.toml` manifest path.
    pub manifest: PathBuf,
    /// Run every `landed` entry when true; otherwise use `ids`.
    #[serde(default)]
    pub all_landed: bool,
    /// Explicit entry ids (used when `all_landed` is false).
    #[serde(default)]
    pub ids: Vec<String>,
    /// Entry ids to skip as differential-incompatible: repros that pin
    /// engine-defined behavior (error message text, internal trap counts)
    /// can never agree across engines. They stay in the regression DB —
    /// that is their job — but run nowhere differential. Reported with
    /// reason `engine-specific`, never silently.
    #[serde(default)]
    pub exclude_ids: Vec<String>,
}

/// Adversarial directory selection.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdversarialSection {
    /// Directory of curated `.js` files (loaded in sorted order).
    pub dir: PathBuf,
}

/// One executable differential case.
#[derive(Debug, Clone)]
pub struct Case {
    /// Stable id: test262-relative path (no `.js`), `regression:<id>`, or
    /// `adversarial:<file>`; strict variants append `#strict`.
    pub id: String,
    /// Final program text (harness/prelude + test, before driver composition).
    pub program: String,
    /// Leading harness/prelude line count (fixed infrastructure: the
    /// minimizer never touches these lines and the line budget excludes them).
    pub prefix_lines: usize,
    /// Case origin for records (`test262`, `regression`, `adversarial`).
    pub origin: &'static str,
}

/// Why a candidate program was excluded from the corpus.
#[derive(Debug, Clone)]
pub struct Exclusion {
    /// Candidate id.
    pub id: String,
    /// Machine-readable reason (`module`, `async`, `host-262`, ...).
    pub reason: &'static str,
}

/// Loaded corpus: runnable cases plus the full exclusion accounting.
#[derive(Debug)]
pub struct Corpus {
    /// Corpus name from the manifest.
    pub name: String,
    /// Runnable cases in deterministic order.
    pub cases: Vec<Case>,
    /// Every excluded candidate with its reason.
    pub excluded: Vec<Exclusion>,
}

/// Minimal Test262 frontmatter: only what corpus filtering needs.
#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    includes: Vec<String>,
}

/// Minimal regression-manifest entry: only what case loading needs.
#[derive(Debug, Deserialize)]
struct RegressionManifest {
    #[serde(default)]
    entries: Vec<RegressionEntry>,
}

/// Minimal regression entry.
#[derive(Debug, Deserialize)]
struct RegressionEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    reproducer: String,
}

/// Pure-JS `assert` prelude injected before regression repros (identical on
/// both sides; mirrors the DB harness's injected globals).
const ASSERT_PRELUDE: &str = r#"
function assert(cond, msg) {
    if (!cond) { throw new Error("assert failed: " + (msg || "")); }
}
function assertEquals(actual, expected, msg) {
    if (actual !== expected) {
        throw new Error("assertEquals failed: " + actual + " !== " + expected + " " + (msg || ""));
    }
}
"#;

/// Substrings marking time/random-dependent programs (frozen-vs-real-clock
/// programs can never agree literally; excluded, never redacted-at-scale).
/// `new Date` deliberately has no trailing paren: `new Date;` (no-paren
/// now) evaded the old `new Date(` scan. `.call(Date` catches Date-as-ctor
/// smuggling (`Array.from.call(Date, …)` yields now on both sides).
const TIME_SCANS: &[&str] = &[
    "Date.now",
    "new Date",
    ".call(Date",
    "performance.",
    "Math.random",
    "console.time",
];

/// Loads and filters the corpus described by `manifest_path`.
pub fn load_corpus(manifest_path: &Path) -> Result<Corpus, String> {
    let text = std::fs::read_to_string(manifest_path)
        .map_err(|err| format!("could not read corpus manifest: {err}"))?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|err| format!("invalid corpus manifest: {err}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported corpus schema {}",
            manifest.schema_version
        ));
    }
    if manifest.tool != "boa_differential" {
        return Err(format!(
            "corpus tool is {:?}, expected \"boa_differential\"",
            manifest.tool
        ));
    }
    let mut cases = Vec::new();
    let mut excluded = Vec::new();
    if let Some(section) = &manifest.test262 {
        load_test262(section, manifest.strict_variants, &mut cases, &mut excluded)?;
    }
    if let Some(section) = &manifest.regression {
        load_regression(section, manifest.strict_variants, &mut cases, &mut excluded)?;
    }
    if let Some(section) = &manifest.adversarial {
        load_adversarial(section, manifest.strict_variants, &mut cases, &mut excluded)?;
    }
    Ok(Corpus {
        name: manifest.name,
        cases,
        excluded,
    })
}

/// Loads Test262 slice cases.
fn load_test262(
    section: &Test262Section,
    strict_variants: bool,
    cases: &mut Vec<Case>,
    excluded: &mut Vec<Exclusion>,
) -> Result<(), String> {
    let matrix_excluded = match &section.exclude_from_gap_matrix {
        Some(path) => load_gap_ids(path)?,
        None => BTreeSet::new(),
    };
    let harness_dir = section.root.join("harness");
    let read_harness = |name: &str| {
        std::fs::read_to_string(harness_dir.join(name))
            .map_err(|err| format!("could not read harness {name}: {err}"))
    };
    let assert_js = read_harness("assert.js")?;
    let sta_js = read_harness("sta.js")?;

    for slice in &section.slices {
        let dir = section.root.join(slice);
        for file in sorted_js_files(&dir)? {
            let id = test_id(&section.root, &file);
            if matrix_excluded.contains(&id) {
                excluded.push(Exclusion {
                    id,
                    reason: "matrix-tracked",
                });
                continue;
            }
            let source = std::fs::read_to_string(&file)
                .map_err(|err| format!("could not read {}: {err}", file.display()))?;
            let meta = parse_frontmatter(&source);
            if meta.flags.iter().any(|flag| flag == "module") {
                excluded.push(Exclusion {
                    id,
                    reason: "module",
                });
                continue;
            }
            if meta.flags.iter().any(|flag| flag == "async") {
                excluded.push(Exclusion {
                    id,
                    reason: "async",
                });
                continue;
            }
            // The pinned oracle (jsshell main thread) always blocks, so a
            // `CanBlockIsFalse` case can never run block-free there; the Boa
            // side runs every case blockable (tester `exec/mod.rs`
            // precedent: false only under this flag), hence exclusion.
            if meta.flags.iter().any(|flag| flag == "CanBlockIsFalse") {
                excluded.push(Exclusion {
                    id,
                    reason: "canblock-false",
                });
                continue;
            }
            let raw = meta.flags.iter().any(|flag| flag == "raw");
            let mut program = String::new();
            if !raw {
                program.push_str(&assert_js);
                program.push('\n');
                program.push_str(&sta_js);
                program.push('\n');
                for include in &meta.includes {
                    // Include names are harness-relative paths with extension
                    // (tester `read.rs` precedent: looked up verbatim).
                    // `testTypedArray.js` asserts a `TypedArray` global that
                    // no engine provides (intrinsic-only per SS 18); mirror
                    // the tester's runner-side injection so both sides run
                    // the test's substance instead of a harness ReferenceError.
                    if include == "testTypedArray.js" {
                        program.push_str("var TypedArray = Object.getPrototypeOf(Uint8Array);\n");
                    }
                    let content = read_harness(include)?;
                    program.push_str(&content);
                    program.push('\n');
                }
            }
            // Harness line count (exact newline count: the minimizer splits
            // the prefix on the same boundaries).
            let prefix_lines = program.matches('\n').count();
            program.push_str(&source);
            if program.contains("$262") {
                excluded.push(Exclusion {
                    id,
                    reason: "host-262",
                });
                continue;
            }
            if TIME_SCANS.iter().any(|scan| program.contains(scan)) {
                excluded.push(Exclusion {
                    id,
                    reason: "time-random",
                });
                continue;
            }
            if check_program_clean(&program).is_err() {
                excluded.push(Exclusion {
                    id,
                    reason: "driver-token",
                });
                continue;
            }
            push_variants(
                &id,
                &program,
                prefix_lines,
                "test262",
                &meta.flags,
                strict_variants,
                cases,
            );
        }
    }
    Ok(())
}

/// Loads regression-DB reproducer cases.
fn load_regression(
    section: &RegressionSection,
    strict_variants: bool,
    cases: &mut Vec<Case>,
    excluded: &mut Vec<Exclusion>,
) -> Result<(), String> {
    let text = std::fs::read_to_string(&section.manifest)
        .map_err(|err| format!("could not read regression manifest: {err}"))?;
    let manifest: RegressionManifest =
        toml::from_str(&text).map_err(|err| format!("invalid regression manifest: {err}"))?;
    let base = section.manifest.parent().unwrap_or_else(|| Path::new("."));
    for entry in &manifest.entries {
        if entry.status != "landed" {
            continue;
        }
        if !section.all_landed && !section.ids.iter().any(|id| id == &entry.id) {
            continue;
        }
        let id = format!("regression:{}", entry.id);
        if section
            .exclude_ids
            .iter()
            .any(|excluded| excluded == &entry.id)
        {
            excluded.push(Exclusion {
                id,
                reason: "engine-specific",
            });
            continue;
        }
        let source = std::fs::read_to_string(base.join(&entry.reproducer))
            .map_err(|err| format!("could not read reproducer for {}: {err}", entry.id))?;
        if check_program_clean(&source).is_err() {
            excluded.push(Exclusion {
                id,
                reason: "driver-token",
            });
            continue;
        }
        if TIME_SCANS.iter().any(|scan| source.contains(scan)) {
            excluded.push(Exclusion {
                id,
                reason: "time-random",
            });
            continue;
        }
        let mut program = String::from(ASSERT_PRELUDE);
        let prefix_lines = program.matches('\n').count();
        program.push_str(&source);
        push_variants(
            &id,
            &program,
            prefix_lines,
            "regression",
            &[],
            strict_variants,
            cases,
        );
    }
    Ok(())
}

/// Loads curated adversarial cases.
fn load_adversarial(
    section: &AdversarialSection,
    strict_variants: bool,
    cases: &mut Vec<Case>,
    excluded: &mut Vec<Exclusion>,
) -> Result<(), String> {
    for file in sorted_js_files(&section.dir)? {
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = format!("adversarial:{name}");
        let program = std::fs::read_to_string(&file)
            .map_err(|err| format!("could not read {}: {err}", file.display()))?;
        if check_program_clean(&program).is_err() {
            excluded.push(Exclusion {
                id,
                reason: "driver-token",
            });
            continue;
        }
        if TIME_SCANS.iter().any(|scan| program.contains(scan)) {
            excluded.push(Exclusion {
                id,
                reason: "time-random",
            });
            continue;
        }
        push_variants(&id, &program, 0, "adversarial", &[], strict_variants, cases);
    }
    Ok(())
}

/// Pushes the sloppy and (optionally) strict variants of a case.
fn push_variants(
    id: &str,
    program: &str,
    prefix_lines: usize,
    origin: &'static str,
    flags: &[String],
    strict_variants: bool,
    cases: &mut Vec<Case>,
) {
    let only_strict = flags.iter().any(|flag| flag == "onlyStrict");
    let no_strict = flags.iter().any(|flag| flag == "noStrict");
    if !only_strict {
        cases.push(Case {
            id: id.to_owned(),
            program: program.to_owned(),
            prefix_lines,
            origin,
        });
    }
    if only_strict || (strict_variants && !no_strict) {
        cases.push(Case {
            id: format!("{id}#strict"),
            program: format!("\"use strict\";\n{program}"),
            prefix_lines: prefix_lines + 1,
            origin,
        });
    }
}

/// Parses Test262 frontmatter (`/*--- ... ---*/`) into flags + includes.
fn parse_frontmatter(source: &str) -> Frontmatter {
    const START: &str = "/*---";
    const END: &str = "---*/";
    let Some(start) = source.find(START) else {
        return Frontmatter::default();
    };
    let Some(end) = source[start..].find(END) else {
        return Frontmatter::default();
    };
    let yaml = &source[start + START.len()..start + end];
    serde_yaml::from_str(yaml).unwrap_or_default()
}

/// Loads the gap-matrix entry-id set (mechanical P2-red exclusion).
fn load_gap_ids(path: &Path) -> Result<BTreeSet<String>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|err| format!("could not read gap matrix: {err}"))?;
    let matrix: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| format!("invalid gap matrix: {err}"))?;
    let Some(entries) = matrix.get("entries").and_then(serde_json::Value::as_object) else {
        return Err("gap matrix has no entries map".to_owned());
    };
    Ok(entries.keys().cloned().collect())
}

/// Test id: root-relative path without the `.js` extension (tester convention).
fn test_id(root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let mut id = relative
        .to_string_lossy()
        .cow_replace('\\', "/")
        .into_owned();
    if let Some(stripped) = id.strip_suffix(".js") {
        id = stripped.to_owned();
    }
    id
}

/// Recursively collects `.js` files in sorted order (deterministic).
fn sorted_js_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_js_files(dir, &mut files)?;
    files.sort();
    Ok(files)
}

/// Recursive helper for [`sorted_js_files`].
fn collect_js_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|err| format!("could not read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("could not read dir entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_js_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "js") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Test262Section, load_test262};
    use std::path::PathBuf;

    fn fake_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir()
            .join(format!("boa_diff_corpus_{tag}_{}", std::process::id()))
            .join(nanos.to_string());
        std::fs::create_dir_all(root.join("harness")).expect("harness dir");
        std::fs::write(root.join("harness/assert.js"), "var assert = {};\n").expect("assert");
        std::fs::write(root.join("harness/sta.js"), "var sta = 1;\n").expect("sta");
        std::fs::create_dir_all(root.join("test/slice")).expect("slice dir");
        root
    }

    #[test]
    fn canblock_flags_route() {
        // `CanBlockIsFalse` is unprovidable on the pinned oracle (its main
        // thread always blocks) and excluded; `CanBlockIsTrue` runs, since
        // the Boa side is blockable (tester `exec/mod.rs` precedent).
        let root = fake_root("canblock");
        std::fs::write(
            root.join("test/slice/block-false.js"),
            "/*---\nflags: [CanBlockIsFalse]\n---*/\nvar a = 1;\n",
        )
        .expect("write");
        std::fs::write(
            root.join("test/slice/block-true.js"),
            "/*---\nflags: [CanBlockIsTrue]\n---*/\nvar b = 2;\n",
        )
        .expect("write");
        let section = Test262Section {
            root: root.clone(),
            slices: vec![PathBuf::from("test/slice")],
            exclude_from_gap_matrix: None,
        };
        let mut cases = Vec::new();
        let mut excluded = Vec::new();
        load_test262(&section, false, &mut cases, &mut excluded).expect("load");
        assert_eq!(excluded.len(), 1);
        assert_eq!(excluded[0].reason, "canblock-false");
        assert!(
            excluded[0].id.ends_with("block-false"),
            "{}",
            excluded[0].id
        );
        assert_eq!(cases.len(), 1);
        assert!(cases[0].id.ends_with("block-true"), "{}", cases[0].id);
    }

    #[test]
    fn time_scans_catch_parenless_and_smuggled_date() {
        // Scan-gap regression: `new Date;` (no-paren now) and
        // `Array.from.call(Date, …)` both yield now on the oracle while
        // the Boa side runs a frozen epoch clock, so they must exclude
        // as time-random like their parenthesized cousins.
        let root = fake_root("timescan");
        std::fs::write(
            root.join("test/slice/noparen.js"),
            "/*---\n---*/\nvar d = new Date;\n",
        )
        .expect("write");
        std::fs::write(
            root.join("test/slice/slicensee.js"),
            "/*---\n---*/\nvar d = Array.from.call(Date, [\"A\"]);\n",
        )
        .expect("write");
        std::fs::write(root.join("test/slice/plain.js"), "var a = 1;\n").expect("write");
        let section = Test262Section {
            root: root.clone(),
            slices: vec![PathBuf::from("test/slice")],
            exclude_from_gap_matrix: None,
        };
        let mut cases = Vec::new();
        let mut excluded = Vec::new();
        load_test262(&section, false, &mut cases, &mut excluded).expect("load");
        assert_eq!(excluded.len(), 2);
        assert!(excluded.iter().all(|x| x.reason == "time-random"));
        assert_eq!(cases.len(), 1);
        assert!(cases[0].id.ends_with("plain"), "{}", cases[0].id);
    }
}
