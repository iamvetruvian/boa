// OPEN (P5-metamorphic): DCE replaces a falsy `if` (no `else`), a
// never-taken `while`, and a never-entered `for` with `Empty`, which
// inherits the previous completion instead of completing `undefined`
// (ECMA-262: `undefined` for all three paths).
//
// Repro (needs the optimizer; default contexts optimize, `boa_cli` needs
// `-O`): each line below completes 0 under `-O` (node: `undefined`).
//
// Runner note: script completions are unobservable from inside the script
// (`eval`/`new Function` compile unoptimized), so this repro is
// documentary; the fix is pinned by the Rust unit test, like
// await-recursion.
0; if (false) { 1; }
0; while (false) { 1; }
0; for (;false;) { 1; }
