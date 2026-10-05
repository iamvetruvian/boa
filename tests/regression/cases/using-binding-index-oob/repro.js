// OPEN (P4-fuzzilli): a `using` declaration directly in a function body must
// throw TypeError for a non-disposable initializer (node v26 oracle); Boa
// aborts with a binding index-OOB in PutLexicalValue at
// core/engine/src/vm/opcode/define/mod.rs:126. Minimized from a 7-line
// class-extends-BigUint64Array find: no class, `new`, or params are needed.
function usingTopScope() { using v = usingTopScope; }
usingTopScope();
