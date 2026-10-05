// P4 fuzz probe (OPEN): binding patterns without initializers must throw
// SyntaxError (V8 + jsshell agree on all six). Boa parses them (reached code
// throws TypeError at runtime instead). Post-fix these assertions pass; the
// entry flips to landed.
function assertSyntaxError(src, label) {
  try {
    eval(src);
  } catch (e) {
    assertEquals(e.constructor.name, "SyntaxError", label);
    return;
  }
  throw new Error("expected SyntaxError for " + label + ": completed");
}

assertSyntaxError("var [];", "var-array-empty");
assertSyntaxError("let [a];", "let-array");
assertSyntaxError("var {};", "var-object-empty");
assertSyntaxError("let {};", "let-object-empty");
assertSyntaxError("for (var [a];;);", "for-var-array");
assertSyntaxError("let [a, , ];", "let-array-elision");
