// Destructuring defaults evaluate only when needed, left to right.
var log = [];
function d() { log.push("d"); return 7; }
var [a = d(), b = 2] = [1];
var { x = d(), y = 3 } = {};
[a, b, x, y].join(",") + "|" + log.join("");
