// P3 differential: no `TypedArray` global (SS 18 lists none; %TypedArray%
// is intrinsic-only). The engine used to install it as a global binding.
assertEquals(typeof TypedArray, "undefined", "no TypedArray global");
assertEquals(globalThis.TypedArray, undefined, "no globalThis.TypedArray");
// The intrinsic itself stays reachable through its subclasses.
assertEquals(
  typeof Object.getPrototypeOf(Uint8Array),
  "function",
  "%TypedArray% reachable",
);
assert(
  Object.getPrototypeOf(Uint8Array) === Object.getPrototypeOf(Float64Array),
  "shared %TypedArray%",
);
