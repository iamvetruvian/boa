// Crash-drill input: padstart-oom (reverted fix 419a8ed5).
// Pre-fix: heap allocation failure (dev builds spin to the timeout).
"a".padStart(Number.MAX_SAFE_INTEGER);
