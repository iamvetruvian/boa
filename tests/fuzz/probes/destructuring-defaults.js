// P5.4 gap probe: destructuring defaults (`JumpIfNotUndefined`, both
// arms). The default store is skipped when the extracted value is not
// undefined; array and object forms, default taken and skipped.
let [destrA = 1] = [undefined];
let [destrB = 1] = [2];
let { destrX = 1 } = {};
let { destrY = 1 } = { destrY: 2 };
globalThis.destrSum = destrA + destrB + destrX + destrY;
