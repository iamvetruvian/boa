// Fuzz seed: Atomics.wait timeout shapes on non-suspendable agents
// (finite timeouts must return "timed-out"/"not-equal", never throw;
// only infinite waits throw).
var ia = new Int32Array(new SharedArrayBuffer(4));
Atomics.wait(ia, 0, 0, 0);
Atomics.wait(ia, 0, 0, false);
Atomics.wait(ia, 0, 0, -1);
Atomics.wait(ia, 0, 1, 10);
var bia = new BigInt64Array(new SharedArrayBuffer(8));
Atomics.wait(bia, 0, 0n, 0);
