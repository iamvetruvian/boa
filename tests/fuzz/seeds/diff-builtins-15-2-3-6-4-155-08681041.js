// Differential minimization: builtins/test/built-ins/Object/defineProperty/15.2.3.6-4-155 (boa-bug).
var arrObj = [];

Object.defineProperty(arrObj, "length", {
  value: 4294967295
});
