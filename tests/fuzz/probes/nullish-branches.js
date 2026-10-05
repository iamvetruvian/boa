// P5.4 gap probe: nullish branches (`JumpIfNullOrUndefined|taken`;
// the not-taken arm already fires in the battery, whose `??` chains use
// bound non-nullish variables). Covers `??` with nullish and non-nullish
// left sides plus optional chaining.
let nul1 = null ?? 1;
let nul2 = undefined ?? 2;
let nul3 = 0 ?? 3;
let nulObj = { a: 1 };
let nulV1 = nulObj?.a;
let nulV2 = nulObj?.b?.c;
globalThis.nulSum = nul1 + nul2 + nul3 + nulV1 + (nulV2 === undefined ? 0 : 1);
