#![no_main]

use boa_fuzz::{
    common::FuzzSource,
    semantic::{compile_outcome, compile_outcome_stressed},
};
use libfuzzer_sys::{fuzz_target, Corpus};

fn do_fuzz(original: FuzzSource) -> Corpus {
    let source = &original.source;

    // Compile twice in fresh contexts: success/failure and the full
    // disassembly must match (compile determinism), with the second leg
    // under GC stress (compilation allocates heavily without running user
    // code, so no timing filter applies). The P5 verifier hook runs inside
    // `compile_outcome` on every successful compile.
    let first = compile_outcome(source);
    let second = compile_outcome_stressed(source);
    assert_eq!(
        first, second,
        "nondeterministic compile.\nSource:\n{source}\nFirst:\n{first:?}\nSecond:\n{second:?}",
    );

    if first.compiled {
        Corpus::Keep
    } else {
        Corpus::Reject
    }
}

fuzz_target!(|original: FuzzSource| -> Corpus { do_fuzz(original) });
