// Mapped (sloppy) vs unmapped (#strict variant) arguments object.
function f(a, b) {
  arguments[0] = 99;
  return a + "|" + arguments[1];
}
f(1, 2);
