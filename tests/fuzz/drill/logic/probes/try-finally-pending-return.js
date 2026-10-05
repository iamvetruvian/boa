// Logic-drill probe for try-finally-pending-return (970ea933).
// A break out of an inner finally must discard the inner pending return,
// not the outer one. Fixed + oracle: "42|43".
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
function g() {
  try {
    return 42;
  } finally {
    return 43;
  }
}
console.log(f() + "|" + g());
