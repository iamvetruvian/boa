// P5.4 gap probe (OPTIMIZED feed only): `StoreNan` + `StoreNegativeInfinity`.
//
// `NaN` has no literal syntax, so the unoptimized emitter can never produce
// these cells: only the optimizer's const-fold materializes NaN/-Infinity
// rationals (`0/0` folds to NaN, `-1e999` to -Infinity). Run with
// `--optimize`; the unoptimized feed covers the same file as Div/Mul/Sub.
0/0;
-1e999;
1e999 * 1e999 - 1e999 * 1e999;
