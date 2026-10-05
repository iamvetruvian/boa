// P4 fuzz probe (OPEN): bare `using;` is an identifier reference
// (V8 + jsshell agree: ReferenceError), not a declaration. Boa throws
// SyntaxError: the `using` entry lacks the `let`-style lookahead gate
// (statement/mod.rs dispatches Keyword::Using unconditionally).
// (Module-goal `await using;` deliberately excluded: goal-dependent.)
function assertReferenceError(src, label) {
  try {
    eval(src);
  } catch (e) {
    assertEquals(e.constructor.name, "ReferenceError", label);
    return;
  }
  throw new Error("expected ReferenceError for " + label + ": completed");
}

assertReferenceError("function f() { using; } f();", "bare-using-in-function");
assertReferenceError("using;", "bare-using-top-level");
