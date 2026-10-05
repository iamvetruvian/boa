// P3 differential: RegExp.prototype.toString must emit the source text
// verbatim (already-escaped `/` stays single-escaped).
var re = new RegExp("<(\\/)?([^<>]+)>");
assertEquals(String(re), "/<(\\/)?([^<>]+)>/", "regexp toString preserves source escapes");
