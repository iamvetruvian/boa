//! Shared adversarial host-boundary harnesses (P4.3).
//!
//! These helpers drive `boa_runtime` entry points (`URL`, `Response`, fetch)
//! and host-adjacent APIs (module specifier resolution, raw-byte parsing, the
//! full CLI host context) with adversarial inputs. The fetch harness encodes
//! the WHATWG fetch "initialize a response" validation order as executable
//! oracle rules so any divergence between the engine and the spec crashes the
//! fuzzer instead of passing silently.

use crate::semantic::{error_class_name, stress_point, EVAL_FUEL};
use boa_ast::scope::Scope;
use boa_engine::{Context, Source};
use boa_interner::Interner;
use boa_parser::Parser;
use boa_runtime::{
    extensions::{ConsoleExtension, FetchExtension},
    fetch::ErrorFetcher,
    NullLogger,
};

/// Frozen wall-clock time observed through the fuzz host's `Date`
/// (`Date.now()`, `new Date()`, `Date()` with no arguments).
pub const FROZEN_EPOCH_MILLIS: i64 = 1_720_000_000_000;

/// Frozen `Math.random()` value observed through the fuzz host.
pub const FROZEN_RANDOM: f64 = 0.5;

/// Build a full host context mirroring `boa_cli` (console + fetch + the
/// always-on extensions, including `URL`), except the console logger is
/// [`NullLogger`] (adversarial `console.log` must not spam fuzzer stderr)
/// and the fetch backend is [`ErrorFetcher`] (offline) instead of reqwest:
/// the fuzzer must never touch the network. Fuelled like every other fuzz
/// context so adversarial programs cannot hang the loop.
///
/// Wall-clock and entropy sources (`Date`, `Math.random`) are frozen to
/// constants: without this, time-observing adversarial programs would
/// diverge between determinism legs on every run, and any *remaining*
/// divergence is therefore an engine bug by construction. Timezone- and
/// locale-dependent renderings stay live (leg-consistent within a run).
pub fn host_context() -> Context {
    let mut context = Context::builder()
        .instructions_remaining(EVAL_FUEL)
        .build()
        .expect("fuzz host context must build");
    boa_runtime::register(
        (ConsoleExtension(NullLogger), FetchExtension(ErrorFetcher)),
        None,
        &mut context,
    )
    .expect("host extensions register");
    context
        .eval(Source::from_bytes(
            format!(
                "Date = new Proxy(Date, {{ \
                    construct(t, a) {{ return new t(...(a.length ? a : [{FROZEN_EPOCH_MILLIS}])); }}, \
                    apply(t) {{ return new t({FROZEN_EPOCH_MILLIS}).toString(); }}, \
                    get(t, p, r) {{ return p === \"now\" ? () => {FROZEN_EPOCH_MILLIS} : Reflect.get(t, p, r); }}, \
                }}); \
                Math.random = () => {FROZEN_RANDOM};"
            )
            .as_bytes(),
        ))
        .expect("host time/entropy freeze must evaluate");
    stress_point(&mut context);
    context
}

/// Escape `text` as a double-quoted JavaScript string literal.
pub fn js_string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                write!(out, "\\u{:04x}", c as u32).expect("write to String");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Mirror of the fetch reason-phrase check
/// (`core/runtime/src/fetch/response.rs`): `*( HTAB / SP / VCHAR / obs-text )`.
#[must_use]
pub fn is_valid_reason_phrase(text: &str) -> bool {
    text.bytes()
        .all(|b| matches!(b, 0x09 | 0x20 | 0x21..=0x7E | 0x80..=0xFF))
}

/// Null body statuses per the fetch spec (101, 103, 204, 205, 304).
#[must_use]
pub fn is_null_body_status(status: u16) -> bool {
    matches!(status, 101 | 103 | 204 | 205 | 304)
}

/// A synthesized `new Response(body, init)` case. Headers are valid by
/// construction (token names, trimmable values); invalid-header rejection is
/// `http`-crate behavior, not Boa behavior, and is out of scope.
#[derive(Debug, Clone)]
pub struct FetchCase {
    /// Response body, if any (always a JS string when present).
    pub body: Option<String>,
    /// `init.status` (full `u16` range, adversarial).
    pub status: u16,
    /// `init.statusText` (arbitrary Unicode, adversarial).
    pub status_text: String,
    /// Valid header pairs.
    pub headers: Vec<(String, String)>,
}

/// Expected completion of a [`FetchCase`], following the check order of
/// `initialize_response`: status range first, then statusText, then the
/// null-body-status rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchExpectation {
    /// Constructor completes normally.
    Completed,
    /// `RangeError` (status outside 200..=599).
    RangeError,
    /// `TypeError` (bad statusText, or body with a null-body status).
    TypeError,
}

/// Compute the expected outcome of a [`FetchCase`].
#[must_use]
pub fn fetch_case_expectation(case: &FetchCase) -> FetchExpectation {
    if !(200..=599).contains(&case.status) {
        return FetchExpectation::RangeError;
    }
    if !is_valid_reason_phrase(&case.status_text) {
        return FetchExpectation::TypeError;
    }
    if case.body.is_some() && is_null_body_status(case.status) {
        return FetchExpectation::TypeError;
    }
    FetchExpectation::Completed
}

/// Valid header names the fetch target samples from (all HTTP tokens).
pub const HEADER_NAME_POOL: &[&str] = &[
    "x-a",
    "content-type",
    "x-custom-header",
    "etag",
    "accept",
    "a",
];

/// Valid header values the fetch target samples from (valid after the
/// engine's OWS trim; the empty value is included deliberately).
pub const HEADER_VALUE_POOL: &[&str] = &[
    "1",
    "hello world",
    "  padded  ",
    "",
    "caf\u{e9}",
    "\tindented",
    "a,b",
    "text/plain",
];

/// Build a [`FetchCase`] from fuzzer parts, sampling headers from the valid
/// pools by index (duplicates exercise append-vs-insert combining).
#[must_use]
pub fn fetch_case_from_parts(
    body: Option<String>,
    status: u16,
    status_text: String,
    picks: &[(u8, u8)],
) -> FetchCase {
    FetchCase {
        body,
        status,
        status_text,
        headers: picks
            .iter()
            .map(|(n, v)| {
                (
                    HEADER_NAME_POOL[*n as usize % HEADER_NAME_POOL.len()].to_owned(),
                    HEADER_VALUE_POOL[*v as usize % HEADER_VALUE_POOL.len()].to_owned(),
                )
            })
            .collect(),
    }
}

/// Render a [`FetchCase`] as a `new Response(...)` source string.
#[must_use]
pub fn fetch_case_source(case: &FetchCase) -> String {
    let mut out = String::from("new Response(");
    match &case.body {
        Some(body) => out.push_str(&js_string_literal(body)),
        None => out.push_str("undefined"),
    }
    out.push_str(", { status: ");
    out.push_str(&case.status.to_string());
    out.push_str(", statusText: ");
    out.push_str(&js_string_literal(&case.status_text));
    out.push_str(", headers: { ");
    for (i, (name, value)) in case.headers.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&js_string_literal(name));
        out.push_str(": ");
        out.push_str(&js_string_literal(value));
    }
    out.push_str(" } })");
    out
}

/// Parse arbitrary bytes as a script (lossy UTF-8, like the CLI). Returns a
/// stable summary: `Ok` for parse success, `Err` with the error shape for
/// parse failure. Never panics.
pub fn parse_bytes_summary(bytes: &[u8]) -> Result<&'static str, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut interner = Interner::default();
    match Parser::new(Source::from_bytes(text.as_bytes()))
        .parse_script(&Scope::new_global(), &mut interner)
    {
        Ok(_) => Ok("parsed"),
        Err(err) => Err(format!("{err:?}")),
    }
}

/// Resolve a module specifier against a base directory and referrer,
/// mirroring [`boa_engine::module::resolve_module_specifier`]. Returns a
/// stable summary string. Never panics.
pub fn resolve_specifier_summary(
    specifier: &str,
    base: Option<&str>,
    referrer: Option<&str>,
) -> String {
    use std::cell::RefCell;
    // `resolve_module_specifier` ignores its context (`_context`), so one
    // thread-local context serves all calls: building a fresh realm per
    // input would dominate this target's runtime (~13 ms each in dev).
    thread_local! {
        static RESOLVE_CONTEXT: RefCell<Context> = RefCell::new(Context::default());
    }
    let spec = boa_engine::JsString::from(specifier);
    let base_owned = base.map(std::path::PathBuf::from);
    let referrer_owned = referrer.map(std::path::PathBuf::from);
    RESOLVE_CONTEXT.with_borrow_mut(|context| {
        match boa_engine::module::resolve_module_specifier(
            base_owned.as_deref(),
            &spec,
            referrer_owned.as_deref(),
            context,
        ) {
            Ok(path) => format!("ok:{}", path.display()),
            Err(err) => format!("err:{}", error_class_name(&err)),
        }
    })
}

/// Parse a URL against an optional base through the native [`boa_runtime`]
/// constructor. Returns a stable summary: the serialized URL on success, the
/// error class on failure. Never panics.
pub fn url_parse_summary(url: &str, base: Option<&str>) -> String {
    use boa_engine::value::Convert;

    match boa_runtime::url::Url::new(
        Convert::from(url.to_owned()),
        base.map(|b| Convert::from(b.to_owned())),
    ) {
        Ok(parsed) => format!("ok:{parsed}"),
        Err(err) => format!("err:{}", error_class_name(&err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{eval_outcome_in, outcomes_equal};

    #[test]
    fn escaper_handles_nasty_strings() {
        // Quotes, backslashes, controls, NUL, line/paragraph separators.
        let nasty = "a\"b\\c\x00\x01\n\r\t\u{2028}\u{2029}é𝄞";
        let lit = js_string_literal(nasty);
        // The literal must round-trip through the Boa parser and evaluate
        // back to the original string. (`Context::default()` carries no
        // instruction fuel, so eval needs an explicitly fuelled context.)
        let context = &mut Context::builder()
            .instructions_remaining(10_000)
            .build()
            .expect("test context must build");
        let value = context
            .eval(Source::from_bytes(lit.as_bytes()))
            .expect("escaped literal parses");
        let back = value
            .to_string(context)
            .expect("to string")
            .to_std_string_escaped();
        assert_eq!(back, nasty);
    }

    #[test]
    fn reason_phrase_mirror_basics() {
        assert!(is_valid_reason_phrase("OK"));
        assert!(is_valid_reason_phrase(""));
        assert!(is_valid_reason_phrase("Not Found\t "));
        assert!(is_valid_reason_phrase("café — π")); // obs-text allowed
        assert!(!is_valid_reason_phrase("a\nb")); // LF rejected
        assert!(!is_valid_reason_phrase("a\rb")); // CR rejected
        assert!(!is_valid_reason_phrase("\0")); // NUL rejected
        assert!(!is_valid_reason_phrase("a\x7Fb")); // DEL rejected
    }

    #[test]
    fn fetch_expectation_check_order() {
        // Status range wins over everything.
        let case = FetchCase {
            body: Some("x".into()),
            status: 99,
            status_text: "bad\ntext".into(),
            headers: vec![],
        };
        assert_eq!(fetch_case_expectation(&case), FetchExpectation::RangeError);
        // statusText wins over the body rule.
        let case = FetchCase {
            body: Some("x".into()),
            status: 204,
            status_text: "bad\ntext".into(),
            headers: vec![],
        };
        assert_eq!(fetch_case_expectation(&case), FetchExpectation::TypeError);
        // Body + null-body status (empty string is still a body: non-null).
        let case = FetchCase {
            body: Some(String::new()),
            status: 204,
            status_text: "fine".into(),
            headers: vec![],
        };
        assert_eq!(fetch_case_expectation(&case), FetchExpectation::TypeError);
        // No body + null-body status is fine.
        let case = FetchCase {
            body: None,
            status: 304,
            status_text: String::new(),
            headers: vec![],
        };
        assert_eq!(fetch_case_expectation(&case), FetchExpectation::Completed);
    }

    #[test]
    fn fetch_source_evaluates_to_expectation() {
        let cases = [
            (
                FetchCase {
                    body: Some("hello".into()),
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![("x-a".into(), "1".into())],
                },
                FetchExpectation::Completed,
            ),
            (
                FetchCase {
                    body: None,
                    status: 700,
                    status_text: String::new(),
                    headers: vec![],
                },
                FetchExpectation::RangeError,
            ),
            (
                FetchCase {
                    body: None,
                    status: 200,
                    status_text: "a\nb".into(),
                    headers: vec![],
                },
                FetchExpectation::TypeError,
            ),
            (
                FetchCase {
                    body: Some(String::new()),
                    status: 204,
                    status_text: "No Content".into(),
                    headers: vec![],
                },
                FetchExpectation::TypeError,
            ),
        ];
        for (case, expected) in cases {
            assert_eq!(fetch_case_expectation(&case), expected);
            let src = fetch_case_source(&case);
            let outcome = eval_outcome_in(&mut host_context(), &src);
            let class = outcome.error_class.as_deref();
            match expected {
                FetchExpectation::Completed => {
                    assert!(outcome.completed, "source failed: {src}: {outcome:?}");
                }
                FetchExpectation::RangeError => {
                    assert_eq!(class, Some("RangeError"), "source: {src}")
                }
                FetchExpectation::TypeError => {
                    assert_eq!(class, Some("TypeError"), "source: {src}")
                }
            }
        }
    }

    #[test]
    fn host_context_registers_url_and_response() {
        let context = &mut host_context();
        for src in [
            "typeof URL",
            "typeof Response",
            "typeof fetch",
            "new URL('https://example.com/').host",
        ] {
            let outcome = eval_outcome_in(context, src);
            assert!(
                outcome.completed && !outcome.primitive.is_empty(),
                "host source failed: {src}: {outcome:?}"
            );
        }
    }

    #[test]
    fn parse_bytes_is_deterministic() {
        // NOTE: deep-nesting inputs are bug #22 (parser stack overflow);
        // they stay out of unit fixtures (2 MB test stacks) until the depth
        // limit lands. The `source-bytes` target owns depth adversarially.
        let inputs: [&[u8]; 4] = [b"1 + 1", b"let x = ;", b"\xff\xfe\x00invalid", &[b'('; 20]];
        for input in inputs {
            let first = parse_bytes_summary(input);
            let second = parse_bytes_summary(input);
            assert_eq!(first, second, "nondeterministic parse summary");
        }
        assert_eq!(parse_bytes_summary(b"1 + 1"), Ok("parsed"));
    }

    #[test]
    fn resolve_specifier_fixtures() {
        // Docstring case: relative spec against base + referrer resolves.
        let a = resolve_specifier_summary("../a.js", Some("/base"), Some("/base/hello/ref.js"));
        assert!(a.starts_with("ok:"), "unexpected: {a}");
        assert_eq!(
            a,
            resolve_specifier_summary("../a.js", Some("/base"), Some("/base/hello/ref.js"))
        );
        // Relative spec without a referrer is a TypeError by rule.
        let no_ref = resolve_specifier_summary("./x.mjs", Some("/tmp"), None);
        assert_eq!(no_ref, "err:TypeError");
        // Nonsense specifier: stable summary, no panic.
        let b = resolve_specifier_summary("\0\n:", None, None);
        assert_eq!(b, resolve_specifier_summary("\0\n:", None, None));
    }

    #[test]
    fn url_parse_fixtures() {
        let ok = url_parse_summary("https://example.com/a", None);
        assert!(ok.starts_with("ok:"), "unexpected: {ok}");
        assert_eq!(ok, url_parse_summary("https://example.com/a", None));
        let bad = url_parse_summary("::::", None);
        assert!(bad.starts_with("err:"), "unexpected: {bad}");
        assert_eq!(bad, url_parse_summary("::::", None));
        // Determinism across legs for an adversarial pair.
        let pair = url_parse_summary("\u{2028}http://[::1", Some("http://b/"));
        assert_eq!(
            pair,
            url_parse_summary("\u{2028}http://[::1", Some("http://b/"))
        );
    }

    #[test]
    fn header_pools_are_all_valid() {
        // Every pool pair must fill without error under a neutral status:
        // if this fails, the pool (not the engine) is wrong.
        let context = &mut host_context();
        for name in HEADER_NAME_POOL {
            for value in HEADER_VALUE_POOL {
                let case = FetchCase {
                    body: None,
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![((*name).to_owned(), (*value).to_owned())],
                };
                let src = fetch_case_source(&case);
                let outcome = eval_outcome_in(context, &src);
                assert!(
                    outcome.completed,
                    "pool pair rejected: {name:?}={value:?}: {outcome:?}"
                );
            }
        }
    }

    #[test]
    fn host_freezes_time_and_entropy() {
        // The freeze script must hold: every wall-clock/entropy read sees
        // the constants, across fresh contexts.
        for _ in 0..2 {
            let context = &mut host_context();
            for (src, expected) in [
                ("Date.now()", "1720000000000"),
                ("new Date().getTime()", "1720000000000"),
                ("Date.parse('2024-01-01T00:00:00Z')", "1704067200000"),
                ("Math.random()", "0.5"),
                ("new Date(0).getTime()", "0"),
            ] {
                let outcome = eval_outcome_in(context, src);
                assert!(
                    outcome.completed && outcome.primitive.contains(expected),
                    "freeze failed for {src}: {outcome:?}"
                );
            }
            // `Date()` (called) renders the frozen instant deterministically.
            let first = eval_outcome_in(context, "Date()");
            let second = eval_outcome_in(&mut host_context(), "Date()");
            assert!(
                outcomes_equal(&first, &second),
                "Date() diverged: {first:?} vs {second:?}"
            );
        }
    }

    #[test]
    fn host_double_eval_agrees() {
        // The determinism property the host targets assert, on fixtures
        // (including time-observing and timer/microtask shapes, which the
        // host freeze keeps leg-consistent).
        for src in [
            "1 + 1",
            "new URL('https://a.bc/').href",
            "typeof fetch",
            "Date.now()",
            "Math.random()",
            "new Date().toString()",
            "setTimeout(() => 99, 5); queueMicrotask(() => 98); 42",
        ] {
            let first = eval_outcome_in(&mut host_context(), src);
            let second = eval_outcome_in(&mut host_context(), src);
            assert!(
                outcomes_equal(&first, &second),
                "host nondeterminism on {src}: {first:?} vs {second:?}"
            );
        }
    }
}
