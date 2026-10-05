// Fuzz seed: global-object toStringTag rendering (direct + nested paths).
Object.prototype.toString.call(globalThis);
[globalThis, globalThis].join(",");
String(globalThis);
