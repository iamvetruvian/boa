//! Canonicalization passes that keep `Arbitrary` ASTs inside the
//! parser-idempotency oracle's domain.
//!
//! `Arbitrary` derivation can produce AST states the parser itself can never
//! emit (non-finite float literals without literal syntax, templates with
//! mismatched parallel arrays). Printing such states yields source that
//! reparses with materialized names or elements, which the oracle would
//! (correctly, but uselessly) flag. These passes normalize those states up
//! front; execution-path coverage of the same values stays intact in
//! `vm-implied`, which shares no canonicalization.

use boa_ast::{
    declaration::{Declaration, LexicalDeclaration},
    expression::{
        literal::{Literal, LiteralKind, ObjectMethodDefinition, TemplateElement, TemplateLiteral},
        TaggedTemplate,
    },
    function::{
        ArrowFunction, AsyncArrowFunction, AsyncFunctionDeclaration, AsyncFunctionExpression,
        AsyncGeneratorDeclaration, AsyncGeneratorExpression, ClassElement, FunctionDeclaration,
        FunctionExpression, GeneratorDeclaration, GeneratorExpression,
    },
    property::MethodDefinitionKind,
    statement::iteration::ForLoopInitializer,
    visitor::{VisitWith, VisitorMut},
    Expression, Span, Spanned, Statement, StatementList, StatementListItem,
};
use boa_interner::{Interner, Sym};
use std::{convert::Infallible, ops::ControlFlow};

/// Map every `Num(NaN)` literal to zero.
///
/// `NaN` has no literal syntax: it prints as `NaN`, which reparses as an
/// identifier (a fresh name outside the pre-parse universe). Unreachable
/// from the parser, so out of the oracle's domain.
struct NanCanonicalizer;

impl VisitorMut<'_> for NanCanonicalizer {
    type BreakTy = Infallible;

    fn visit_expression_mut(&mut self, node: &mut Expression) -> ControlFlow<Self::BreakTy> {
        if let Expression::Literal(lit) = node {
            if let LiteralKind::Num(num) = lit.kind() {
                if num.is_nan() {
                    let span = lit.span();
                    *node = Expression::Literal(Literal::new(0.0, span));
                }
            }
        }
        node.visit_with_mut(self)
    }
}

/// Normalize templates to parser-reachable shapes.
///
/// `TaggedTemplate` uses derived `Arbitrary` for its parallel arrays, so
/// `raws`/`cookeds`/`exprs` arrive with independent (usually mismatched)
/// lengths, and `TemplateLiteral` may arrive empty or trailing an
/// expression. Normalize to canonical shape (`raws.len() ==
/// cookeds.len() == exprs.len() + 1 >= 1`; literals non-empty and
/// string-terminated). Cooked values are padding-only: the printer renders
/// raws, and the parser recomputes cookeds from them.
struct TemplateCanonicalizer {
    empty: Sym,
}

impl VisitorMut<'_> for TemplateCanonicalizer {
    type BreakTy = Infallible;

    fn visit_expression_mut(&mut self, node: &mut Expression) -> ControlFlow<Self::BreakTy> {
        match node {
            Expression::TaggedTemplate(t) => {
                let n = t.exprs().len();
                if t.raws().len() != n + 1 || t.cookeds().len() != n + 1 {
                    let mut raws = t.raws().to_vec();
                    let mut cookeds = t.cookeds().to_vec();
                    raws.resize(n + 1, self.empty);
                    cookeds.resize(n + 1, Some(self.empty));
                    let tag = t.tag().clone();
                    let exprs = t.exprs().to_vec().into_boxed_slice();
                    let (identifier, span) = (t.identifier(), t.span());
                    *node = Expression::TaggedTemplate(TaggedTemplate::new(
                        tag,
                        raws.into_boxed_slice(),
                        cookeds.into_boxed_slice(),
                        exprs,
                        identifier,
                        span,
                    ));
                }
            }
            Expression::TemplateLiteral(t) => {
                let elts = t.elements();
                let needs_trailer =
                    elts.is_empty() || matches!(elts.last(), Some(TemplateElement::Expr(_)));
                if needs_trailer {
                    let mut elts = elts.to_vec();
                    elts.push(TemplateElement::String(self.empty));
                    let span = t.span();
                    *node = Expression::TemplateLiteral(TemplateLiteral::new(
                        elts.into_boxed_slice(),
                        span,
                    ));
                }
            }
            _ => {}
        }
        node.visit_with_mut(self)
    }
}

/// Rewrite `yield`/`await` expressions that sit outside a generator /
/// async context.
///
/// `yield` and `await` are contextual: outside generators / async functions
/// they print as bare keywords that reparse as identifiers (fresh names
/// outside the pre-parse universe, e.g. `` yield`` `` morphing a
/// `Yield`-tagged template into an identifier-tagged one). Inside a proper
/// context they round-trip and are preserved. Context is tracked through
/// every function kind (expressions, declarations, arrows, object and class
/// methods); field initializers and static blocks inherit the enclosing
/// context (fail-loud: if the parser disagrees, the subset oracle fires and
/// pinpoints the construct).
struct YieldAwaitCanonicalizer {
    generator_depth: u32,
    async_depth: u32,
}

impl YieldAwaitCanonicalizer {
    fn in_context<T>(
        &mut self,
        generator: bool,
        async_ctx: bool,
        node: &mut T,
    ) -> ControlFlow<Infallible>
    where
        T: for<'a> VisitWithMutShim<'a>,
    {
        let (prev_gen, prev_async) = (self.generator_depth, self.async_depth);
        self.generator_depth = u32::from(generator);
        self.async_depth = u32::from(async_ctx);
        let flow = node.visit_shim(self);
        self.generator_depth = prev_gen;
        self.async_depth = prev_async;
        flow
    }
}

/// Shim so [`YieldAwaitCanonicalizer::in_context`] works over every function
/// node type without a macro.
trait VisitWithMutShim<'a> {
    fn visit_shim(&'a mut self, visitor: &mut YieldAwaitCanonicalizer) -> ControlFlow<Infallible>;
}

macro_rules! impl_shim {
    ($($ty:ty),*) => {
        $(
            impl<'a> VisitWithMutShim<'a> for $ty {
                fn visit_shim(
                    &'a mut self,
                    visitor: &mut YieldAwaitCanonicalizer,
                ) -> ControlFlow<Infallible> {
                    self.visit_with_mut(visitor)
                }
            }
        )*
    };
}

impl_shim!(
    FunctionExpression,
    FunctionDeclaration,
    GeneratorExpression,
    GeneratorDeclaration,
    AsyncFunctionExpression,
    AsyncFunctionDeclaration,
    AsyncGeneratorExpression,
    AsyncGeneratorDeclaration,
    ArrowFunction,
    AsyncArrowFunction,
    ObjectMethodDefinition,
    ClassElement
);

impl VisitorMut<'_> for YieldAwaitCanonicalizer {
    type BreakTy = Infallible;

    fn visit_expression_mut(&mut self, node: &mut Expression) -> ControlFlow<Self::BreakTy> {
        // Loop: the replacement can itself be an out-of-context node
        // (`await await x`), and plain `visit_with_mut` recursion would
        // dispatch it past this match via the default `visit_await_mut`.
        // Each iteration strictly shrinks the node (the replacement is a
        // former descendant), so the loop terminates.
        loop {
            match node {
                Expression::Yield(y) if self.generator_depth == 0 => {
                    if let Some(target) = y.target() {
                        *node = target.clone();
                    } else {
                        let span = y.span();
                        *node = Expression::Literal(Literal::new(0.0, span));
                    }
                }
                Expression::Await(a) if self.async_depth == 0 => {
                    *node = a.target().clone();
                }
                _ => break,
            }
        }
        node.visit_with_mut(self)
    }

    fn visit_function_expression_mut(
        &mut self,
        node: &mut FunctionExpression,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(false, false, node)
    }

    fn visit_function_declaration_mut(
        &mut self,
        node: &mut FunctionDeclaration,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(false, false, node)
    }

    fn visit_generator_expression_mut(
        &mut self,
        node: &mut GeneratorExpression,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(true, false, node)
    }

    fn visit_generator_declaration_mut(
        &mut self,
        node: &mut GeneratorDeclaration,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(true, false, node)
    }

    fn visit_async_function_expression_mut(
        &mut self,
        node: &mut AsyncFunctionExpression,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(false, true, node)
    }

    fn visit_async_function_declaration_mut(
        &mut self,
        node: &mut AsyncFunctionDeclaration,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(false, true, node)
    }

    fn visit_async_generator_expression_mut(
        &mut self,
        node: &mut AsyncGeneratorExpression,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(true, true, node)
    }

    fn visit_async_generator_declaration_mut(
        &mut self,
        node: &mut AsyncGeneratorDeclaration,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(true, true, node)
    }

    fn visit_arrow_function_mut(&mut self, node: &mut ArrowFunction) -> ControlFlow<Self::BreakTy> {
        self.in_context(false, false, node)
    }

    fn visit_async_arrow_function_mut(
        &mut self,
        node: &mut AsyncArrowFunction,
    ) -> ControlFlow<Self::BreakTy> {
        self.in_context(false, true, node)
    }

    fn visit_object_method_definition_mut(
        &mut self,
        node: &mut ObjectMethodDefinition,
    ) -> ControlFlow<Self::BreakTy> {
        match node.kind() {
            MethodDefinitionKind::Generator => self.in_context(true, false, node),
            MethodDefinitionKind::Async => self.in_context(false, true, node),
            MethodDefinitionKind::AsyncGenerator => self.in_context(true, true, node),
            MethodDefinitionKind::Get
            | MethodDefinitionKind::Set
            | MethodDefinitionKind::Ordinary => self.in_context(false, false, node),
        }
    }

    fn visit_class_element_mut(&mut self, node: &mut ClassElement) -> ControlFlow<Self::BreakTy> {
        // Methods push their kind's context; fields and static blocks inherit
        // the enclosing context (see the struct docs).
        if let ClassElement::MethodDefinition(method) = node {
            match method.kind() {
                MethodDefinitionKind::Generator => self.in_context(true, false, node),
                MethodDefinitionKind::Async => self.in_context(false, true, node),
                MethodDefinitionKind::AsyncGenerator => self.in_context(true, true, node),
                MethodDefinitionKind::Get
                | MethodDefinitionKind::Set
                | MethodDefinitionKind::Ordinary => self.in_context(false, false, node),
            }
        } else {
            node.visit_with_mut(self)
        }
    }
}

/// Rewrite variable declarations with empty binding lists.
///
/// Derived `Arbitrary` bypasses [`VariableList::new`][new] (which refuses
/// empty lists, so the parser never emits one) and produces degenerate
/// declarations that print as bare contextual keywords: `for (let;;)` or a
/// bare `let;` statement reparse as the *identifier* `let` — a fresh name
/// outside the pre-parse universe. `var`/`const` empties fail closed instead
/// (reserved words → parse error → rejection), but they are rewritten too
/// for uniformity. Replacement is the `0` expression statement.
///
/// [new]: boa_ast::declaration::VariableList::new
struct EmptyDeclarationCanonicalizer;

impl EmptyDeclarationCanonicalizer {
    fn zero_expression() -> Expression {
        Expression::Literal(Literal::new(0.0, Span::new((1, 1), (1, 1))))
    }

    fn lexical_is_empty(lexical: &LexicalDeclaration) -> bool {
        AsRef::<[boa_ast::declaration::Variable]>::as_ref(lexical.variable_list()).is_empty()
    }
}

impl VisitorMut<'_> for EmptyDeclarationCanonicalizer {
    type BreakTy = Infallible;

    fn visit_statement_list_item_mut(
        &mut self,
        node: &mut StatementListItem,
    ) -> ControlFlow<Self::BreakTy> {
        match node {
            StatementListItem::Declaration(decl)
                if matches!(decl.as_ref(), Declaration::Lexical(lexical)
                    if Self::lexical_is_empty(lexical)) =>
            {
                *node = StatementListItem::Statement(
                    Statement::Expression(Self::zero_expression()).into(),
                );
            }
            StatementListItem::Statement(stmt) if matches!(stmt.as_ref(), Statement::Var(var) if var.0.as_ref().is_empty()) =>
            {
                *node = StatementListItem::Statement(
                    Statement::Expression(Self::zero_expression()).into(),
                );
            }
            _ => {}
        }
        node.visit_with_mut(self)
    }

    fn visit_for_loop_initializer_mut(
        &mut self,
        node: &mut ForLoopInitializer,
    ) -> ControlFlow<Self::BreakTy> {
        match node {
            ForLoopInitializer::Lexical(init) if Self::lexical_is_empty(init.declaration()) => {
                *node = ForLoopInitializer::Expression(Self::zero_expression());
            }
            ForLoopInitializer::Var(var) if var.0.as_ref().is_empty() => {
                *node = ForLoopInitializer::Expression(Self::zero_expression());
            }
            _ => {}
        }
        node.visit_with_mut(self)
    }
}

/// Run every idempotency-domain canonicalization pass over `ast`.
pub fn canonicalize_for_idempotency(ast: &mut StatementList, interner: &mut Interner) {
    let _: ControlFlow<Infallible> = NanCanonicalizer.visit_statement_list_mut(ast);
    let empty = interner.get_or_intern("");
    let _: ControlFlow<Infallible> = TemplateCanonicalizer { empty }.visit_statement_list_mut(ast);
    let _: ControlFlow<Infallible> = YieldAwaitCanonicalizer {
        generator_depth: 0,
        async_depth: 0,
    }
    .visit_statement_list_mut(ast);
    let _: ControlFlow<Infallible> = EmptyDeclarationCanonicalizer.visit_statement_list_mut(ast);
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrary::{Arbitrary, Unstructured};
    use boa_ast::{
        declaration::{VarDeclaration, Variable, VariableList},
        statement::iteration::ForLoopInitializerLexical,
    };
    use boa_ast::{
        expression::{Await, Yield},
        scope::Scope,
        Expression, Statement, StatementListItem,
    };
    use boa_interner::ToInternedString;
    use boa_parser::{Parser, Source};

    fn span() -> boa_ast::Span {
        boa_ast::Span::new((1, 1), (1, 2))
    }

    fn wrap(expr: Expression) -> StatementList {
        StatementList::new(
            [StatementListItem::Statement(
                Statement::Expression(expr).into(),
            )],
            boa_ast::LinearPosition::new(0),
            false,
        )
    }

    fn wrap_item(item: StatementListItem) -> StatementList {
        StatementList::new([item], boa_ast::LinearPosition::new(0), false)
    }

    /// Build the degenerate empty list exactly the way production does:
    /// derived `Arbitrary` bypassing `VariableList::new` (which would
    /// refuse it).
    fn empty_variable_list() -> VariableList {
        VariableList::arbitrary(&mut Unstructured::new(&[]))
            .expect("empty input must derive an empty list")
    }

    #[test]
    fn nan_literal_maps_to_zero() {
        let mut interner = Interner::default();
        let expr = Expression::Literal(Literal::new(f64::NAN, span()));
        let mut ast = wrap(expr);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert!(
            !printed.contains("NaN"),
            "NaN must be canonicalized, printed: {printed}"
        );
        assert!(printed.contains('0'), "expected zero, printed: {printed}");
    }

    #[test]
    fn empty_tagged_template_gains_one_element() {
        let mut interner = Interner::default();
        let tag = Expression::Literal(Literal::new(0.0, span()));
        let template = TaggedTemplate::new(tag, [].into(), [].into(), [].into(), 0, span());
        let mut ast = wrap(Expression::TaggedTemplate(template));
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "0``;\n", "printed: {printed:?}");
    }

    #[test]
    fn mismatched_tagged_arrays_truncate_to_expr_count() {
        let mut interner = Interner::default();
        let a = interner.get_or_intern("a");
        let b = interner.get_or_intern("b");
        let tag = Expression::Literal(Literal::new(0.0, span()));
        // 3 raws, 1 cooked, 0 exprs: canonical is exactly 1 raw + 1 cooked.
        let template = TaggedTemplate::new(
            tag,
            [a, b, a].into(),
            [Some(a)].into(),
            [].into(),
            0,
            span(),
        );
        let mut ast = wrap(Expression::TaggedTemplate(template));
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "0`a`;\n", "printed: {printed:?}");
    }

    #[test]
    fn empty_template_literal_gains_empty_string() {
        let mut interner = Interner::default();
        let template = TemplateLiteral::new([].into(), span());
        let mut ast = wrap(Expression::TemplateLiteral(template));
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "``;\n", "printed: {printed:?}");
    }

    #[test]
    fn bare_yield_outside_generator_maps_to_zero() {
        let mut interner = Interner::default();
        let expr = Expression::Yield(Yield::new(None, false, span()));
        let mut ast = wrap(expr);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "0;\n", "printed: {printed:?}");
    }

    #[test]
    fn yield_with_target_outside_generator_unwraps() {
        let mut interner = Interner::default();
        let target = Expression::Literal(Literal::new(5.0, span()));
        for delegate in [false, true] {
            let expr = Expression::Yield(Yield::new(Some(target.clone()), delegate, span()));
            let mut ast = wrap(expr);
            canonicalize_for_idempotency(&mut ast, &mut interner);
            let printed = ast.to_interned_string(&interner);
            assert_eq!(printed, "5;\n", "delegate={delegate} printed: {printed:?}");
        }
    }

    #[test]
    fn await_outside_async_unwraps() {
        let mut interner = Interner::default();
        let target = Expression::Literal(Literal::new(7.0, span()));
        let expr = Expression::Await(Await::new(Box::new(target), span()));
        let mut ast = wrap(expr);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "7;\n", "printed: {printed:?}");
    }

    /// Parse real source (the parser only emits in-context nodes),
    /// canonicalize, and assert the keyword survived.
    fn assert_preserved(source: &str, keyword: &str) {
        let mut interner = Interner::default();
        let mut script = Parser::new(Source::from_bytes(source))
            .parse_script(&Scope::new_global(), &mut interner)
            .unwrap_or_else(|_| panic!("test source must parse: {source}"));
        canonicalize_for_idempotency(script.statements_mut(), &mut interner);
        let printed = script.statements().to_interned_string(&interner);
        assert!(
            printed.contains(keyword),
            "{keyword} must survive in context; source: {source} printed: {printed}"
        );
    }

    #[test]
    fn yield_and_await_preserved_in_context() {
        assert_preserved("function* g() { yield 1; }", "yield");
        assert_preserved("(function* () { yield 1; });", "yield");
        assert_preserved("async function f() { await x; }", "await");
        assert_preserved("(async function () { await x; });", "await");
        assert_preserved("async function* g() { yield await x; }", "yield");
        assert_preserved("async function* g() { yield await x; }", "await");
        assert_preserved("(() => 1);", "=>");
        assert_preserved("(async () => await x);", "await");
        assert_preserved("({ *m() { yield 1; } });", "yield");
        assert_preserved("({ async m() { await x; } });", "await");
        assert_preserved("(class C { *m() { yield 1; } });", "yield");
        assert_preserved("(class C { async m() { await x; } });", "await");
    }

    #[test]
    fn nested_sync_function_resets_generator_context() {
        // The parser never emits an out-of-context `Yield`, so inject one
        // surgically: replace the single literal inside sync `h` (nested in
        // generator `g`) with a bare `Yield`, then assert canonicalization
        // rewrites it (the function boundary resets the context).
        struct Injector;
        impl VisitorMut<'_> for Injector {
            type BreakTy = Infallible;
            fn visit_expression_mut(&mut self, node: &mut Expression) -> ControlFlow<Infallible> {
                if let Expression::Literal(lit) = node {
                    let span = lit.span();
                    *node = Expression::Yield(Yield::new(None, false, span));
                }
                node.visit_with_mut(self)
            }
        }
        let mut interner = Interner::default();
        let mut script = Parser::new(Source::from_bytes(
            "function* g() { function h() { return 1; } }",
        ))
        .parse_script(&Scope::new_global(), &mut interner)
        .expect("test source must parse");
        let _: ControlFlow<Infallible> = Injector.visit_statement_list_mut(script.statements_mut());
        assert!(
            script
                .statements()
                .to_interned_string(&interner)
                .contains("yield"),
            "injection failed: no yield present before canonicalize"
        );
        canonicalize_for_idempotency(script.statements_mut(), &mut interner);
        let printed = script.statements().to_interned_string(&interner);
        assert!(
            !printed.contains("yield"),
            "yield inside nested sync function must be rewritten: {printed}"
        );
    }

    #[test]
    fn empty_let_statement_rewritten_to_zero() {
        let mut interner = Interner::default();
        let list = empty_variable_list();
        assert!(AsRef::<[Variable]>::as_ref(&list).is_empty());
        let item = StatementListItem::Declaration(
            Declaration::Lexical(LexicalDeclaration::Let(list)).into(),
        );
        let mut ast = wrap_item(item);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "0;\n", "printed: {printed:?}");
    }

    #[test]
    fn empty_var_statement_rewritten_to_zero() {
        let mut interner = Interner::default();
        let item = StatementListItem::Statement(
            Statement::Var(VarDeclaration(empty_variable_list())).into(),
        );
        let mut ast = wrap_item(item);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "0;\n", "printed: {printed:?}");
    }

    #[test]
    fn empty_lexical_for_init_rewritten_to_zero() {
        let init = ForLoopInitializer::Lexical(ForLoopInitializerLexical::new(
            LexicalDeclaration::Let(empty_variable_list()),
            Scope::new_global(),
        ));
        let mut init = init;
        let _: ControlFlow<Infallible> =
            EmptyDeclarationCanonicalizer.visit_for_loop_initializer_mut(&mut init);
        assert!(
            matches!(init, ForLoopInitializer::Expression(_)),
            "empty for-init must become an expression"
        );
    }

    #[test]
    fn non_empty_declarations_untouched() {
        let mut interner = Interner::default();
        let mut script = Parser::new(Source::from_bytes(
            "let a = 1; for (let i = 0;;) { break; }",
        ))
        .parse_script(&Scope::new_global(), &mut interner)
        .expect("test source must parse");
        canonicalize_for_idempotency(script.statements_mut(), &mut interner);
        let printed = script.statements().to_interned_string(&interner);
        assert!(
            printed.contains("let a") && printed.contains("let i"),
            "non-empty declarations must survive: {printed}"
        );
    }

    #[test]
    fn await_nested_in_with_object_rewritten() {
        // Crash repro (minimal shape): `with (await x) {}` at top level.
        let mut interner = Interner::default();
        let target = Expression::Literal(Literal::new(3.0, span()));
        let with_stmt = Statement::With(
            boa_ast::statement::With::new(
                Expression::Await(Await::new(Box::new(target), span())),
                Statement::Empty,
            )
            .into(),
        );
        let mut ast = wrap_item(StatementListItem::Statement(with_stmt.into()));
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert!(
            !printed.contains("await"),
            "out-of-context await must be rewritten: {printed}"
        );
    }

    #[test]
    fn nested_out_of_context_nodes_fully_unwrapped() {
        // Regression: `await await x` must unwrap twice. A single replace
        // exposes the inner node to default dispatch, which skips the
        // rewrite match — the loop re-examines each replacement.
        let mut interner = Interner::default();
        let inner = Expression::Await(Await::new(
            Box::new(Expression::Literal(Literal::new(3.0, span()))),
            span(),
        ));
        let outer = Expression::Await(Await::new(Box::new(inner), span()));
        let mut ast = wrap(outer);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "3;\n", "printed: {printed:?}");

        // Mixed nesting: `yield await x` outside both contexts.
        let inner = Expression::Await(Await::new(
            Box::new(Expression::Literal(Literal::new(4.0, span()))),
            span(),
        ));
        let outer = Expression::Yield(Yield::new(Some(inner), false, span()));
        let mut ast = wrap(outer);
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "4;\n", "printed: {printed:?}");
    }

    #[test]
    fn expression_trailing_literal_gains_trailer() {
        let mut interner = Interner::default();
        let a = interner.get_or_intern("a");
        let template = TemplateLiteral::new(
            [
                TemplateElement::String(a),
                TemplateElement::Expr(Expression::Literal(Literal::new(1.0, span()))),
            ]
            .into(),
            span(),
        );
        let mut ast = wrap(Expression::TemplateLiteral(template));
        canonicalize_for_idempotency(&mut ast, &mut interner);
        let printed = ast.to_interned_string(&interner);
        assert_eq!(printed, "`a${1}`;\n", "printed: {printed:?}");
    }
}
