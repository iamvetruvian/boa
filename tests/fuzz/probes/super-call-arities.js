// P5.4 gap probe: `SuperCall|argv-4` through `SuperCall|argv-7` plus
// `SuperCall|argv-many`. No super-call in the battery or Test262 static
// feeds passes more than 3 arguments, so these arity cells need a direct
// probe (one call per arity; 9 args land in the `many` bucket).
class A {
    constructor() {}
}
class B extends A {
    constructor(n) {
        if (n === 0) { super(1, 2, 3, 4); }
        else if (n === 1) { super(1, 2, 3, 4, 5); }
        else if (n === 2) { super(1, 2, 3, 4, 5, 6); }
        else if (n === 3) { super(1, 2, 3, 4, 5, 6, 7); }
        else { super(1, 2, 3, 4, 5, 6, 7, 8, 9); }
    }
}
new B(0);
new B(1);
new B(2);
new B(3);
new B(4);
