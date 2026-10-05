// P1 seed for a93e0b3f: only `Infinity`, `+Infinity`, `-Infinity` spell
// infinity, and non-decimal literals take no sign after the prefix.
assertEquals(Number("Infinity"), Infinity, "bare Infinity");
assertEquals(Number("+Infinity"), Infinity, "signed +Infinity");
assertEquals(Number("-Infinity"), -Infinity, "signed -Infinity");

[
  "inf", "INF", "Inf", "infinity",
  "+inf", "-inf", "+Inf", "-Inf", "+INF", "-INF",
  "+infinity", "-infinity", "+INFINITY", "-INFINITY", "+iNfInItY",
].forEach((s) => assert(Number.isNaN(Number(s)), "NaN for " + s));

assertEquals(Number("0x10"), 16, "hex");
assertEquals(Number("0X10"), 16, "hex caps");
assertEquals(Number("0b101"), 5, "binary");
assertEquals(Number("0o17"), 15, "octal");
assertEquals(Number("0x1FFFFFFFF"), 8589934591, "wide hex");

["0x", "0b", "0o", "0x+1", "0x-1", "0x+0", "0b+1", "0b-1", "0o+7", "0o-7"].forEach(
  (s) => assert(Number.isNaN(Number(s)), "NaN for " + s)
);
