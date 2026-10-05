// Synchronous generator delegation values.
function* inner() { yield 1; yield 2; }
function* outer() { yield 0; yield* inner(); yield 3; }
var out = [];
for (var v of outer()) { out.push(v); }
out.join(",");
