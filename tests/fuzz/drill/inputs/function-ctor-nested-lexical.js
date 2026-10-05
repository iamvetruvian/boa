// Crash-drill input: function-ctor-nested-lexical (reverted fix 957e6f83).
// Pre-fix: index-out-of-bounds panic in scope analysis.
Function("function f() { const a = 42; return () => a; } return f()()")();
