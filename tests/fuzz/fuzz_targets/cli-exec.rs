#![no_main]

//! Full CLI host-context execution (P4.3): arbitrary bytes, run as a script in
//! the complete host environment (console/fetch/URL/...), must evaluate
//! deterministically and never panic or hang (fuelled).

use boa_fuzz::{
    adversarial::host_context,
    semantic::{eval_outcome_in, eval_outcome_stressed_in, outcomes_equal, should_stress},
};
use boa_gc::gc_total_allocs;
use libfuzzer_sys::fuzz_target;

fn do_fuzz(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    // Normal-vs-stress differential in the full host context (arbitrary
    // bytes can spell `WeakRef`, so the timing filter is load-bearing here).
    // The churn budget measures the eval only: host-context construction
    // (extensions + the time-freeze eval) is excluded.
    let mut first_context = host_context();
    let allocs_before = gc_total_allocs();
    let first = eval_outcome_in(&mut first_context, &text);
    let stressed = should_stress(&text, gc_total_allocs() - allocs_before);
    let second = if stressed {
        eval_outcome_stressed_in(&mut host_context(), &text)
    } else {
        eval_outcome_in(&mut host_context(), &text)
    };
    let leg = if stressed { "gc-stress" } else { "second" };
    assert!(
        outcomes_equal(&first, &second),
        "nondeterministic host eval ({leg} leg).\nFirst:\n{first:?}\nSecond:\n{second:?}",
    );
}

fuzz_target!(|bytes: &[u8]| {
    do_fuzz(bytes);
});
