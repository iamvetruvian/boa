#![no_main]

//! Module specifier resolution (P4.3): adversarial (specifier, base, referrer)
//! triples must resolve deterministically and never panic.

use arbitrary::Arbitrary;
use boa_fuzz::adversarial::resolve_specifier_summary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct SpecifierInput {
    specifier: String,
    base: Option<String>,
    referrer: Option<String>,
}

fn do_fuzz(input: SpecifierInput) {
    let run = || {
        resolve_specifier_summary(
            &input.specifier,
            input.base.as_deref(),
            input.referrer.as_deref(),
        )
    };
    let first = run();
    let second = run();
    assert_eq!(
        first, second,
        "nondeterministic specifier resolution.\nInput:\n{input:?}",
    );
}

fuzz_target!(|input: SpecifierInput| {
    do_fuzz(input);
});
