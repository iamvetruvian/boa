// Fuzz seed: builtin own-property order (length/name/prototype placement).
Object.getOwnPropertyNames(Object);
Object.getOwnPropertyNames(Promise);
Object.getOwnPropertyNames(Function.prototype);
Object.getOwnPropertyNames(Function);
