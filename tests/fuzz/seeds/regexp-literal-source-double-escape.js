// Fuzz seed: RegExp .source escaping across literal/constructor paths.
var r1 = /\//;
var s1 = r1.source;
var r2 = /[/]/;
var s2 = r2.source;
var r3 = new RegExp("/");
var s3 = r3.source;
