// P3 differential: global `var` bindings enumerate in source order.
var zkdb_a = 0;
var zkdb_b = 0;
var zkdb_c = 0;
assertEquals(
  Object.keys(globalThis)
    .filter(function (k) {
      return k.indexOf("zkdb_") === 0;
    })
    .join(","),
  "zkdb_a,zkdb_b,zkdb_c",
  "global var key order"
);
eval("var zkev2_a = 0; var zkev2_b = 0;");
assertEquals(
  Object.keys(globalThis)
    .filter(function (k) {
      return k.indexOf("zkev2_") === 0;
    })
    .join(","),
  "zkev2_a,zkev2_b",
  "global eval var key order"
);
