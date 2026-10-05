// P3 differential (OPEN): reduced trigger for the huge-sparse-join abort.
// Post-fix this must throw RangeError (or complete), never abort.
// Kept small on purpose: the seed must not OOM the fuzz fleet; scale comes
// from the fuzzer's own mutations, not from a 4G literal here.
String(new Array(100000));
