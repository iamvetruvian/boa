// P3 differential: %RegExp.prototype%.lastIndex must be writable-only
// (non-enumerable, non-configurable).
var desc = Object.getOwnPropertyDescriptor(RegExp.prototype, "lastIndex");
assertEquals(desc.enumerable, false, "lastIndex non-enumerable");
assertEquals(desc.configurable, false, "lastIndex non-configurable");
assertEquals(desc.writable, true, "lastIndex writable");
