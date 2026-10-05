// Error cause chains and instanceof through the cause option.
var e = new TypeError("inner", { cause: "root-cause" });
e.cause + "|" + (e instanceof Error);
