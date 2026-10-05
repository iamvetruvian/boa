// P5-metamorphic (OPEN): dead-branch completion shapes for the optimizer
// on/off differential. Under the optimizer each completes the leaked prior
// value instead of `undefined` (DCE folds to `Empty`); unoptimized they
// complete `undefined`.
0; if (false) { 1; }
0; while (false) { 1; }
0; for (;false;) { 1; }
