// P5.4 gap probe: async-generator abrupt resumption
// (`JumpIfNotEqual|nottaken`).
//
// Besides the `super` home-object check (whose taken arm the
// fused-branches probe covers), `JumpIfNotEqual` dispatches async
// `yield` resume kinds: resuming with the awaited kind falls through
// (nottaken). A plain `for await` loop only takes the jumps, so this
// probe drives `.return()` and `.throw()` into a suspended async
// generator.
async function* asyncGen() {
    try {
        yield 1;
        yield 2;
    } finally {
        yield 3;
    }
}
async function drive() {
    let a1 = asyncGen();
    await a1.next();
    await a1.return(9);
    let a2 = asyncGen();
    await a2.next();
    try {
        await a2.throw(new Error("boom"));
    } catch (e) {
        globalThis.asyncAbrupt = "caught";
    }
    let a3 = asyncGen();
    for await (const value of a3) {
        globalThis.asyncLast = value;
    }
}
drive();
