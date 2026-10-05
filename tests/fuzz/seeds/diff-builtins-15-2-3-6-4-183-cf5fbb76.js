// Differential minimization: builtins/test/built-ins/Object/defineProperty/15.2.3.6-4-183 (boa-bug).
var arrObj = [];

Object.defineProperty(arrObj, 4294967294, {
  value: 100
});
