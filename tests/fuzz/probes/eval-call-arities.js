// P5.4 gap probe: `CallEval|argv-3` through `CallEval|argv-7` plus
// `CallEval|argv-many`. Direct `eval` calls in the feeds pass at most 2
// arguments (extra args are ignored at runtime but present in the arity
// operand), so these cells need a direct probe, one call per arity.
eval("1", 2, 3);
eval("1", 2, 3, 4);
eval("1", 2, 3, 4, 5);
eval("1", 2, 3, 4, 5, 6);
eval("1", 2, 3, 4, 5, 6, 7);
eval("1", 2, 3, 4, 5, 6, 7, 8, 9);
