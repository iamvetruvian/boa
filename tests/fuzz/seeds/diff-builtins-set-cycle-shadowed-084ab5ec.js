// Differential minimization: builtins/test/built-ins/Object/prototype/__proto__/set-cycle-shadowed (boa-bug).
var root = {};
var intermediary = new Proxy(Object.create(root), {});
var leaf = Object.create(intermediary);
root.__proto__ = leaf;
