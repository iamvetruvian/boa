// P4-fuzzilli (OPEN): re-entrant await through a promise-`constructor` getter.
// Post-fix deep recursion rejects with RangeError instead of a sync
// EnginePanic. Capped at 100 levels (verified clean; 200 trips) so the seed
// itself never aborts the fleet; depth comes from the fuzzer's own mutations.
let n = 0;
const p = new Promise(() => {});
Object.defineProperty(p, "constructor", { configurable: true, get() { n++; if (n < 100) { (async () => { await p; })(); } } });
(async () => { await p; })();
