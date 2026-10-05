// P3 differential: no `TypedArray` global; %TypedArray% intrinsic-only.
assertEquals(typeof TypedArray, "undefined", "no TypedArray global");
assert(
  Object.getPrototypeOf(Uint8Array) === Object.getPrototypeOf(Float64Array),
  "shared %TypedArray%",
);
