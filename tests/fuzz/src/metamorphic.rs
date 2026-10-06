//! Semantics-preserving AST transforms for VM metamorphic differentials (P5.3).
//!
//! [`metamorphic_variants`] rewrites one parsed program into up to four
//! single-transform variants (one per firing transform) that must evaluate
//! *identically* to the original. The `vm-implied` loop asserts full
//! [`EvalOutcome`][crate::semantic::EvalOutcome] equality (completion class +
//! canonical primitive) between the original and every variant, so a
//! divergence on an oracle-qualified transform is an engine bug.
//!
//! ## The four transforms
//!
//! Each is a deep bottom-up [`VisitorMut`] pass (children first, so inner
//! rewrites enable outer ones in a single pass). Proof sketches:
//!
//! - **T1 `paren-add`**: wrapping any expression in parentheses is
//!   meaning-preserving (ECMA-262 §13.2.10, Grouping Operator evaluates its
//!   inner with no additional semantics), and the parens are explicit in the
//!   lifted text, so the variant re-parses to the same tree shape. Lifts
//!   past [`crate::semantic::MAX_DERIVED_NESTING`] are dropped (parser
//!   red-zone; variants must not straddle it).
//! - **T2 `const-fold`**: `+`/`-`/`*` on `Int`+`Int` (exact in `i64`,
//!   `Int`-when-it-fits else the correctly-rounded `f64` — Rust `as` casts
//!   round-to-nearest-even, matching IEEE-754/JS) and on `Num`+`Num` (Rust
//!   emits precise IEEE-754 ops; this workspace sets no fast-math flags, so
//!   `+`/`-`/`*` are bitwise identical to JS). Also `&`/`|`/`^` on
//!   `Int`+`Int` (`ToInt32` is the identity there), unary `-` on
//!   `Int`/`Num`, and `!` on `Bool`. NaN results are *not* folded (`NaN`
//!   prints as the shadowable `NaN` identifier, not a literal).
//! - **T3 `cond-fold`**: a literal condition has no effects and no abrupt
//!   completion, and only the taken branch evaluates (ECMA-262 §13.14.1 the
//!   Conditional Operator, §14.6 the `if` Statement), so replacing the node
//!   *in position* with the taken branch preserves evaluation and completion
//!   values. `if (false) s` without `else` is excluded (`Empty` inherits the
//!   previous completion, which would change completion values).
//! - **T4 `regroup`**: `(a⊕b)⊕c` → `a⊕(b⊕c)` for same-operator `&&`/`||`/`??`
//!   (each yields the first decisive operand or the last, evaluated strictly
//!   left-to-right with identical short-circuit order in both nestings) and
//!   `&`/`|`/`^` (exact associativity, no short-circuit). Same-operator only:
//!   mixed operators, `+`/`*` (float rounding), `-`/`/`/`%`/shifts, and
//!   comparisons are all excluded.
//!
//! ## Deliberately skipped (plan items dropped with reason)
//!
//! - Paren *dropping* (the T1 dual): the AST printer never *inserts*
//!   precedence parens, so dropped parens mis-reparse (`-(5 + 2)` lifts to
//!   `- 5 + 2`). Verifying the lift structurally would need span-insensitive
//!   AST equality, which does not exist (spans compare by value, and the
//!   misprint *is* a print fixpoint, so the `parser-idempotency` string
//!   check cannot see it either). Adding parens stays inside the printer's
//!   parser-shaped-ASTs contract by construction.
//! - Dead-store elimination and statement reordering: `StatementList` items
//!   are sealed to out-of-crate passes, and completion values make position
//!   changes unsound without control-flow analysis.
//! - Loop unrolling: needs break/continue/label and effect analysis beyond
//!   local rewriting (plus the same completion sensitivity).
//! - Alpha-renaming: needs scope resolution; textual renaming breaks property
//!   names, labels, and `with` bodies.
//! - String folds and `ToInt32`-dependent folds (bitwise on `Num`, shifts):
//!   the AST printer does not escape quotes, so synthesized strings can
//!   mis-reparse; revisit with a printer fix.
//!
//! ## Soundness argument (four layers)
//!
//! 1. The proof sketches above (reviewed, spec-cited).
//! 2. The unit value battery below: Boa-vs-Boa *with primitives* on curated
//!    edge values (`-0`, `NaN`-adjacent, `i32` boundaries, infinities).
//! 3. The ignored `soundness_gate_10k` test: 10k generated programs × firing
//!    transforms, oracle-vs-oracle (completion + error class) at scale.
//! 4. Continuous comparison in the `vm-implied` loop on every fuzz input.
//!
//! Triage rule: a counterexample indicts the *transform* first. Reproduce
//! with the oracle on both sides (`node --print` shows values the harness
//! oracle comparison elides); only oracle-agreeing pairs are engine bugs.

use boa_ast::{
    expression::{
        literal::{Literal, LiteralKind},
        operator::{
            binary::{ArithmeticOp, BinaryOp, BitwiseOp, LogicalOp},
            unary::UnaryOp,
            Binary,
        },
        Expression, Parenthesized,
    },
    scope::Scope,
    statement::Statement,
    visitor::{VisitWith, VisitorMut},
    Script, Spanned,
};
use boa_interner::{Interner, ToInternedString};
use boa_parser::{Parser, Source};
use std::{io::Cursor, ops::ControlFlow};

use crate::semantic::{max_nesting_depth, MAX_DERIVED_NESTING};

/// One semantics-preserving rewrite of a program.
pub struct Variant {
    /// Transform name (`paren-add`, `const-fold`, `cond-fold`, `regroup`).
    pub name: &'static str,
    /// Lifted variant source (re-parses; compared by *evaluation*, not text).
    pub source: String,
}

/// Rewrite `source` into its single-transform metamorphic variants.
///
/// Returns one variant per firing transform (at most four), each applying one
/// transform at all of its sites. Unparsable input yields no variants.
/// Variants whose lift coincides with the original lift carry no signal and
/// are dropped (e.g. parens the printer re-inserts).
#[must_use]
pub fn metamorphic_variants(source: &str) -> Vec<Variant> {
    let mut interner = Interner::default();
    let script = match Parser::new(Source::from_reader(Cursor::new(source), None))
        .parse_script(&Scope::new_global(), &mut interner)
    {
        Ok(script) => script,
        Err(_) => return Vec::new(),
    };
    let baseline = script.to_interned_string(&interner);
    let mut variants = Vec::new();

    let mut t1 = script.clone();
    let _ = ParenAdd.visit_statement_list_mut(t1.statements_mut());
    push_if_changed(&t1, &interner, &baseline, "paren-add", &mut variants);

    let mut t2 = script.clone();
    let _ = ConstFold.visit_statement_list_mut(t2.statements_mut());
    push_if_changed(&t2, &interner, &baseline, "const-fold", &mut variants);

    let mut t3 = script.clone();
    let _ = CondFold.visit_statement_list_mut(t3.statements_mut());
    push_if_changed(&t3, &interner, &baseline, "cond-fold", &mut variants);

    let mut t4 = script.clone();
    let _ = Regroup.visit_statement_list_mut(t4.statements_mut());
    push_if_changed(&t4, &interner, &baseline, "regroup", &mut variants);

    variants
}

/// Lift `script`, keeping it as `name` only when [`emit_variant`] agrees.
fn push_if_changed(
    script: &Script,
    interner: &Interner,
    baseline: &str,
    name: &'static str,
    variants: &mut Vec<Variant>,
) {
    let lifted = script.to_interned_string(interner);
    if emit_variant(&lifted, baseline) {
        variants.push(Variant {
            name,
            source: lifted,
        });
    }
}

/// Emission rule, factored for unit tests: the lift must differ from the
/// baseline (else the variant carries no signal) and stay within the
/// derived-input nesting budget (else it could straddle the parser
/// red-zone its original passes — see [`MAX_DERIVED_NESTING`]).
fn emit_variant(lifted: &str, baseline: &str) -> bool {
    lifted != baseline && max_nesting_depth(lifted) <= MAX_DERIVED_NESTING
}

/// T1: wrap bare expressions in redundant parentheses.
///
/// Adding parens is sound by construction: grouping has no runtime semantics
/// (ECMA-262 §13.2.10) and the parens are explicit in the lifted text, so the
/// variant re-parses to the same tree shape. (The dual — dropping parens —
/// was rejected: the AST printer never *inserts* precedence parens, so
/// dropping creates shapes that mis-reparse, e.g. `-(5 + 2)` → `- 5 + 2`.
/// The printer is only obligated to print parser-shaped ASTs; T1-add stays
/// inside that contract by construction.)
struct ParenAdd;

impl<'ast> VisitorMut<'ast> for ParenAdd {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<()> {
        // Children first (bottom-up): inner rewrites enable outer ones.
        if node.visit_with_mut(self).is_break() {
            return ControlFlow::Break(());
        }
        // No per-level depth cap: [`emit_variant`] drops over-nested lifts
        // wholesale, which subsumes any added-depth bound.
        if !matches!(&*node, Expression::Parenthesized(_)) {
            let span = node.span();
            let dummy = Expression::Literal(Literal::new(LiteralKind::Null, span));
            let inner = std::mem::replace(node, dummy);
            *node = Expression::Parenthesized(Parenthesized::new(inner, span));
        }
        ControlFlow::Continue(())
    }
}

/// T2: fold pure literal arithmetic, bitwise, and unary operations.
struct ConstFold;

impl<'ast> VisitorMut<'ast> for ConstFold {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<()> {
        if node.visit_with_mut(self).is_break() {
            return ControlFlow::Break(());
        }
        let folded = match &*node {
            Expression::Binary(b) => fold_binary(b.op(), b.lhs(), b.rhs()),
            Expression::Unary(u) => fold_unary(u.op(), u.target()),
            _ => None,
        };
        if let Some(kind) = folded {
            let span = node.span();
            *node = Expression::Literal(Literal::new(kind, span));
        }
        ControlFlow::Continue(())
    }
}

/// T3: replace literal-conditioned branches with the taken branch.
struct CondFold;

impl<'ast> VisitorMut<'ast> for CondFold {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<()> {
        if node.visit_with_mut(self).is_break() {
            return ControlFlow::Break(());
        }
        if let Expression::Conditional(c) = &*node {
            if let Some(taken) = literal_truthiness(c.condition()) {
                let branch = if taken { c.if_true() } else { c.if_false() }.clone();
                *node = branch;
            }
        }
        ControlFlow::Continue(())
    }

    fn visit_statement_mut(&mut self, node: &'ast mut Statement) -> ControlFlow<()> {
        if node.visit_with_mut(self).is_break() {
            return ControlFlow::Break(());
        }
        if let Statement::If(i) = &*node {
            if let Some(taken) = literal_truthiness(i.cond()) {
                if taken {
                    *node = i.body().clone();
                } else if let Some(alt) = i.else_node() {
                    *node = alt.clone();
                }
                // Falsy without `else` is kept: `Empty` inherits the previous
                // completion, so `;` is not equivalent in completion position.
            }
        }
        ControlFlow::Continue(())
    }
}

/// T4: rotate left-nested same-operator trees right.
struct Regroup;

impl<'ast> VisitorMut<'ast> for Regroup {
    type BreakTy = ();

    fn visit_expression_mut(&mut self, node: &'ast mut Expression) -> ControlFlow<()> {
        if node.visit_with_mut(self).is_break() {
            return ControlFlow::Break(());
        }
        // Build the rotation under a shared borrow, then install it: the
        // left nesting may hide under redundant parens (looked through —
        // same-operator parens are never load-bearing).
        let rotated: Option<Expression> = match &*node {
            Expression::Binary(b) if regroupable(b.op()) => {
                match left_nested_same_op(b.op(), b.lhs()) {
                    Some(inner) => {
                        let op = b.op();
                        let rhs = Binary::new(op, inner.rhs().clone(), b.rhs().clone());
                        Some(Expression::Binary(Binary::new(
                            op,
                            inner.lhs().clone(),
                            Expression::Binary(rhs),
                        )))
                    }
                    None => None,
                }
            }
            _ => None,
        };
        if let Some(rotated) = rotated {
            *node = rotated;
        }
        ControlFlow::Continue(())
    }
}

/// Find a same-operator left nesting under `lhs`, looking through
/// `Parenthesized` layers (same-operator parens are never load-bearing, so
/// dropping them in the rotation is safe).
fn left_nested_same_op(outer_op: BinaryOp, lhs: &Expression) -> Option<&Binary> {
    let mut current = lhs;
    loop {
        match current {
            Expression::Parenthesized(p) => current = p.expression(),
            Expression::Binary(inner) if inner.op() == outer_op => return Some(inner),
            _ => return None,
        }
    }
}

/// Operators whose same-operator nesting is associative *with identical
/// left-to-right evaluation order* in both rotations (see module docs).
fn regroupable(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Logical(LogicalOp::And | LogicalOp::Or | LogicalOp::Coalesce)
            | BinaryOp::Bitwise(BitwiseOp::And | BitwiseOp::Or | BitwiseOp::Xor)
    )
}

/// Fold one binary operation over two literals.
fn fold_binary(op: BinaryOp, lhs: &Expression, rhs: &Expression) -> Option<LiteralKind> {
    let (Expression::Literal(l), Expression::Literal(r)) = (lhs, rhs) else {
        return None;
    };
    match op {
        BinaryOp::Arithmetic(op) => fold_arith(op, l.kind(), r.kind()),
        BinaryOp::Bitwise(BitwiseOp::And | BitwiseOp::Or | BitwiseOp::Xor) => {
            fold_int_bitwise(op, l.kind(), r.kind())
        }
        _ => None,
    }
}

/// Fold `+`/`-`/`*` over `Int`+`Int` or `Num`+`Num` (never mixed).
fn fold_arith(op: ArithmeticOp, lhs: &LiteralKind, rhs: &LiteralKind) -> Option<LiteralKind> {
    match (lhs, rhs) {
        (LiteralKind::Int(a), LiteralKind::Int(b)) => {
            // `i32` arithmetic is exact in `i64` (products fit: |a·b| ≤ 2⁶²).
            // `Add`/`Sub` on integers never yield `-0` (that needs a `-0`
            // operand); `Mul` can (`0 * -x` is `-0`), so zero products are
            // recomputed in `f64`, where the operands are exact and the
            // signed zero comes out right.
            let (a, b) = (i64::from(*a), i64::from(*b));
            match op {
                ArithmeticOp::Add => Some(int_or_num(a + b)),
                ArithmeticOp::Sub => Some(int_or_num(a - b)),
                ArithmeticOp::Mul => {
                    if a * b == 0 {
                        Some(LiteralKind::Num((a as f64) * (b as f64)))
                    } else {
                        Some(int_or_num(a * b))
                    }
                }
                _ => None,
            }
        }
        (LiteralKind::Num(a), LiteralKind::Num(b)) => {
            let value = match op {
                ArithmeticOp::Add => a + b,
                ArithmeticOp::Sub => a - b,
                ArithmeticOp::Mul => a * b,
                _ => return None,
            };
            // NaN prints as the shadowable `NaN` identifier, not a literal.
            if value.is_nan() {
                return None;
            }
            Some(LiteralKind::Num(value))
        }
        _ => None,
    }
}

/// `Int` when `value` fits, else the correctly-rounded `f64` (Rust `as`
/// rounds-to-nearest-even, matching IEEE-754/JS; no overflow: f64 ≫ i64).
fn int_or_num(value: i64) -> LiteralKind {
    match i32::try_from(value) {
        Ok(int) => LiteralKind::Int(int),
        Err(_) => LiteralKind::Num(value as f64),
    }
}

/// Fold `&`/`|`/`^` over `Int`+`Int` (`ToInt32` is the identity there, so
/// Rust `i32` ops are exactly JS). Bitwise on `Num` needs `ToInt32` and is
/// deliberately unhandled.
fn fold_int_bitwise(op: BinaryOp, lhs: &LiteralKind, rhs: &LiteralKind) -> Option<LiteralKind> {
    let (LiteralKind::Int(a), LiteralKind::Int(b)) = (lhs, rhs) else {
        return None;
    };
    let value = match op {
        BinaryOp::Bitwise(BitwiseOp::And) => a & b,
        BinaryOp::Bitwise(BitwiseOp::Or) => a | b,
        BinaryOp::Bitwise(BitwiseOp::Xor) => a ^ b,
        _ => return None,
    };
    Some(LiteralKind::Int(value))
}

/// Fold unary `-` over `Int`/`Num` and `!` over `Bool`.
fn fold_unary(op: UnaryOp, target: &Expression) -> Option<LiteralKind> {
    let Expression::Literal(lit) = target else {
        return None;
    };
    match (op, lit.kind()) {
        // `-(0)` is `-0`, not `+0`.
        (UnaryOp::Minus, LiteralKind::Int(a)) if *a == 0 => Some(LiteralKind::Num(-0.0)),
        (UnaryOp::Minus, LiteralKind::Int(a)) => Some(int_or_num(-i64::from(*a))),
        (UnaryOp::Minus, LiteralKind::Num(a)) => {
            let value = -a;
            if value.is_nan() {
                return None;
            }
            Some(LiteralKind::Num(value))
        }
        (UnaryOp::Not, LiteralKind::Bool(b)) => Some(LiteralKind::Bool(!b)),
        _ => None,
    }
}

/// `ToBoolean` for conditions that need no interner. Strings need symbol
/// resolution and `BigInt` is too rare in fuzz shapes to matter: both yield
/// `None` (conservative skip, not a wrong answer).
fn literal_truthiness(condition: &Expression) -> Option<bool> {
    let Expression::Literal(lit) = condition else {
        return None;
    };
    match lit.kind() {
        LiteralKind::Bool(b) => Some(*b),
        LiteralKind::Int(i) => Some(*i != 0),
        LiteralKind::Num(n) => Some(*n != 0.0 && !n.is_nan()),
        LiteralKind::Null | LiteralKind::Undefined => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battery::battery_program;
    use crate::semantic::{eval_outcome, outcomes_equal};

    /// Variant names for `source`, in transform order.
    fn names(source: &str) -> Vec<&'static str> {
        metamorphic_variants(source)
            .iter()
            .map(|v| v.name)
            .collect()
    }

    /// The `name` variant's source for `source` (panics when it did not fire).
    fn variant(source: &str, name: &str) -> String {
        metamorphic_variants(source)
            .into_iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("{name} did not fire on {source:?}"))
            .source
    }

    /// Every variant of `source` must re-parse and evaluate identically
    /// (full outcome: completion class + canonical primitive).
    fn assert_sound(source: &str) -> Vec<Variant> {
        let variants = metamorphic_variants(source);
        let expected = eval_outcome(source);
        for variant in &variants {
            assert_reparses(&variant.source);
            let outcome = eval_outcome(&variant.source);
            assert!(
                outcomes_equal(&expected, &outcome),
                "transform `{}` broke {source:?}:\nvariant:\n{}\nexpected: {expected:?}\ngot: {outcome:?}",
                variant.name, variant.source,
            );
        }
        variants
    }

    /// A lifted variant must re-parse (parse-gate pin).
    fn assert_reparses(lifted: &str) {
        let mut interner = Interner::default();
        Parser::new(Source::from_reader(Cursor::new(lifted), None))
            .parse_script(&Scope::new_global(), &mut interner)
            .expect("variant must re-parse");
    }

    #[test]
    fn paren_add_wraps_bare_expressions() {
        assert_sound("(a) + (b);");
        assert_eq!(names("(a) + (b);"), vec!["paren-add"]);
        // Bare identifiers gain parens; already-parenthesized nodes double.
        assert!(variant("(a) + (b);", "paren-add").contains("((a))"));
    }

    #[test]
    fn emit_rule_drops_identical_and_over_nested() {
        assert!(!emit_variant("1;", "1;"));
        assert!(emit_variant("(1);", "1;"));
        let over = format!("{}1{}", "(".repeat(300), ")".repeat(300));
        assert!(!emit_variant(&over, "1;"));
        let under = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        assert!(emit_variant(&under, "1;"));
    }

    #[test]
    fn red_zone_budget_fails_closed() {
        // Ten-deep parses on a 2 MB test thread and paren-add fires.
        let shallow = format!("{}1{}", "(".repeat(10), ")".repeat(10));
        assert_eq!(names(&shallow), vec!["paren-add"]);
        // Thirty-deep exceeds the test-thread red-zone budget, so the parse
        // fails and no variants emerge (fail-closed, not unsound). On 64 MB
        // harness workers this parses and paren-add fires normally.
        let deep = format!("{}1{}", "(".repeat(30), ")".repeat(30));
        assert!(names(&deep).is_empty());
    }

    #[test]
    fn folds_fire_through_parens() {
        assert_eq!(names("(1 + 2) * 3;"), vec!["paren-add", "const-fold"]);
        assert_sound("(1 + 2) * 3;");
        assert_eq!(
            eval_outcome(&variant("(1 + 2) * 3;", "const-fold")).primitive,
            "9.0"
        );
    }

    #[test]
    fn const_fold_int_boundaries() {
        // `i32::MAX + 1` overflows to the correctly-rounded f64.
        assert_sound("2147483647 + 1;");
        assert_eq!(names("2147483647 + 1;"), vec!["paren-add", "const-fold"]);
        assert_eq!(
            eval_outcome(&variant("2147483647 + 1;", "const-fold")).primitive,
            "2147483648.0"
        );
        // Products up to 2⁶² stay exact through the `i64` widening.
        assert_sound("2000000000 * 2000000000;");
        assert_sound("0 - 2147483647;");
        // `- 0` must fold to `-0.0` (which prints back as `- 0`, so no
        // variant): folding to `Int(0)` instead would emit `0;` (wrongly
        // `+0`), which `assert_sound` would catch. Only paren-add fires.
        assert_sound("- 0;");
        assert_eq!(names("- 0;"), vec!["paren-add"]);
    }

    #[test]
    fn const_fold_float_round_trip() {
        // Pins the `Display` shortest-round-trip dependency: the folded
        // literal must reprint *and re-parse* to the identical f64.
        assert_sound("0.1 + 0.2;");
        let folded = variant("0.1 + 0.2;", "const-fold");
        assert!(folded.contains("0.30000000000000004"));
        assert_eq!(eval_outcome(&folded).primitive, "0.30000000000000004");
        // Infinities fold (exact) and print as overflowing literals.
        assert_sound("1e999 + 1e999;");
        assert_eq!(
            eval_outcome(&variant("1e999 + 1e999;", "const-fold")).primitive,
            "inf"
        );
        // Negative zero survives the fold exactly.
        assert_sound("0.0 * -1.0;");
        assert_eq!(
            eval_outcome(&variant("0.0 * -1.0;", "const-fold")).primitive,
            "-0.0"
        );
    }

    #[test]
    fn const_fold_skips_nan() {
        // `inf - inf` is NaN, which prints as the shadowable `NaN`
        // identifier — so no fold (only paren-add fires here).
        assert_eq!(names("1e999 - 1e999;"), vec!["paren-add"]);
    }

    #[test]
    fn const_fold_unary_and_bitwise() {
        assert_sound("!true;");
        assert!(variant("!true;", "const-fold").contains("false"));
        assert_sound("- 5;");
        assert_sound("6 & 3;");
        assert!(variant("6 & 3;", "const-fold").contains('2'));
        assert_sound("5 | 2;");
        assert_sound("5 ^ 1;");
    }

    #[test]
    fn cond_fold_ternary() {
        assert_sound("true ? 1 : 2;");
        let folded = variant("true ? 1 : 2;", "cond-fold");
        assert!(!folded.contains('?'));
        assert_eq!(eval_outcome(&folded).primitive, "1.0");
        assert_sound("0 ? 1 : 2;");
        assert_sound("undefined ? 1 : 2;");
        assert_sound("0.5 ? 1 : 2;");
    }

    #[test]
    fn cond_fold_if() {
        assert_sound("if (false) { 1; } else { 2; }");
        assert!(!variant("if (false) { 1; } else { 2; }", "cond-fold").contains("if "));
        assert_sound("if (true) { 1; }");
        // Falsy without `else`: kept (completion-sensitivity); only
        // paren-add fires on this shape.
        assert_eq!(names("if (false) { 1; }"), vec!["paren-add"]);
    }

    #[test]
    fn regroup_and_or_coalesce() {
        let source = "let a = 1; let b = 2; let c = 0; (a && b) && c;";
        assert_sound(source);
        assert_eq!(names(source), vec!["paren-add", "regroup"]);
        assert_eq!(eval_outcome(&variant(source, "regroup")).primitive, "0.0");
        assert_sound("let a = 0; let b = 1; let c = 2; (a || b) || c;");
        assert_sound("let a = 6; let b = 3; let c = 1; (a & b) & c;");
        assert_sound("let a = null; let b = 1; (a ?? b) ?? 2;");
        // Look-through fires under doubled parens too.
        let doubled = "let a = 1; let b = 2; let c = 3; ((a && b)) && c;";
        assert_sound(doubled);
        assert_eq!(names(doubled), vec!["paren-add", "regroup"]);
    }

    #[test]
    fn regroup_rejects_mixed_operators() {
        // Different operators at the two levels, or a non-regroupable
        // operator: regroup must not fire (only paren-add does).
        assert_eq!(names("(a && b) || c;"), vec!["paren-add"]);
        assert_eq!(names("(a + b) + c;"), vec!["paren-add"]);
    }

    #[test]
    fn value_battery_end_to_end() {
        // Curated edge values: every variant must re-parse and evaluate
        // identically, primitives included.
        for source in [
            "0.1 + 0.2;",
            "2147483647 + 1;",
            "2000000000 * 2000000000;",
            "0.0 * -1.0;",
            "1e999 + 1e999;",
            "1 + 2 * 3;",
            "true ? 1 : 2;",
            "if (true) { 1; } else { 2; }",
            "(true && false) && true;",
            "((6));",
            "!false;",
            "-(5 + 2);",
            "(10 - 4) - 3;",
        ] {
            assert_sound(source);
        }
    }

    #[test]
    fn unparsable_yields_no_variants() {
        assert!(metamorphic_variants("function (;").is_empty());
    }

    // (seed-7701 diagnostics removed; the divergence filed as
    // `dce-dead-branch-completion-leak`. See `MAX_DERIVED_NESTING` docs for
    // the red-zone measurements from the same investigation.)

    // ------------------------------------------------------------------
    // Soundness gate (P5.3 validation): oracle-vs-oracle at scale.
    // ------------------------------------------------------------------

    /// Programs checked by [`soundness_gate_10k`]; pairs are programs ×
    /// firing transforms.
    const GATE_PROGRAMS: usize = 10_000;
    /// Minimum oracle-checked pairs per transform: below this the gate did
    /// not exercise the transform and fails instead of passing vacuously.
    const MIN_PAIRS_PER_TRANSFORM: usize = 100;
    const TRANSFORM_NAMES: [&str; 4] = ["paren-add", "const-fold", "cond-fold", "regroup"];

    /// P5.3 soundness gate: qualify the transforms against the oracle.
    ///
    /// Ignored by default (needs an oracle shell and a few minutes):
    /// `cd tests/fuzz && BOA_FUZZ_ORACLE=<node|jsshell> cargo test --lib
    /// soundness_gate_10k -- --ignored --nocapture`.
    ///
    /// For each generated program and each firing transform, the oracle must
    /// agree with *itself* on (original, variant) — completion + error
    /// class. Disagreement fails the gate: the TRANSFORM is unsound (fix the
    /// transform, never the engine). Boa-vs-Boa (full outcome, primitives
    /// included) runs alongside: oracle agreement plus a Boa divergence is an
    /// ENGINE bug — file it, then re-run with `BOA_GATE_FILED_ENGINE_DIV=1`
    /// to acknowledge and re-qualify the transforms.
    ///
    /// Programs come from the hermetic [`battery_program`](crate::battery::battery_program) battery
    /// (valid-by-construction, transform-dense). Deterministic: program `i`
    /// derives from seed `i`, so failures reproduce with the printed seed.
    /// Failure examples print sorted by seed (first 10); counts are exact.
    #[test]
    #[ignore]
    fn soundness_gate_10k() {
        use crate::semantic::{eval_oracle, EvalOutcome, OracleOutcome};

        let oracle = std::env::var("BOA_FUZZ_ORACLE")
            .expect("soundness gate needs BOA_FUZZ_ORACLE=<oracle shell path>");
        let engine_div_acked = std::env::var("BOA_GATE_FILED_ENGINE_DIV").is_ok();
        // Pilot override (default: the full 10k). Floors scale accordingly.
        let programs: usize = std::env::var("BOA_GATE_PROGRAMS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(GATE_PROGRAMS);
        let min_pairs = (MIN_PAIRS_PER_TRANSFORM * programs / GATE_PROGRAMS).max(1);

        #[derive(Debug)]
        struct SoundnessFailure {
            seed: usize,
            name: &'static str,
            orig: OracleOutcome,
            variant: OracleOutcome,
            orig_src: String,
            variant_src: String,
        }
        #[derive(Debug)]
        struct EngineDivergence {
            seed: usize,
            name: &'static str,
            orig: EvalOutcome,
            variant: EvalOutcome,
            orig_src: String,
            variant_src: String,
        }
        struct ThreadReport {
            pairs: usize,
            skipped_fuel: usize,
            skipped_oracle: usize,
            per_transform: [usize; 4],
            soundness_fail_total: usize,
            soundness_fail_examples: Vec<SoundnessFailure>,
            engine_div_total: usize,
            engine_div_examples: Vec<EngineDivergence>,
        }

        fn transform_index(name: &str) -> usize {
            TRANSFORM_NAMES
                .iter()
                .position(|n| *n == name)
                .expect("unknown transform name")
        }

        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4);
        // 64 MB stacks (like the `vm-implied` worker): the parser red-zone
        // trips at ~10 nesting levels on 2 MB test threads, which template
        // variants straddle constantly. Scope threads cannot set stack
        // size, so these are `Builder` threads joined by hand.
        let oracle = std::sync::Arc::new(oracle);
        let chunk = programs.div_ceil(threads);
        let mut totals = ThreadReport {
            pairs: 0,
            skipped_fuel: 0,
            skipped_oracle: 0,
            per_transform: [0; 4],
            soundness_fail_total: 0,
            soundness_fail_examples: Vec::new(),
            engine_div_total: 0,
            engine_div_examples: Vec::new(),
        };
        let mut handles = Vec::new();
        for t in 0..threads {
            let oracle = oracle.clone();
            let lo = t * chunk;
            let hi = ((t + 1) * chunk).min(programs);
            handles.push(
                std::thread::Builder::new()
                    .name(format!("gate-worker-{t}"))
                    .stack_size(64 << 20)
                    .spawn(move || {
                        let mut report = ThreadReport {
                            pairs: 0,
                            skipped_fuel: 0,
                            skipped_oracle: 0,
                            per_transform: [0; 4],
                            soundness_fail_total: 0,
                            soundness_fail_examples: Vec::new(),
                            engine_div_total: 0,
                            engine_div_examples: Vec::new(),
                        };
                        for seed in lo..hi {
                            let source = battery_program(seed);
                            let variants = metamorphic_variants(&source);
                            // Template programs always parse and always carry
                            // expressions, so paren-add always fires: an empty
                            // set is a generator bug, failed loud with the
                            // offending program.
                            assert!(
                                !variants.is_empty(),
                                "gate generator bug (seed {seed}): no transform \
                             fired — program invalid or expressionless:\n{source}"
                            );
                            // Fuel pre-screen: likely-nonterminating programs
                            // would burn oracle timeouts; termination is
                            // preserved by construction (no transform changes
                            // evaluation counts or order), so skipping is safe.
                            let boa_orig = eval_outcome(&source);
                            if matches!(
                                boa_orig.error_class.as_deref(),
                                Some("FuelExhausted" | "RuntimeLimit")
                            ) {
                                report.skipped_fuel += 1;
                                continue;
                            }
                            let oracle_orig = eval_oracle(&oracle, &source);
                            for variant in &variants {
                                let oracle_var = eval_oracle(&oracle, &variant.source);
                                if oracle_orig.timed_out
                                    || oracle_orig.unclassified
                                    || oracle_var.timed_out
                                    || oracle_var.unclassified
                                {
                                    report.skipped_oracle += 1;
                                    continue;
                                }
                                report.pairs += 1;
                                report.per_transform[transform_index(variant.name)] += 1;
                                if oracle_orig != oracle_var {
                                    report.soundness_fail_total += 1;
                                    if report.soundness_fail_examples.len() < 20 {
                                        report.soundness_fail_examples.push(SoundnessFailure {
                                            seed,
                                            name: variant.name,
                                            orig: oracle_orig.clone(),
                                            variant: oracle_var.clone(),
                                            orig_src: source.clone(),
                                            variant_src: variant.source.clone(),
                                        });
                                    }
                                }
                                let boa_var = eval_outcome(&variant.source);
                                if !outcomes_equal(&boa_orig, &boa_var) {
                                    report.engine_div_total += 1;
                                    if report.engine_div_examples.len() < 20 {
                                        report.engine_div_examples.push(EngineDivergence {
                                            seed,
                                            name: variant.name,
                                            orig: boa_orig.clone(),
                                            variant: boa_var,
                                            orig_src: source.clone(),
                                            variant_src: variant.source.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        report
                    })
                    .expect("gate worker spawn failed"),
            );
        }
        for handle in handles {
            let report = handle.join().expect("gate worker panicked");
            totals.pairs += report.pairs;
            totals.skipped_fuel += report.skipped_fuel;
            totals.skipped_oracle += report.skipped_oracle;
            for (total, count) in totals.per_transform.iter_mut().zip(report.per_transform) {
                *total += count;
            }
            totals.soundness_fail_total += report.soundness_fail_total;
            totals
                .soundness_fail_examples
                .extend(report.soundness_fail_examples);
            totals.engine_div_total += report.engine_div_total;
            totals
                .engine_div_examples
                .extend(report.engine_div_examples);
        }
        totals.soundness_fail_examples.sort_by_key(|f| f.seed);
        totals.soundness_fail_examples.truncate(10);
        totals.engine_div_examples.sort_by_key(|d| d.seed);
        totals.engine_div_examples.truncate(10);

        fn snip(s: &str) -> String {
            const CAP: usize = 2000;
            if s.len() <= CAP {
                return s.to_owned();
            }
            let mut out: String = s.chars().take(CAP).collect();
            out.push_str("\n...[truncated]");
            out
        }

        println!("soundness gate: {programs} programs");
        println!("  pairs checked: {}", totals.pairs);
        for (name, count) in TRANSFORM_NAMES.iter().zip(totals.per_transform) {
            println!("    {name}: {count}");
        }
        println!(
            "  skipped: fuel={} oracle={}",
            totals.skipped_fuel, totals.skipped_oracle
        );
        println!(
            "  oracle disagreements (unsound transform): {}",
            totals.soundness_fail_total
        );
        for failure in &totals.soundness_fail_examples {
            println!(
                "    seed={} transform={} orig={:?} variant={:?}\n    orig src:\n{}\n    variant src:\n{}",
                failure.seed,
                failure.name,
                failure.orig,
                failure.variant,
                snip(&failure.orig_src),
                snip(&failure.variant_src)
            );
        }
        println!(
            "  Boa divergences on oracle-sound pairs (engine bugs): {}",
            totals.engine_div_total
        );
        for div in &totals.engine_div_examples {
            println!(
                "    seed={} transform={} orig={:?} variant={:?}\n    orig src:\n{}\n    variant src:\n{}",
                div.seed, div.name, div.orig, div.variant,
                snip(&div.orig_src),
                snip(&div.variant_src)
            );
        }

        for (name, count) in TRANSFORM_NAMES.iter().zip(totals.per_transform) {
            assert!(
                count >= min_pairs,
                "soundness gate vacuous for {name}: only {count} pairs \
                 (< {min_pairs}); strengthen the generator"
            );
        }
        assert!(
            totals.soundness_fail_total == 0,
            "SOUNDNESS GATE FAILED: {} oracle disagreement(s) — the \
             TRANSFORM is unsound; fix the transform, never the engine",
            totals.soundness_fail_total
        );
        if totals.engine_div_total > 0 && !engine_div_acked {
            panic!(
                "gate found {} ENGINE divergence(s) on oracle-sound pairs — \
                 FILE AS ENGINE BUGS, then re-run with \
                 BOA_GATE_FILED_ENGINE_DIV=1 to acknowledge",
                totals.engine_div_total
            );
        }
        if totals.engine_div_total > 0 {
            println!("  (engine divergences acknowledged via BOA_GATE_FILED_ENGINE_DIV)");
        }
        println!("soundness gate: PASS");
    }
}
