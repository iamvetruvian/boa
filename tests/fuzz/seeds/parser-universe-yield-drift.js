// P4 open-class seed: computed generator-method names (universe-oracle
// area, see regression entry parser-universe-yield-drift). The exact
// fleet shape (a synthetic Yield node outside generator context) cannot
// be spelled in source, so this exercises the neighboring real-source
// shapes instead. Must never crash any harness (it is plain valid JS).
var key = "k";
var o = { async *[key]() {} };
function* g() { return 1; }
g().next();
