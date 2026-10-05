// Fuzz seed: try/catch/finally completion shapes (abrupt try with
// empty/non-empty catch and finally; the completion must come from the
// handler block, never from pre-abrupt try values).
var i = 0;
try {
  do {
    if (i === 5) throw i;
    i++;
  } while (i < 10);
} catch (e) {
}
try {
  while (i < 10) {
    if (i === 5) throw i;
    i++;
  }
} catch (e) {
  e;
} finally {
}
(0, eval)("try { 1; throw 2; } catch (e) { }");
