// P1 seed for 5cee37eb: calls inside with() resolve through the object
// environment exactly as specified, with the with-object as `this`.
let emptyHasCount = 0;
const emptyProxy = new Proxy({}, {
  has(t, p) {
    if (p === "Object") {
      emptyHasCount++;
    }
    return Reflect.has(t, p);
  }
});
with (emptyProxy) {
  Object();
}
assertEquals(emptyHasCount, 1, "single lookup");

let hasCount = 0;
let callThis = null;
const target = {
  fn() {
    callThis = this;
  }
};
const proxy = new Proxy(target, {
  has(t, p) {
    if (p === "fn") {
      hasCount++;
    }
    return Reflect.has(t, p);
  }
});
with (proxy) {
  fn();
}
assertEquals(hasCount, 2, "call lookups");
assert(callThis === proxy, "this is the with-object");
