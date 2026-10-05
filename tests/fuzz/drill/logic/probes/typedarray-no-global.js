// Logic-drill probe for typedarray-no-global (uncommitted fix).
// No `TypedArray` global (spec SS 18; intrinsic-only). Fixed + oracles:
// "undefined|function".
console.log(typeof TypedArray + "|" + typeof Object.getPrototypeOf(Uint8Array));
