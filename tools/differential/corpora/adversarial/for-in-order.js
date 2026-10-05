// for-in order: integer-like keys ascending, then insertion order.
var o = {};
o.b = 1; o[2] = 2; o.a = 3; o[10] = 4; o[1] = 5;
var keys = [];
for (var k in o) { keys.push(k); }
keys.join(",");
