// P4-fuzzilli (OPEN): String.prototype.at with huge negative indices.
// Post-fix ("-9223372036854775807" -> undefined) the full-scale shape is inert.
// Kept below the INT64_MIN f64-rounding boundary so the seed itself never
// aborts the fleet; scale comes from the fuzzer's own mutations.
("function").at("-9007199254740993");
("function").at(-9007199254740993);
