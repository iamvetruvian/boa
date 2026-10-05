// Direct eval leaks vars globally; strict direct eval contains them.
var out = [];
eval("var directVar = 11;");
out.push(directVar);
(0, eval)("var indirectVar = 22;");
out.push(indirectVar);
out.push(eval("'use strict'; var strictVar = 33; strictVar;"));
try { strictVar; out.push("leaked"); } catch (e) { out.push("contained"); }
out.join("|");
