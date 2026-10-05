// P3 differential: EscapeRegExpPattern must not re-escape an already-escaped
// solidus from literal source text.
assertEquals(/\//.source, "\\/", "literal slash source");
assertEquals(/\//.source.length, 2, "literal slash source length");
// Control: the constructor path is already correct.
assertEquals(new RegExp("/").source, "\\/", "constructor slash source");
