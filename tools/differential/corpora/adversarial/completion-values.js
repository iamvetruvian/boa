// Completion-value rendering across types (single outcome: joined string).
var results = [];
results.push(String(1 + 2));
results.push(String("a" + "b"));
results.push(String(null));
results.push(String(undefined));
results.push(String(true));
results.push(String(10n + 5n));
results.push(typeof function named() {});
results.push(String([1, 2, 3]));
results.join("|");
