// Crash-drill input: class-scope-index (reverted fix 46e92c75).
// Pre-fix: index-out-of-bounds panic (len 0, index 0) in scope analysis.
new (class{constructor(){class D{}}})();
