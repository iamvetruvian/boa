// P3 differential: globalThis lacks @@toStringTag "global".
assertEquals(
  Object.prototype.toString.call(globalThis),
  "[object global]",
  "globalThis toStringTag"
);
