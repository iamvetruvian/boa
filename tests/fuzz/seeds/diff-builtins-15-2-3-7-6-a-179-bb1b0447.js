// Differential minimization: builtins/test/built-ins/Object/defineProperties/15.2.3.7-6-a-179 (boa-bug).
var arr = [];

Object.defineProperties(arr, {
  "4294967294": {
    value: 100
  }
});
