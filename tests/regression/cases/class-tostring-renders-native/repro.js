// P3 differential: Function.prototype.toString on a class must return
// class source text, not a native-code placeholder.
class TestClass {
  constructor() {}
  method() {}
}
var rendered = String(TestClass);
assert(
  rendered.slice(0, 5) === "class",
  "class toString starts with 'class': " + rendered.slice(0, 40)
);
