// Symbol.toPrimitive hint order; symbol keys invisible to Object.keys.
var log = [];
var o = { [Symbol.toPrimitive]: function (hint) { log.push(hint); return 42; } };
var sum = o + 1;
var s = Symbol("s");
var o2 = { [s]: 1, plain: 2 };
sum + "|" + log.join(",") + "|" + Object.keys(o2).join(",") + "|" + (o2[s] === 1);
