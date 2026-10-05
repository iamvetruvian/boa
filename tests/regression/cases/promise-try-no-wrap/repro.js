var sentinel = Promise.resolve(42);
assert(
    Promise.try(function () { return sentinel; }) === sentinel,
    'Promise.try must not wrap same-constructor promises'
);
