// Number-to-string edges: -0, NaN, radix, precision, exponents.
[
  String(-0),
  String(NaN),
  (255).toString(16),
  (7).toString(2),
  String(1 / 3),
  String(1e21),
  String(0.1 + 0.2)
].join("|");
