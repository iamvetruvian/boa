// P3 differential: try/catch completion must be the catch-block
// completion (empty here), not the pre-abrupt try-block value.
var completion = (0, eval)(
  "var i = 0;\n" +
  "try {\n" +
  "  do {\n" +
  "    if (i === 5) throw i;\n" +
  "    i++;\n" +
  "  } while (i < 10);\n" +
  "} catch (e) {\n" +
  "  if (e !== 5) throw e;\n" +
  "}"
);
assertEquals(completion, undefined, "try/catch completion is catch-block completion (empty)");
