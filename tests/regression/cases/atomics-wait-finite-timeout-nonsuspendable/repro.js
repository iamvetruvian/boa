// P3 differential (open/unreproducible-in-default-context): finite
// Atomics.wait on a non-suspendable agent must return "timed-out"
// without suspending (spec Suspend fast-path), not throw. Default
// contexts suspend (masking the gap); the differential's
// non-suspendable Boa context throws "agent cannot be suspended".
// See test/built-ins/Atomics/wait/false-for-timeout (B threw, O ok).
var ia = new Int32Array(new SharedArrayBuffer(4));
var result = Atomics.wait(ia, 0, 0, 0);
assertEquals(result, "timed-out", "finite wait returns timed-out");
