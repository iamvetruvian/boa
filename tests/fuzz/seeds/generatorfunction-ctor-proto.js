// Fuzz seed: generator-constructor prototype links + name fallback after delete.
var G = Object.getPrototypeOf(function*() {}).constructor;
var isFn = Object.getPrototypeOf(G) === Function;
delete G.name;
var fallback = G.name;
