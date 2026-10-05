// Class field order: statics at definition, instance fields before ctor body.
var log = [];
class A {
  a = (log.push("a"), 1);
  static b = (log.push("static-b"), 2);
  c = (log.push("c"), 3);
  constructor() { log.push("ctor"); }
}
new A();
log.join(",");
