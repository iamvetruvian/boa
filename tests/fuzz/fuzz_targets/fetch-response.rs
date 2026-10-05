#![no_main]

//! `Response` construction (P4.3): adversarial (body, status, statusText)
//! triples must follow the fetch "initialize a response" validation order
//! exactly (status range → statusText → null-body rule), and evaluate
//! deterministically in the host context.

use arbitrary::Arbitrary;
use boa_fuzz::{
    adversarial::{
        fetch_case_expectation, fetch_case_from_parts, fetch_case_source, host_context,
        FetchExpectation,
    },
    semantic::{eval_outcome_in, outcomes_equal},
};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct FetchInput {
    body: Option<String>,
    status: u16,
    status_text: String,
    header_picks: Vec<(u8, u8)>,
}

fn do_fuzz(input: FetchInput) {
    let case = fetch_case_from_parts(
        input.body,
        input.status,
        input.status_text,
        &input.header_picks,
    );
    let expected = fetch_case_expectation(&case);
    let src = fetch_case_source(&case);

    // Determinism: two fresh host evals must agree.
    let first = eval_outcome_in(&mut host_context(), &src);
    let second = eval_outcome_in(&mut host_context(), &src);
    assert!(
        outcomes_equal(&first, &second),
        "nondeterministic Response eval.\nSource:\n{src}\nFirst:\n{first:?}\nSecond:\n{second:?}",
    );

    // Spec-rule oracle: the completion must match the predicted class.
    match expected {
        FetchExpectation::Completed => assert!(
            first.completed,
            "Response should complete.\nSource:\n{src}\nOutcome:\n{first:?}",
        ),
        FetchExpectation::RangeError => assert_eq!(
            first.error_class.as_deref(),
            Some("RangeError"),
            "Response should throw RangeError.\nSource:\n{src}",
        ),
        FetchExpectation::TypeError => assert_eq!(
            first.error_class.as_deref(),
            Some("TypeError"),
            "Response should throw TypeError.\nSource:\n{src}",
        ),
    }
}

fuzz_target!(|input: FetchInput| {
    do_fuzz(input);
});
