// Array holes: length, join, and forEach skipping.
var a = [1, , 3];
var seen = [];
a.forEach(function (v, i) { seen.push(i + ":" + v); });
a.length + "|" + a.join("-") + "|" + seen.join(",");
