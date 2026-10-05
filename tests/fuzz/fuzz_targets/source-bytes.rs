#![no_main]

//! Raw-byte script parsing (P4.3): arbitrary bytes must parse deterministically
//! and never panic the parser, whatever the UTF-8 damage or nesting depth.

use boa_fuzz::adversarial::parse_bytes_summary;
use libfuzzer_sys::fuzz_target;

fn do_fuzz(bytes: &[u8]) {
    let first = parse_bytes_summary(bytes);
    let second = parse_bytes_summary(bytes);
    assert_eq!(
        first,
        second,
        "nondeterministic parse summary for {} input bytes",
        bytes.len(),
    );
}

fuzz_target!(|bytes: &[u8]| {
    do_fuzz(bytes);
});
