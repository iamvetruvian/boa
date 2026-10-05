// P3 differential (OPEN: abort, not yet fixed): String() of a huge sparse
// array aborts the process. DO NOT run blindly: `new Array(4294967295)`
// is fine (sparse-safe constructor), but rendering it via join/toString
// attempts a dense 64GiB pre-allocation (`Vec::with_capacity(2*len-1)`)
// and aborts. V8 throws RangeError; jsshell spins (Timeout).
// Minimal trigger (aborts pre-fix, must throw-or-complete post-fix):
//   String(new Array(4294967295))
var x = new Array(1000);
var rendered = String(x);
assertEquals(rendered.length, 999, "holes join to separators");
assertEquals(typeof x.length, "number", "length intact");
