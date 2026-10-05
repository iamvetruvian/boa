// BigInt arithmetic, typeof, Number mixing (TypeError), parsing.
var out = [];
out.push(String(10n + 5n));
out.push(typeof (10n * 2n));
try { 10n + 1; out.push("no-throw"); } catch (e) { out.push(e.constructor.name); }
try { BigInt("12"); out.push("parsed"); } catch (e) { out.push("threw"); }
out.join("|");
