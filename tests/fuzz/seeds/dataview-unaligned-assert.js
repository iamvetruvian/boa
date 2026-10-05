// P4 crash-gate merge-back seed: small unaligned atomic read (dev-only
// assertion class, fixed by a6101fe1). Sentry shape for the class.
var view = new DataView(new SharedArrayBuffer(9));
view.getUint8(6);
