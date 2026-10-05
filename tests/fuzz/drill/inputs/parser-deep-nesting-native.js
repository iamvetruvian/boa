// Crash-drill input: parser-deep-nesting (reverted WT fix, bug #22).
// Pre-fix: native stack overflow. Depth 217 is EXACT-minimal (216 parses,
// 217 overflows; bisected on dev boa). Recalibrate if the toolchain moves
// the threshold (the drill fails closed: a non-crashing input RETIREs).
(((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((1)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))
