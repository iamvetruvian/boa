// P2 seam: with() call does one lookup with the with-object as this.
var seen = [];
var obj = {
  get f() {
    seen.push("get");
    var self = this;
    return function () { seen.push(self === obj ? "this-ok" : "this-bad"); };
  }
};
with (obj) { f(); }
seen.join(",");
