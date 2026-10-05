var sentinel = Promise.resolve(42);
if (Promise.try(function () { return sentinel; }) !== sentinel) {
    throw new Error("Promise.try wrapped a same-constructor promise");
}
