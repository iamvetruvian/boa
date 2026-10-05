// P2 seam: StringToNumber signed-Infinity and signed-radix spellings.
var cases = ["", "   ", "0x+1", "+Infinity", "-Infinity", "Infinity", "0b2", "12abc", "  42  "];
cases.map(function (s) {
  var n = Number(s);
  return Number.isNaN(n) ? "NaN" : String(n);
}).join("|");
