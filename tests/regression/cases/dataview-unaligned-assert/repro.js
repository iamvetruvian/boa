// Small unaligned atomic reads must not trip copy-helper assertions.
// Pre-fix (reverted a6101fe1): dev builds aborted in
// `array_buffer/utils.rs` on this shape; post-fix it reads 0.
// Index 6 is load-bearing (dest-alignment dependent; probed 0-8).
var view = new DataView(new SharedArrayBuffer(9));
assertEquals(view.getUint8(6), 0, "unaligned small atomic read");
