//! Executable regression database.
//!
//! Every entry in `regressions.toml` is a fixed bug with a reproducer that
//! must stay green: `cargo test -p boa_regression` runs each `repro.js`
//! against the current engine and fails on the first red entry (all failures
//! are reported together). See `README.md` for the entry schema, the
//! admission rules, and the shared JSON-store conventions.

// The package library is intentionally empty; this target uses only dev-deps.
#![allow(unused_crate_dependencies)]

use std::path::{Path, PathBuf};

use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, Source,
    native_function::NativeFunction, object::FunctionObjectBuilder, property::Attribute,
    string::JsString,
};

/// Top-level regression manifest.
#[derive(Debug, serde::Deserialize)]
struct Manifest {
    schema_version: u8,
    tool: String,
    updated: String,
    entries: Vec<Entry>,
}

/// One regression entry. Field documentation lives in `README.md`.
#[derive(Debug, serde::Deserialize)]
struct Entry {
    id: String,
    title: String,
    status: String,
    reproducer: String,
    expect: String,
    layer: String,
    unit_test: String,
    #[serde(default)]
    test262_style: Option<String>,
    #[serde(default)]
    no_test262_reason: Option<String>,
    #[serde(default)]
    fuzz_seed: Option<String>,
    symptom: String,
    root_cause: String,
    #[serde(default)]
    spec: Option<String>,
    fix_commit: String,
    finder: String,
    added: String,
}

/// Every fixed bug keeps a green reproducer.
#[test]
fn regression_database() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: Manifest = toml::from_str(
        &std::fs::read_to_string(root.join("regressions.toml")).expect("read regressions.toml"),
    )
    .expect("parse regressions.toml");
    assert_eq!(manifest.schema_version, 1, "unsupported manifest version");
    assert_eq!(manifest.tool, "boa_regression");
    assert!(
        !manifest.updated.trim().is_empty(),
        "manifest `updated` is empty"
    );
    assert!(!manifest.entries.is_empty(), "no regression entries");

    let mut failures = Vec::new();
    for entry in &manifest.entries {
        if let Err(err) = run_entry(root, entry) {
            failures.push(format!("{}: {err}", entry.id));
        }
    }
    assert!(
        failures.is_empty(),
        "{} regression failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Validates one entry's manifest fields and runs its reproducer.
fn run_entry(root: &Path, entry: &Entry) -> Result<(), String> {
    for (field, value) in [
        ("id", entry.id.as_str()),
        ("title", entry.title.as_str()),
        ("status", entry.status.as_str()),
        ("reproducer", entry.reproducer.as_str()),
        ("expect", entry.expect.as_str()),
        ("layer", entry.layer.as_str()),
        ("unit_test", entry.unit_test.as_str()),
        ("symptom", entry.symptom.as_str()),
        ("root_cause", entry.root_cause.as_str()),
        ("fix_commit", entry.fix_commit.as_str()),
        ("finder", entry.finder.as_str()),
        ("added", entry.added.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("field `{field}` is empty"));
        }
    }
    if entry.status != "landed" && entry.status != "open" {
        return Err(format!("unknown status '{}'", entry.status));
    }
    if let Some(spec) = entry.spec.as_deref()
        && !spec.starts_with("https://")
    {
        return Err(format!("spec link must be an https URL, got `{spec}`"));
    }
    // Fourth artifact (P2 audit): every entry stages a raw-JS corpus seed
    // for the bug class under `tests/fuzz/seeds/`; P4 wires them into
    // harnesses. The path is repo-relative and the file must exist.
    match entry.fuzz_seed.as_deref().map(str::trim) {
        None | Some("") => {
            return Err(String::from(
                "fuzz_seed is required (stage the seed under tests/fuzz/seeds/)",
            ));
        }
        Some(seed) if !root.join("../../").join(seed).exists() => {
            return Err(format!("fuzz_seed `{seed}` does not exist"));
        }
        Some(_) => {}
    }

    // Test262-style test or a written justification: one of the two must
    // be present (both is fine).
    let has_link = entry
        .test262_style
        .as_deref()
        .is_some_and(|link| !link.trim().is_empty());
    let has_reason = entry
        .no_test262_reason
        .as_deref()
        .is_some_and(|reason| !reason.trim().is_empty());
    if !has_link && !has_reason {
        return Err(String::from(
            "either test262_style or no_test262_reason is required",
        ));
    }

    // The responsible-layer unit test must exist (checked as a file path;
    // `path::to::test_fn` suffixes are allowed and ignored here).
    let unit_file = entry.unit_test.split("::").next().unwrap_or("");
    if !root.join("../../").join(unit_file).exists() {
        return Err(format!("unit test file `{unit_file}` does not exist"));
    }

    // Test262 cross-links are checked when a checkout is present (silently
    // skipped otherwise: absence of the checkout is an environment fact, not
    // a schema violation worth failing — or printing — about).
    if let Some(link) = entry.test262_style.as_deref() {
        let checkout = root.join("../../test262");
        if checkout.is_dir() && !checkout.join(link).exists() {
            return Err(format!("test262_style `{link}` does not exist"));
        }
    }

    // Open entries document unlanded work; only landed entries execute.
    if entry.status == "open" {
        return Ok(());
    }

    let source = std::fs::read_to_string(root.join(&entry.reproducer))
        .map_err(|err| format!("cannot read reproducer: {err}"))?;

    let mut context = Context::default();
    register_assert_fns(&mut context);

    match context.eval(Source::from_bytes(&source)) {
        Ok(_) if entry.expect == "pass" => Ok(()),
        Ok(value) => Err(format!("expected {}, got {value:?}", entry.expect)),
        Err(err) if entry.expect == "pass" => Err(format!("unexpected throw: {err:?}")),
        Err(err) => check_thrown(&entry.expect, err, &mut context),
    }
}

/// Checks a thrown error against `throw <Name>`.
fn check_thrown(expect: &str, err: JsError, context: &mut Context) -> Result<(), String> {
    let Some(expected) = expect.strip_prefix("throw ") else {
        return Err(format!("unknown expectation `{expect}`"));
    };
    let name = thrown_name(err, context);
    if name == expected.trim() {
        Ok(())
    } else {
        Err(format!("expected throw {expected}, got `{name}`"))
    }
}

/// Best-effort `name` of a thrown error.
fn thrown_name(err: JsError, context: &mut Context) -> String {
    err.into_opaque(context)
        .ok()
        .and_then(|value| value.as_object())
        .and_then(|object| object.get(JsString::from("name"), context).ok())
        .and_then(|name| name.to_string(context).ok())
        .map(|name| name.to_std_string_escaped())
        .unwrap_or_default()
}

/// Registers the `assert` / `assertEquals` tripwires used by reproducers.
fn register_assert_fns(context: &mut Context) {
    for (name, func) in [
        ("assert", NativeFunction::from_fn_ptr(assert_fn)),
        (
            "assertEquals",
            NativeFunction::from_fn_ptr(assert_equals_fn),
        ),
    ] {
        let js_function = FunctionObjectBuilder::new(context.realm(), func)
            .name(name)
            .length(2)
            .build();
        context
            .register_global_property(JsString::from(name), js_function, Attribute::all())
            .expect("register assert fn");
    }
}

/// `assert(cond, message?)`: throws a `TypeError` unless `cond` is truthy.
fn assert_fn(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    if args.get_or_undefined(0).to_boolean() {
        return Ok(JsValue::undefined());
    }
    let message = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    Err(JsNativeError::typ()
        .with_message(format!("assertion failed: {message}"))
        .into())
}

/// `assertEquals(actual, expected, message?)` under `===` semantics.
fn assert_equals_fn(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let actual = args.get_or_undefined(0);
    let expected = args.get_or_undefined(1);
    if actual.strict_equals(expected) {
        return Ok(JsValue::undefined());
    }
    let message = args
        .get_or_undefined(2)
        .to_string(context)?
        .to_std_string_escaped();
    Err(JsNativeError::typ()
        .with_message(format!(
            "assertion failed: {message}: {actual:?} !== {expected:?}"
        ))
        .into())
}

/// Manifest-relative fixture root for unit tests.
#[cfg(test)]
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::run_entry;

    /// A minimal well-formed entry used to test the runner itself.
    fn entry(reproducer: &str, expect: &str) -> super::Entry {
        super::Entry {
            id: String::from("self-test"),
            title: String::from("self-test"),
            status: String::from("landed"),
            reproducer: reproducer.into(),
            expect: expect.into(),
            layer: String::from("test"),
            unit_test: String::from("tests/regression/src/lib.rs"),
            test262_style: None,
            no_test262_reason: Some(String::from("self-test")),
            fuzz_seed: None,
            symptom: String::from("self-test"),
            root_cause: String::from("self-test"),
            spec: None,
            fix_commit: String::from("self-test"),
            finder: String::from("self-test"),
            added: String::from("2026-10-02"),
        }
    }

    fn write_repro(root: &std::path::Path, name: &str, source: &str) -> String {
        let dir = root.join("target").join("repro-selftest");
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join(name), source).expect("write");
        format!("target/repro-selftest/{name}")
    }

    /// Writes a seed file and returns its repo-relative path.
    fn write_seed(root: &std::path::Path) -> String {
        let dir = root.join("target").join("repro-selftest");
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("seed.js"), "var x = 1;\n").expect("write");
        String::from("tests/regression/target/repro-selftest/seed.js")
    }

    fn seeded(root: &std::path::Path, reproducer: &str, expect: &str) -> super::Entry {
        let mut entry = entry(reproducer, expect);
        entry.fuzz_seed = Some(write_seed(root));
        entry
    }

    #[test]
    fn runner_accepts_pass_and_throw() {
        let root = super::fixture_root();
        let pass = write_repro(&root, "pass.js", "assertEquals(1 + 1, 2, 'math');\n");
        run_entry(&root, &seeded(&root, &pass, "pass")).expect("pass");
        let throw = write_repro(&root, "throw.js", "null.nonexistent;\n");
        run_entry(&root, &seeded(&root, &throw, "throw TypeError")).expect("throw");
    }

    #[test]
    fn runner_rejects_mismatches() {
        let root = super::fixture_root();
        let failing = write_repro(&root, "failing.js", "assert(false, 'always');\n");
        assert!(run_entry(&root, &seeded(&root, &failing, "pass")).is_err());
        let passing = write_repro(&root, "passing.js", "var x = 1;\n");
        assert!(run_entry(&root, &seeded(&root, &passing, "throw TypeError")).is_err());
        let mut missing = seeded(&root, "target/repro-selftest/nope.js", "pass");
        missing.unit_test = String::from("does/not/exist.rs");
        assert!(run_entry(&root, &missing).is_err());
    }

    #[test]
    fn runner_requires_seed_and_link_or_reason() {
        let root = super::fixture_root();
        let pass = write_repro(&root, "ok.js", "var x = 1;\n");
        // Missing seed fails even when everything else is valid.
        assert!(run_entry(&root, &entry(&pass, "pass")).is_err());
        // Dangling seed path fails.
        let mut dangling = seeded(&root, &pass, "pass");
        dangling.fuzz_seed = Some(String::from("tests/fuzz/seeds/nope.js"));
        assert!(run_entry(&root, &dangling).is_err());
        // Neither link nor reason fails.
        let mut unjustified = seeded(&root, &pass, "pass");
        unjustified.no_test262_reason = None;
        assert!(run_entry(&root, &unjustified).is_err());
    }
}
