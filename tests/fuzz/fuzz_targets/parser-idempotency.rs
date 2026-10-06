#![no_main]

use boa_ast::scope::Scope;
use boa_fuzz::{
    canonicalize::canonicalize_for_idempotency,
    common::FuzzData,
    universe::{names_outside_universe_set, syms_of},
};
use boa_interner::ToInternedString;
use boa_parser::{Parser, Source};
use libfuzzer_sys::{fuzz_target, Corpus};
use std::{error::Error, io::Cursor};

/// Fuzzer test harness. This function accepts the arbitrary AST and performs the fuzzing operation.
///
/// See [README.md](../README.md) for details on the design of this fuzzer.
fn do_fuzz(mut data: FuzzData) -> Result<(), Box<dyn Error>> {
    canonicalize_for_idempotency(&mut data.ast, &mut data.interner);

    let original = data.ast.to_interned_string(&data.interner);

    let mut parser = Parser::new(Source::from_reader(Cursor::new(&original), None));
    let scope = Scope::new_global();
    // For a variety of reasons, we may not actually produce valid code here (e.g., nameless function).
    // Fail fast and only make the next checks if we were valid; signal rejection
    // so libFuzzer does not retain unparsable inputs in the corpus.
    // Before rejecting, assert error-determinism: the same bytes must fail identically twice.
    let first = match parser.parse_script(&scope, &mut data.interner) {
        Ok(first) => first,
        Err(first_err) => {
            let mut retry = Parser::new(Source::from_reader(Cursor::new(&original), None));
            let retry_scope = Scope::new_global();
            let second_err = retry
                .parse_script(&retry_scope, &mut data.interner)
                .expect_err("parse error must be deterministic: retry of failing input parsed");
            assert_eq!(
                format!("{first_err:?}"),
                format!("{second_err:?}"),
                "parse error must be deterministic.\nOriginal:\n{original}",
            );
            return Err(Box::new(first_err));
        }
    };
    {
        let first_interned = first.to_interned_string(&data.interner);

        // Subset oracle (see `boa_fuzz::universe`): every name referenced
        // by a reparse must already exist in the pre-parse universe, with
        // the spec-rule exception for private-field `"#x"` synthesis.
        let mut universe = syms_of(&data.ast);
        let unknown = names_outside_universe_set(&universe, first.statements(), &data.interner);
        assert!(
            unknown.is_empty(),
            "reparse referenced {} name(s) outside the pre-parse universe: {}\nBefore:\n{}\nAfter:\n{}",
            unknown.len(),
            unknown.join(", "),
            original,
            first_interned,
        );
        universe.extend(syms_of(first.statements()));
        let mut parser = Parser::new(Source::from_reader(Cursor::new(&first_interned), None));
        let second_scope = Scope::new_global();

        // Now, we most assuredly should produce valid code. It has already gone through a first pass.
        let second = parser
            .parse_script(&second_scope, &mut data.interner)
            .expect("Could not parse the first-pass interned copy.");
        let second_interned = second.to_interned_string(&data.interner);
        let unknown_second =
            names_outside_universe_set(&universe, second.statements(), &data.interner);
        assert!(
            unknown_second.is_empty(),
            "second reparse referenced {} name(s) outside the universe: {}\nFirst:\n{}\nSecond:\n{}",
            unknown_second.len(),
            unknown_second.join(", "),
            first_interned,
            second_interned,
        );
        // Idempotency is a print fixpoint, NOT AST equality: spans are layout
        // metadata and the printer normalizes whitespace by design (e.g. `-x`
        // prints as `- x`), so first/second ASTs always differ in spans.
        // Assert the strings reach a fixpoint, then confirm with a third pass
        // (catches A -> B -> A oscillation, which a two-pass check would miss).
        assert_eq!(
            first_interned,
            second_interned,
            "Expected print to reach a fixpoint after two passes.\nOriginal:\n{}\nFirst:\n{}\nSecond:\n{}",
            original,
            first_interned,
            second_interned,
        );
        let mut third_parser =
            Parser::new(Source::from_reader(Cursor::new(&second_interned), None));
        let third_scope = Scope::new_global();
        let third = third_parser
            .parse_script(&third_scope, &mut data.interner)
            .expect("Could not parse the second-pass interned copy.");
        let third_interned = third.to_interned_string(&data.interner);
        assert_eq!(
            second_interned, third_interned,
            "Print fixpoint oscillated on the third pass.\nSecond:\n{}\nThird:\n{}",
            second_interned, third_interned,
        );
    }
    Ok(())
}

// Fuzz harness wrapper to expose it to libfuzzer (and thus cargo-fuzz)
// See: https://rust-fuzz.github.io/book/cargo-fuzz.html
fuzz_target!(|data: FuzzData| -> Corpus {
    if do_fuzz(data).is_ok() {
        Corpus::Keep
    } else {
        Corpus::Reject
    }
});
