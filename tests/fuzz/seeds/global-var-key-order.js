// Fuzz seed for the global-var key-order bug class: declaration
// instantiation order must match source order (spec Lists, not sets).
var zkfuzz_a = 1;
var zkfuzz_b = 2;
var zkfuzz_c = 3;
Object.keys(globalThis).filter(function (k) {
  return k.indexOf("zkfuzz_") === 0;
});
eval("var zkfuzz_d = 4; var zkfuzz_e = 5;");
