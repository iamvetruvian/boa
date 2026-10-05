// P1 seed for 970ea933: a break out of an inner finally must discard the
// inner pending return, not the outer one.
function f() {
  try {
    return 42;
  } finally {
    do try {
      return 43;
    } finally {
      break;
    } while (0);
  }
}
assertEquals(f(), 42, "outer return survives inner break");

function g() {
  try {
    return 42;
  } finally {
    return 43;
  }
}
assertEquals(g(), 43, "finally return still wins");
