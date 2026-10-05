// P1 seed for 9f5521bd: `new` on a non-constructor reports the value type.
function messageOf(f) {
  try {
    f();
  } catch (e) {
    return e.message;
  }
  return "<no throw>";
}

assertEquals(messageOf(() => new (42)()), "number is not a constructor", "number");
assertEquals(messageOf(() => new ("s")()), "string is not a constructor", "string");
assertEquals(messageOf(() => new (undefined)()), "undefined is not a constructor", "undefined");
assertEquals(messageOf(() => new (true)()), "boolean is not a constructor", "boolean");
assertEquals(
  messageOf(() => new (() => {})()),
  "function is not a constructor",
  "arrow function"
);
