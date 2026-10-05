// Fuzz seed: class and function source-text rendering (toString must
// return source text; classes never render as native code).
class SeedClass {
  constructor() {}
  method() {}
  static sm() {}
  get g() { return 1; }
}
String(SeedClass);
String(SeedClass.prototype.method);
function seedFn(a, b) { return a + b; }
String(seedFn);
