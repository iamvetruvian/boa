// Loop capture: shared var binding vs per-iteration let binding.
var funcs = [];
for (var i = 0; i < 3; i++) { funcs.push(function () { return i; }); }
var lets = [];
for (let j = 0; j < 3; j++) { lets.push(function () { return j; }); }
funcs.map(function (f) { return f(); }).join("") + "|" + lets.map(function (f) { return f(); }).join("");
