// P4 fuzz probe seed: bare `using;` is an identifier (ReferenceError).
function f() { using; } f();
