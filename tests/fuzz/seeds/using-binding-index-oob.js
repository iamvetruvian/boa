// P4-fuzzilli (OPEN): `using` declarations near function/script top scope.
// Post-fix the top-scope shape throws TypeError for non-disposables; the
// block-scoped shape below already works and must keep working. Staged
// block-scoped so the seed itself never aborts the fleet.
function seedUsing() { { using v = null; } }
seedUsing();
