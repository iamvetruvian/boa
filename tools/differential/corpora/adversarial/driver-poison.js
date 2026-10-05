// Driver poison-safety: replacing every intrinsic the observation
// epilogue needs must not break markers on either side (the driver
// captures intrinsics before the program runs).
Array.prototype.sort = function () { throw new Error("poisoned sort"); };
Object.keys = function () { throw new Error("poisoned keys"); };
globalThis.print = function () { throw new Error("poisoned print"); };
JSON.stringify = function () { throw new Error("poisoned json"); };
Function.prototype.call = function () { throw new Error("poisoned call"); };
var poisoned_ok = 1;
