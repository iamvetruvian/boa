// OPEN (harness-side, not executed): parser name-universe drift on
// synthetic Yield-expression computed keys. The fleet's AST generator can
// build a Yield node where the enclosing context is not a generator; it
// prints as bare `[yield]`, and the reparse interns identifier `yield`
// outside the pre-parse universe (oracle assert at
// fuzz_targets/parser-idempotency.rs:52). Real-source round trips are
// stable in both contexts (generator position reparse keeps the Yield
// node; non-generator position keeps the identifier), so no JS program
// misbehaves — the generator shape is unspellable in source. Minimized
// fleet shape (from tmin, 74 -> 37 bytes):
switch ({
    async *[yield]() {},
}) {
}
