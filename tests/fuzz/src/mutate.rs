//! Structured AST mutations for valid-by-construction VM differentials (P5.2).
//!
//! Byte-level libFuzzer mutations rarely produce *structured* program edits.
//! These [`VisitorMut`] passes apply exactly one such edit (operator swap,
//! branch flip, literal tweak) at a deterministic site, so the caller can
//! lift the mutant back to source and run the standard differential legs on
//! it. Mutations are validity-*biased* — same-arity operator swaps, total
//! literal maps, condition-only negation — but not validity-guaranteed: the
//! caller must always re-parse (parse-gate) before trusting a mutant.
//!
//! This is deliberately *not* the P5.3 metamorphic suite: these mutants are
//! new programs (legs must agree *with each other* on the mutant), while
//! P5.3 transforms are semantics-preserving (the variant must agree *with
//! the original*).

use boa_ast::{
    expression::{
        literal::{Literal, LiteralKind},
        operator::{
            binary::{ArithmeticOp, BinaryOp, BitwiseOp, LogicalOp, RelationalOp},
            unary::{Unary, UnaryOp},
            Binary, Conditional,
        },
        Expression,
    },
    statement::{If, Statement, WhileLoop},
    visitor::{VisitWith, VisitorMut},
    Spanned, StatementList,
};
use boa_interner::Interner;
use std::ops::ControlFlow;

/// Number of mutators in [`mutate`]'s rotation.
pub const NUM_MUTATORS: usize = 5;

/// Apply mutator `which` at site `site` (both wrapping) to `ast`.
///
/// Returns `true` when an edit fired. Deterministic in `(which, site)`.
/// The caller re-lifts and re-parses; mutants that fail the parse-gate are
/// silently skipped (they are derived inputs, not corpus signal).
#[must_use]
pub fn mutate(ast: &mut StatementList, interner: &mut Interner, which: usize, site: usize) -> bool {
    match which % NUM_MUTATORS {
        0 => BinopSwap::new(site).run(ast),
        1 => LiteralTweak::new(site, interner).run(ast),
        2 => NegateIf::new(site).run(ast),
        3 => SwapConditional::new(site).run(ast),
        _ => NegateWhile::new(site).run(ast),
    }
}

/// Swap one binary operator for a same-arity partner.
struct BinopSwap {
    skip: usize,
    seen: usize,
}

impl BinopSwap {
    fn new(skip: usize) -> Self {
        Self { skip, seen: 0 }
    }

    fn run(&mut self, ast: &mut StatementList) -> bool {
        matches!(self.visit_statement_list_mut(ast), ControlFlow::Break(()))
    }

    /// Total partner map (every operator but `Comma`, which has no
    /// same-arity partner and is therefore never a site).
    fn partner(op: BinaryOp) -> Option<BinaryOp> {
        match op {
            BinaryOp::Arithmetic(ArithmeticOp::Add) => {
                Some(BinaryOp::Arithmetic(ArithmeticOp::Sub))
            }
            BinaryOp::Arithmetic(ArithmeticOp::Sub) => {
                Some(BinaryOp::Arithmetic(ArithmeticOp::Add))
            }
            BinaryOp::Arithmetic(ArithmeticOp::Mul) => {
                Some(BinaryOp::Arithmetic(ArithmeticOp::Div))
            }
            BinaryOp::Arithmetic(ArithmeticOp::Div) => {
                Some(BinaryOp::Arithmetic(ArithmeticOp::Mul))
            }
            BinaryOp::Arithmetic(ArithmeticOp::Exp) => {
                Some(BinaryOp::Arithmetic(ArithmeticOp::Mod))
            }
            BinaryOp::Arithmetic(ArithmeticOp::Mod) => {
                Some(BinaryOp::Arithmetic(ArithmeticOp::Exp))
            }
            BinaryOp::Bitwise(BitwiseOp::And) => Some(BinaryOp::Bitwise(BitwiseOp::Or)),
            BinaryOp::Bitwise(BitwiseOp::Or) => Some(BinaryOp::Bitwise(BitwiseOp::Xor)),
            BinaryOp::Bitwise(BitwiseOp::Xor) => Some(BinaryOp::Bitwise(BitwiseOp::And)),
            BinaryOp::Bitwise(BitwiseOp::Shl) => Some(BinaryOp::Bitwise(BitwiseOp::Shr)),
            BinaryOp::Bitwise(BitwiseOp::Shr) => Some(BinaryOp::Bitwise(BitwiseOp::UShr)),
            BinaryOp::Bitwise(BitwiseOp::UShr) => Some(BinaryOp::Bitwise(BitwiseOp::Shl)),
            BinaryOp::Relational(RelationalOp::Equal) => {
                Some(BinaryOp::Relational(RelationalOp::NotEqual))
            }
            BinaryOp::Relational(RelationalOp::NotEqual) => {
                Some(BinaryOp::Relational(RelationalOp::Equal))
            }
            BinaryOp::Relational(RelationalOp::StrictEqual) => {
                Some(BinaryOp::Relational(RelationalOp::StrictNotEqual))
            }
            BinaryOp::Relational(RelationalOp::StrictNotEqual) => {
                Some(BinaryOp::Relational(RelationalOp::StrictEqual))
            }
            BinaryOp::Relational(RelationalOp::GreaterThan) => {
                Some(BinaryOp::Relational(RelationalOp::LessThanOrEqual))
            }
            BinaryOp::Relational(RelationalOp::GreaterThanOrEqual) => {
                Some(BinaryOp::Relational(RelationalOp::LessThan))
            }
            BinaryOp::Relational(RelationalOp::LessThan) => {
                Some(BinaryOp::Relational(RelationalOp::GreaterThanOrEqual))
            }
            BinaryOp::Relational(RelationalOp::LessThanOrEqual) => {
                Some(BinaryOp::Relational(RelationalOp::GreaterThan))
            }
            BinaryOp::Relational(RelationalOp::In) => {
                Some(BinaryOp::Relational(RelationalOp::InstanceOf))
            }
            BinaryOp::Relational(RelationalOp::InstanceOf) => {
                Some(BinaryOp::Relational(RelationalOp::In))
            }
            BinaryOp::Logical(LogicalOp::And) => Some(BinaryOp::Logical(LogicalOp::Or)),
            BinaryOp::Logical(LogicalOp::Or) => Some(BinaryOp::Logical(LogicalOp::Coalesce)),
            BinaryOp::Logical(LogicalOp::Coalesce) => Some(BinaryOp::Logical(LogicalOp::And)),
            BinaryOp::Comma => None,
        }
    }
}

impl<'ast> VisitorMut<'ast> for BinopSwap {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<Self::BreakTy> {
        if let Expression::Binary(binary) = node {
            if let Some(swapped) = Self::partner(binary.op()) {
                if self.seen == self.skip {
                    let rebuilt = Binary::new(swapped, binary.lhs().clone(), binary.rhs().clone());
                    *node = Expression::Binary(rebuilt);
                    return ControlFlow::Break(());
                }
                self.seen += 1;
            }
        }
        node.visit_with_mut(self)
    }
}

/// Tweak one literal to a neighboring value (total map, no invalid output).
struct LiteralTweak<'interner> {
    skip: usize,
    seen: usize,
    interner: &'interner mut Interner,
}

impl<'interner> LiteralTweak<'interner> {
    fn new(skip: usize, interner: &'interner mut Interner) -> Self {
        Self {
            skip,
            seen: 0,
            interner,
        }
    }

    fn run(&mut self, ast: &mut StatementList) -> bool {
        matches!(self.visit_statement_list_mut(ast), ControlFlow::Break(()))
    }
}

impl<'ast> VisitorMut<'ast> for LiteralTweak<'_> {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<Self::BreakTy> {
        if let Expression::Literal(literal) = node {
            let tweaked = match literal.kind() {
                LiteralKind::Int(i) => LiteralKind::Int(i.wrapping_add(1)),
                LiteralKind::Num(f) => LiteralKind::Num(-f),
                LiteralKind::Bool(b) => LiteralKind::Bool(!b),
                LiteralKind::Null => LiteralKind::Undefined,
                LiteralKind::Undefined => LiteralKind::Null,
                LiteralKind::String(_) => LiteralKind::String(self.interner.get_or_intern("m")),
                // BigInt has no cheap neighbor; not a site.
                LiteralKind::BigInt(_) => return node.visit_with_mut(self),
            };
            if self.seen == self.skip {
                let span = literal.span();
                *node = Expression::Literal(Literal::new(tweaked, span));
                return ControlFlow::Break(());
            }
            self.seen += 1;
        }
        node.visit_with_mut(self)
    }
}

/// Negate one `if` condition (`if (c)` → `if (!c)`).
struct NegateIf {
    skip: usize,
    seen: usize,
}

impl NegateIf {
    fn new(skip: usize) -> Self {
        Self { skip, seen: 0 }
    }

    fn run(&mut self, ast: &mut StatementList) -> bool {
        matches!(self.visit_statement_list_mut(ast), ControlFlow::Break(()))
    }
}

impl<'ast> VisitorMut<'ast> for NegateIf {
    type BreakTy = ();

    fn visit_statement_mut(&mut self, node: &'ast mut Statement) -> ControlFlow<Self::BreakTy> {
        if let Statement::If(if_node) = node {
            if self.seen == self.skip {
                let cond = if_node.cond().clone();
                let span = cond.span();
                let negated = Expression::Unary(Unary::new(UnaryOp::Not, cond, span));
                let rebuilt = If::new(
                    negated,
                    if_node.body().clone(),
                    if_node.else_node().cloned(),
                );
                *node = Statement::If(rebuilt);
                return ControlFlow::Break(());
            }
            self.seen += 1;
        }
        node.visit_with_mut(self)
    }
}

/// Swap one conditional's branches (`c ? a : b` → `c ? b : a`).
struct SwapConditional {
    skip: usize,
    seen: usize,
}

impl SwapConditional {
    fn new(skip: usize) -> Self {
        Self { skip, seen: 0 }
    }

    fn run(&mut self, ast: &mut StatementList) -> bool {
        matches!(self.visit_statement_list_mut(ast), ControlFlow::Break(()))
    }
}

impl<'ast> VisitorMut<'ast> for SwapConditional {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<Self::BreakTy> {
        if let Expression::Conditional(cond_node) = node {
            if self.seen == self.skip {
                let rebuilt = Conditional::new(
                    cond_node.condition().clone(),
                    cond_node.if_false().clone(),
                    cond_node.if_true().clone(),
                );
                *node = Expression::Conditional(rebuilt);
                return ControlFlow::Break(());
            }
            self.seen += 1;
        }
        node.visit_with_mut(self)
    }
}

/// Negate one `while` condition (`while (c)` → `while (!c)`).
struct NegateWhile {
    skip: usize,
    seen: usize,
}

impl NegateWhile {
    fn new(skip: usize) -> Self {
        Self { skip, seen: 0 }
    }

    fn run(&mut self, ast: &mut StatementList) -> bool {
        matches!(self.visit_statement_list_mut(ast), ControlFlow::Break(()))
    }
}

impl<'ast> VisitorMut<'ast> for NegateWhile {
    type BreakTy = ();

    fn visit_statement_mut(&mut self, node: &'ast mut Statement) -> ControlFlow<Self::BreakTy> {
        if let Statement::WhileLoop(loop_node) = node {
            if self.seen == self.skip {
                let cond = loop_node.condition().clone();
                let span = cond.span();
                let negated = Expression::Unary(Unary::new(UnaryOp::Not, cond, span));
                let rebuilt = WhileLoop::new(negated, loop_node.body().clone());
                *node = Statement::WhileLoop(rebuilt);
                return ControlFlow::Break(());
            }
            self.seen += 1;
        }
        node.visit_with_mut(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{eval_outcome, eval_outcome_unoptimized, outcomes_equal};
    use boa_ast::scope::Scope;
    use boa_interner::ToInternedString;
    use boa_parser::{Parser, Source};
    use std::io::Cursor;

    /// Parse `source`, returning the script plus its interner.
    fn parse(source: &str) -> (boa_ast::Script, Interner) {
        let mut interner = Interner::default();
        let script = Parser::new(Source::from_reader(Cursor::new(source), None))
            .parse_script(&Scope::new_global(), &mut interner)
            .expect("test program must parse");
        (script, interner)
    }

    /// Mutate `source` with `(which, site)` and lift back to source.
    /// Returns `None` when no edit fired.
    fn mutant(source: &str, which: usize, site: usize) -> Option<String> {
        let (mut script, mut interner) = parse(source);
        if !mutate(script.statements_mut(), &mut interner, which, site) {
            return None;
        }
        Some(script.to_interned_string(&interner))
    }

    /// A lifted mutant must re-parse (validity bias holds on real shapes).
    fn assert_reparses(lifted: &str) {
        let mut interner = Interner::default();
        Parser::new(Source::from_reader(Cursor::new(lifted), None))
            .parse_script(&Scope::new_global(), &mut interner)
            .expect("mutant must re-parse");
    }

    #[test]
    fn binop_swap_fires_and_reparses() {
        let lifted = mutant("a + b;", 0, 0).expect("site exists");
        assert!(lifted.contains('-'), "unexpected lift: {lifted}");
        assert_reparses(&lifted);
    }

    #[test]
    fn binop_swap_site_selection() {
        // Two sites: `+` then `*`. Site 1 must hit only the `*`.
        let lifted = mutant("a + b; c * d;", 0, 1).expect("second site exists");
        assert!(lifted.contains('+'), "first op must survive: {lifted}");
        assert!(lifted.contains('/'), "second op must swap: {lifted}");
        assert_reparses(&lifted);
    }

    #[test]
    fn literal_tweak_covers_kinds() {
        // (`undefined` is an identifier, not a literal: no site there.)
        for (source, needle) in [
            ("x(1);", "2"),
            ("x(2.5);", "-"),
            ("x(true);", "false"),
            ("x('s');", "m"),
            ("x(null);", "undefined"),
        ] {
            let lifted = mutant(source, 1, 0).expect("literal site exists");
            assert!(
                lifted.contains(needle),
                "{source} -> {lifted} should contain {needle}"
            );
            assert_reparses(&lifted);
        }
    }

    #[test]
    fn negate_if_fires_and_reparses() {
        let lifted = mutant("if (a) { b(); }", 2, 0).expect("if site exists");
        assert!(lifted.contains('!'), "unexpected lift: {lifted}");
        assert_reparses(&lifted);
    }

    #[test]
    fn swap_conditional_fires_and_reparses() {
        let lifted = mutant("x ? a : b;", 3, 0).expect("conditional site exists");
        assert_reparses(&lifted);
        // Branch order flipped: `b` now precedes `a`.
        let b = lifted.find('b').expect("b present");
        let a = lifted.find('a').expect("a present");
        assert!(b < a, "branches not swapped: {lifted}");
    }

    #[test]
    fn negate_while_fires_and_reparses() {
        let lifted = mutant("while (a) { b(); }", 4, 0).expect("while site exists");
        assert!(lifted.contains('!'), "unexpected lift: {lifted}");
        assert_reparses(&lifted);
    }

    #[test]
    fn missing_site_does_not_fire() {
        assert!(mutant("a;", 0, 0).is_none());
        assert!(mutant("a + b;", 0, 7).is_none());
        assert!(mutant("a;", 4, 0).is_none());
    }

    #[test]
    fn mutation_is_deterministic() {
        let first = mutant("a + b * c; if (x) { y(); }", 0, 1);
        let second = mutant("a + b * c; if (x) { y(); }", 0, 1);
        assert_eq!(first, second);
    }

    /// The 5.2c core promise in miniature: every firing mutant of a fixed
    /// program runs the differential legs cleanly (normal vs unoptimized).
    #[test]
    fn mutants_run_legs_cleanly() {
        for source in ["a + b * 2;", "if (x) { y(); } else { z(); }", "n ? 1 : 2;"] {
            for which in 0..NUM_MUTATORS {
                let Some(lifted) = mutant(source, which, 0) else {
                    continue;
                };
                assert_reparses(&lifted);
                let normal = eval_outcome(&lifted);
                let unoptimized = eval_outcome_unoptimized(&lifted);
                assert!(
                    outcomes_equal(&normal, &unoptimized),
                    "optimizer leg diverged on mutant of {source}: {lifted}\n{normal:?}\n{unoptimized:?}"
                );
            }
        }
    }
}
