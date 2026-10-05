// OPEN (P4-fuzzilli): re-entrant await through a promise-`constructor` getter
// past the recursion limit must not surface a sync EnginePanic (node v26
// oracle: the script completes and the inner promise rejects with RangeError).
// Context::eval does not drain the job queue, so the RangeError rejection is
// pinned by the Rust unit test; this repro guards the sync escape.
const p = new Promise(() => {});
Object.defineProperty(p, "constructor", { configurable: true, get() { (async () => { await p; })(); } });
(async () => { await p; })();
