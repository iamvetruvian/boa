// P3 differential: %GeneratorFunction% and %AsyncGeneratorFunction% are
// specified as subclasses of Function (proto link is Function itself).
var GeneratorFunction = Object.getPrototypeOf(function*() {}).constructor;
var AsyncGeneratorFunction = Object.getPrototypeOf(async function*() {}).constructor;
assertEquals(Object.getPrototypeOf(GeneratorFunction), Function, "GeneratorFunction proto");
assertEquals(
  Object.getPrototypeOf(AsyncGeneratorFunction),
  Function,
  "AsyncGeneratorFunction proto"
);
// Control: AsyncFunction already links Function.
var AsyncFunction = Object.getPrototypeOf(async function() {}).constructor;
assertEquals(Object.getPrototypeOf(AsyncFunction), Function, "AsyncFunction proto");
