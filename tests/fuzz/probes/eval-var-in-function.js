// P5.4 gap probe (DYNAMIC-ONLY by construction): `DefEvalVar`.
//
// Emitted during eval declaration instantiation step 18.b, which runs when
// runtime-compiled eval code declares vars in a non-global environment.
// Static scans never see it (the eval string compiles at runtime); global
// scope takes step 18.a instead, so the eval must sit in a function.
function f() { eval("var q = 1;"); return q; }
f();
