// Fuzz seed: RegExp source/toString escaping (source text round-trips
// verbatim; escaped `/` and `\\` stay single).
String(new RegExp("<(\\/)?([^<>]+)>"));
String(/a\/b\\/);
new RegExp("\\\\d+\\.\\d*").source;
String(/(?:x)+?/gy);
