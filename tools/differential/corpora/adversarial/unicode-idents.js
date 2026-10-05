// Unicode identifiers, escapes, surrogate-pair strings.
var caf\u00e9 = 1;
var \u{63}at = 2;
"caf\u00e9".length + "|" + "\u{1F600}".length + "|" + "\u{1F600}".codePointAt(0).toString(16) + "|" + (caf\u00e9 + \u{63}at);
