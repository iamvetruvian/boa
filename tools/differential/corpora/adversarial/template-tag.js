// Template tag: cooked vs raw string arrays.
function tag(parts) {
  var cooked = parts.join("|");
  var raw = parts.raw.join("|");
  return cooked + "#" + raw;
}
var name = "Bob";
tag`hello ${name}\nend`;
