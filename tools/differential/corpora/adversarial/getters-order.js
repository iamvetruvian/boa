// Getter evaluation order in binary expressions.
var log = [];
var o = {};
Object.defineProperty(o, "a", { enumerable: true, get: function () { log.push("a"); return 1; } });
Object.defineProperty(o, "b", { enumerable: true, get: function () { log.push("b"); return 2; } });
o.a + o.b + "|" + log.join("");
