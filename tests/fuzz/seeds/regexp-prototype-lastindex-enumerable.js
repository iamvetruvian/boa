// Fuzz seed: RegExp.prototype enumeration (lastIndex must not appear).
var keys = [];
for (var k in RegExp.prototype) {
  keys.push(k);
}
/x/.lastIndex;
