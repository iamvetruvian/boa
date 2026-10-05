// Crash-drill input: dataview-unaligned-assert (reverted fix a6101fe1).
// Pre-fix: over-strict debug_assert! in batched atomic copy (dev-only);
// shape from test262 SharedArrayBuffer/init-zero (harness stripped).
// Index 6 is load-bearing: the assert depends on dest alignment, and only
// index 6 of 0-8 trips it (verified by probe sweep on the broken tree).
var view = new DataView(new SharedArrayBuffer(9));
view.getUint8(6);
