#![no_main]

use boa_ast::scope::Scope;
use boa_fuzz::{
    common::FuzzSource,
    mutate::{mutate, NUM_MUTATORS},
    semantic::{
        check_legs, check_metamorphic_pair, eval_oracle, eval_outcome, known_divergence,
        metamorphic_variants, oracle_agrees, outcomes_equal, reprint, ORACLE_ENV_VAR,
    },
};
use boa_interner::{Interner, ToInternedString};
use boa_parser::{Parser, Source};
use libfuzzer_sys::{fuzz_target, Corpus};
use std::io::Cursor;

fn do_fuzz(source: String) -> Corpus {
    // Fresh interner: `FuzzSource`'s is dropped on the calling thread
    // (`Interner` is `!Send`, so only the source text crosses into the
    // worker). Re-parsing the lifted text re-interns identically-named
    // symbols; all downstream uses share this interner consistently.
    let mut interner = Interner::default();

    // Parse gate (P5.2 valid-only): reject unparseable lifts so the corpus
    // concentrates on programs the VM legs can actually distinguish (parser
    // space belongs to parser-idempotency/source-bytes). Before rejecting,
    // assert error-determinism like parser-idempotency does.
    let mut parser = Parser::new(Source::from_reader(Cursor::new(&source), None));
    let scope = Scope::new_global();
    let mut script = match parser.parse_script(&scope, &mut interner) {
        Ok(script) => script,
        Err(first_err) => {
            let mut retry = Parser::new(Source::from_reader(Cursor::new(&source), None));
            let retry_scope = Scope::new_global();
            let second_err = retry
                .parse_script(&retry_scope, &mut interner)
                .expect_err("parse error must be deterministic: retry of failing input parsed");
            assert_eq!(
                format!("{first_err:?}"),
                format!("{second_err:?}"),
                "parse error must be deterministic.\nSource:\n{source}",
            );
            return Corpus::Reject;
        }
    };

    let first = check_legs(&source, "original");

    // Idempotency: the reprinted program must evaluate identically.
    if let Ok(printed) = reprint(&source) {
        let round_tripped = eval_outcome(&printed);
        assert!(
            outcomes_equal(&first, &round_tripped),
            "reprint changed semantics.\nSource:\n{source}\nReprint:\n{printed}\nFirst:\n{first:?}\nRound-tripped:\n{round_tripped:?}",
        );
    }

    // Metamorphic variants (P5.3): each must evaluate identically to the
    // original (order matters: `check_metamorphic_pair` is sound only
    // after `check_legs` ran on the same original — see its docs).
    for variant in metamorphic_variants(&source) {
        check_metamorphic_pair(&source, &first, &variant);
    }

    // Structured mutant (P5.2): one deterministic AST edit, re-lifted and
    // re-parsed; mutants that fail the re-parse are silently skipped (they
    // are derived inputs, not corpus signal). A surviving mutant runs the
    // full legs — it is a new program, so legs must agree with each other.
    let selector = source.len();
    if mutate(
        script.statements_mut(),
        &mut interner,
        selector % NUM_MUTATORS,
        selector / NUM_MUTATORS,
    ) {
        let mutant = script.to_interned_string(&interner);
        let mut reparser = Parser::new(Source::from_reader(Cursor::new(&mutant), None));
        let rescope = Scope::new_global();
        if reparser.parse_script(&rescope, &mut interner).is_ok() {
            check_legs(&mutant, "mutant");
        }
    }

    // Oracle differential (env-gated; skips fuel/timeout/unclassified
    // pairs and known open-bug shapes so the loop reaches new bugs).
    if let Ok(oracle_path) = std::env::var(ORACLE_ENV_VAR) {
        let oracle = eval_oracle(&oracle_path, &source);
        if known_divergence(&first, &oracle).is_none() {
            if let Some(agree) = oracle_agrees(&first, &oracle) {
                assert!(
                    agree,
                    "oracle divergence.\nSource:\n{source}\nBoa:\n{first:?}\nOracle:\n{oracle:?}",
                );
            }
        }
    }

    Corpus::Keep
}

/// Worker-thread stack: 64 MB. The parser's 256 KB red-zone adapts accepted
/// depth to remaining stack (~10 levels on a 2 MB test thread, ~212 on the
/// 8 MB main thread, both measured), so running legs on the libFuzzer thread
/// would let 2×-depth paren-add variants trip the red-zone their originals
/// pass — a false divergence (P5.3 gate finding). 64 MB puts the cliff at
/// absurdity while `MAX_DERIVED_NESTING` (256) keeps every derived input far
/// below it; as a bonus, deep originals (rejected past ~212 before) now
/// parse and run legs instead of being discarded.
const WORKER_STACK: usize = 64 << 20;

fuzz_target!(|original: FuzzSource| -> Corpus {
    // Only the source text crosses into the worker (`Interner` is `!Send`);
    // panics resume on the calling thread with their original payload, so
    // libFuzzer still records the crash with the true assertion text.
    // Spawn failure (resource exhaustion, not input signal) rejects
    // without failing the run.
    let source = original.source;
    match std::thread::Builder::new()
        .name(String::from("vm-implied-worker"))
        .stack_size(WORKER_STACK)
        .spawn(move || do_fuzz(source))
    {
        Ok(handle) => match handle.join() {
            Ok(corpus) => corpus,
            Err(payload) => std::panic::resume_unwind(payload),
        },
        Err(_) => Corpus::Reject,
    }
});
