// P7.1c gap probe: `ic|to-mono` through `ic|to-mega`. One access site
// (`o.p` below) sees five distinct shapes, walking a single inline cache
// empty→mono→poly(2..4)→megamorphic. Generated feeds never stack five
// shapes on one site, so these cells need a direct probe.
function read_p(o) { return o.p; }
read_p({ p: 1, a: 1 });
read_p({ p: 2, b: 2 });
read_p({ p: 3, c: 3 });
read_p({ p: 4, d: 4 });
read_p({ p: 5, e: 5 });
