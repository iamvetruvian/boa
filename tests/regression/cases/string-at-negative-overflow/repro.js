// OPEN (P4-fuzzilli): String.prototype.at with an index that ToIntegerOrInfinity
// maps to INT64_MIN ("-9223372036854775807" rounds to -2^63 in f64) must return
// undefined (node v26 oracle); Boa aborts on the `(-i)` negate overflow at
// core/engine/src/builtins/string/mod.rs:541.
assertEquals(("function").at("-9223372036854775807"), undefined, "at(INT64_MIN) is undefined");
