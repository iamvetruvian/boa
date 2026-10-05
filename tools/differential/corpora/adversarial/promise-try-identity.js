// P2 seam: Promise.try identity (synchronous check, no job drain needed).
// May mismatch as oracle-quirk if the pinned oracle predates Promise.try.
var p = Promise.resolve(1);
var q = Promise.try(function () { return p; });
String(p === q);
