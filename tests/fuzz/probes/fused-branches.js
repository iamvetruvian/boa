// P5.4 gap probe: fused comparison branches (`JumpIfNotLessThanOrEqual`
// + `JumpIfNotGreaterThanOrEqual`, both arms each).
//
// The bytecompiler fuses relational comparisons in branch position into
// a single jump (`try_fused_comparison_branch`); bare `a <= b;` expression
// statements emit the value opcodes instead. The battery only fuses `<`,
// `>` (and `===`/`!==` never fuse), so `<=`/`>=` arms need a direct
// probe with both outcomes each. Unoptimized: the optimizer would fold
// these literal comparisons away.
if (1 <= 2) { globalThis.p1 = 1; } else { globalThis.p1 = 2; }
if (3 <= 2) { globalThis.p2 = 1; } else { globalThis.p2 = 2; }
if (3 >= 2) { globalThis.p3 = 1; } else { globalThis.p3 = 2; }
if (1 >= 2) { globalThis.p4 = 1; } else { globalThis.p4 = 2; }
// `super` home-object check (`JumpIfNotEqual`, taken arm: valid home).
class FusedBase {
    method() { return 1; }
}
class FusedDerived extends FusedBase {
    call() { return super.method(); }
}
new FusedDerived().call();
