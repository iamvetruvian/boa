// Logic-drill probe for string-tonumber-signed-infinity (a93e0b3f).
// Fixed + oracle: "Infinity|NaN|NaN|16|NaN".
console.log(
  [
    Number("+Infinity"),
    Number("inf"),
    Number("0x+1"),
    Number("0x10"),
    Number("0b-1"),
  ].join("|")
);
