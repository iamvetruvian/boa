// Proxy get-trap argument order and fallthrough values.
var log = [];
var p = new Proxy({ x: 1 }, {
  get: function (t, k) { log.push("get:" + String(k)); return t[k]; }
});
p.x;
p.y;
log.join("|");
