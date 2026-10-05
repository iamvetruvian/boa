//! Pre-parse name-universe checking for the parser-idempotency oracle.
//!
//! The oracle's subset property: every name referenced by a reparse must
//! already exist in the pre-parse universe. A printer morph (e.g. a `NaN`
//! literal printed as the `NaN` identifier) surfaces as a referenced-but-
//! unknown [`Sym`].
//!
//! One principled exception encodes a spec rule: private-field
//! `NamedEvaluation` synthesizes `"#" + description` as the initializer's
//! function name (`class C { static #f = function () {}; }` reparses with
//! function name `"#f"`), so `"#x"` is allowed when `x` is already in the
//! universe.

use boa_ast::{
    visitor::{VisitWith, Visitor},
    StatementList,
};
use boa_interner::{Interner, Sym};
use std::{collections::HashSet, convert::Infallible, ops::ControlFlow};

/// Collect every [`Sym`] referenced anywhere in a statement list.
pub fn syms_of(statements: &StatementList) -> HashSet<Sym> {
    struct Collector(HashSet<Sym>);
    impl<'ast> Visitor<'ast> for Collector {
        type BreakTy = Infallible;
        fn visit_sym(&mut self, node: &'ast Sym) -> ControlFlow<Self::BreakTy> {
            self.0.insert(*node);
            ControlFlow::Continue(())
        }
    }
    let mut collector = Collector(HashSet::new());
    let _: ControlFlow<Infallible> = statements.visit_with(&mut collector);
    collector.0
}

/// Names referenced by `after` that are outside `before`'s universe.
///
/// Each entry renders as `Sym { value }="text"` for diagnostics. `"#x"`
/// entries are allowed when `x` is already referenced by `before` (private-
/// field `NamedEvaluation` synthesis); everything else unknown is reported.
pub fn names_outside_universe(
    before: &StatementList,
    after: &StatementList,
    interner: &Interner,
) -> Vec<String> {
    names_outside_universe_set(&syms_of(before), after, interner)
}

/// [`names_outside_universe`] against a precomputed (possibly running)
/// universe set.
pub fn names_outside_universe_set(
    universe: &HashSet<Sym>,
    after: &StatementList,
    interner: &Interner,
) -> Vec<String> {
    let after_syms = syms_of(after);
    let mut before_texts = HashSet::new();
    for sym in universe {
        if let Some(text) = interner.resolve_expect(*sym).utf8() {
            before_texts.insert(text.to_owned());
        }
    }
    let mut unknown = Vec::new();
    for sym in after_syms.difference(&universe) {
        // `None` (non-UTF-8) is unreachable from UTF-8 source, but report
        // it rather than silently allowing it.
        let text = interner.resolve_expect(*sym).utf8().unwrap_or("<non-utf8>");
        if let Some(rest) = text.strip_prefix('#') {
            if before_texts.contains(rest) {
                continue;
            }
        }
        unknown.push(format!("{sym:?}={text:?}"));
    }
    unknown.sort();
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use boa_ast::scope::Scope;
    use boa_parser::{Parser, Source};

    fn parse(source: &str, interner: &mut Interner) -> StatementList {
        Parser::new(Source::from_bytes(source))
            .parse_script(&Scope::new_global(), interner)
            .unwrap_or_else(|_| panic!("test source must parse: {source}"))
            .statements()
            .clone()
    }

    #[test]
    fn identical_asts_have_no_unknown_names() {
        let mut interner = Interner::default();
        let before = parse("let a = 1; a + 2;", &mut interner);
        let after = parse("let a = 1; a + 2;", &mut interner);
        assert!(names_outside_universe(&before, &after, &interner).is_empty());
    }

    #[test]
    fn morphed_identifier_is_reported() {
        let mut interner = Interner::default();
        let before = parse("let a = 1;", &mut interner);
        let after = parse("NaN;", &mut interner);
        let unknown = names_outside_universe(&before, &after, &interner);
        assert_eq!(unknown.len(), 1, "unknown: {unknown:?}");
        assert!(unknown[0].contains("NaN"), "unknown: {unknown:?}");
    }

    #[test]
    fn private_field_function_name_synthesis_allowed() {
        let mut interner = Interner::default();
        // `before` references `f` (as a private description); `after` is
        // what the parser produces for a function initializer: the
        // spec-mandated `"#f"` function name.
        let before = parse("class C { static #f = 0; }", &mut interner);
        let after = parse("class C { static #f = function () {}; }", &mut interner);
        assert!(
            names_outside_universe(&before, &after, &interner).is_empty(),
            "spec-rule #f synthesis must be allowed"
        );
    }

    #[test]
    fn hash_name_without_private_description_reported() {
        let mut interner = Interner::default();
        let before = parse("let a = 1;", &mut interner);
        let after = parse("class C { static #f = function () {}; }", &mut interner);
        let unknown = names_outside_universe(&before, &after, &interner);
        assert!(
            unknown.iter().any(|entry| entry.contains("#f")),
            "unsanctioned #f must be reported: {unknown:?}"
        );
    }
}
