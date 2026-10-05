// P3 differential: CreateBuiltinFunction defines length/name/prototype
// BEFORE methods; Boa appends them after.
var objectNames = Object.getOwnPropertyNames(Object);
assertEquals(objectNames.indexOf("length"), 0, "Object length first");
assertEquals(objectNames.indexOf("name"), 1, "Object name second");
var promiseNames = Object.getOwnPropertyNames(Promise);
assertEquals(promiseNames.indexOf("length"), 0, "Promise length first");
assertEquals(promiseNames.indexOf("name"), 1, "Promise name second");
var protoNames = Object.getOwnPropertyNames(Function.prototype);
assertEquals(protoNames.indexOf("length"), 0, "F.p length first");
assertEquals(protoNames.indexOf("name"), 1, "F.p name second");
