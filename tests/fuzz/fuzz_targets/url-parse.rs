#![no_main]

//! URL parsing (P4.3): the native `Url` constructor must parse deterministically
//! and never panic, and the JS-level `URL` / `URLSearchParams` constructors
//! must evaluate deterministically in the host context.

use arbitrary::Arbitrary;
use boa_fuzz::{
    adversarial::{host_context, js_string_literal, url_parse_summary},
    semantic::{eval_outcome_in, outcomes_equal},
};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct UrlInput {
    url: String,
    base: Option<String>,
    params: String,
}

fn do_fuzz(input: UrlInput) {
    // Native constructor: deterministic summary, no panic.
    let first = url_parse_summary(&input.url, input.base.as_deref());
    let second = url_parse_summary(&input.url, input.base.as_deref());
    assert_eq!(
        first, second,
        "nondeterministic URL parse.\nInput:\n{input:?}",
    );

    // JS-level constructors in the host context: determinism double-eval.
    let url_src = match &input.base {
        Some(base) => format!(
            "new URL({}, {})",
            js_string_literal(&input.url),
            js_string_literal(base)
        ),
        None => format!("new URL({})", js_string_literal(&input.url)),
    };
    let params_src = format!("new URLSearchParams({})", js_string_literal(&input.params));
    for src in [url_src, params_src] {
        let first = eval_outcome_in(&mut host_context(), &src);
        let second = eval_outcome_in(&mut host_context(), &src);
        assert!(
            outcomes_equal(&first, &second),
            "nondeterministic URL eval.\nSource:\n{src}\nFirst:\n{first:?}\nSecond:\n{second:?}",
        );
    }
}

fuzz_target!(|input: UrlInput| {
    do_fuzz(input);
});
