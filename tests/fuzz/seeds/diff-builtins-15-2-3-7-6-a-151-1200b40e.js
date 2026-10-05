// Differential minimization: builtins/test/built-ins/Object/defineProperties/15.2.3.7-6-a-151 (boa-bug).
var arr = [];

Object.defineProperties(arr, {
  length: {
    value: 4294967295
  }
});
