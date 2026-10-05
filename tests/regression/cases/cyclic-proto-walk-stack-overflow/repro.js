// Proto cycles through a Proxy are spec-mandated: OrdinarySetPrototypeOf
// stops its cycle walk at non-ordinary objects (test262
// test/built-ins/Object/prototype/__proto__/set-cycle-shadowed.js asserts
// the set succeeds). Walking the resulting cycle must surface a catchable
// error (V8: RangeError, jsshell: InternalError) — never abort the process.
var root = {};
var intermediary = new Proxy(Object.create(root), {});
var leaf = Object.create(intermediary);
root.__proto__ = leaf;
var touched = leaf.toString;
