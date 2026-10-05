// Hoisting: function declarations vs var (undefined until assigned).
var out = [];
out.push(typeof hoistedFunc);
out.push(hoistedVar);
function hoistedFunc() { return 1; }
var hoistedVar = "v";
out.join("|");
