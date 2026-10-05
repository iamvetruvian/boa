// P2 seam: pending returns across finally/continue and nested returns.
function f() {
  var r = "none";
  for (var i = 0; i < 1; i++) {
    try { r = "try"; } finally { r += "-fin"; continue; }
  }
  return r;
}
function g() {
  try { return "outer"; }
  finally { try { return "inner"; } finally {} }
}
f() + "|" + g();
