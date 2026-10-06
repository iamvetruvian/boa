# Regression database

Every fixed bug keeps a reproducer that must stay green. `cargo test -p
boa_regression` (part of the default workspace test run) executes each
`repro.js` in `regressions.toml` and fails on any red entry. Add one entry
per fixed bug — a fix without a regression entry is incomplete.

## Entry schema (`regressions.toml`)

| Field               | Required       | Meaning                                                                                                 |
| ------------------- | -------------- | ------------------------------------------------------------------------------------------------------- |
| `id`                | yes            | Stable kebab-case id; doubles as the `cases/<id>/` directory                                            |
| `title`             | yes            | One-line bug description                                                                                |
| `status`            | yes            | `landed` (executes) or `open` (documents unlanded work; validated but skipped)                          |
| `reproducer`        | yes            | Path to `repro.js`, relative to this directory                                                          |
| `expect`            | yes            | `pass`, or `throw <ErrorName>` for must-throw repros                                                    |
| `layer`             | yes            | Responsible layer, e.g. `engine/vm/runtime-limits`                                                      |
| `unit_test`         | yes            | Responsible-layer Rust test (`path::to::file.rs::test_name`); the file must exist                       |
| `test262_style`     | one of the two | Nearest Test262 path (file or dir); checked when a checkout exists                                      |
| `no_test262_reason` | one of the two | Written justification when no Test262 test covers the bug                                               |
| `fuzz_seed`         | yes            | Raw-JS corpus seed for this bug class, staged under `tests/fuzz/seeds/` (P4 wires seeds into harnesses) |
| `symptom`           | yes            | Observable misbehavior                                                                                  |
| `root_cause`        | yes            | Why it happened                                                                                         |
| `spec`              | no             | Spec section URL (omit for host-defined behavior like runtime limits)                                   |
| `fix_commit`        | yes            | Commit that fixed it                                                                                    |
| `finder`            | yes            | Who/what found it (person, fuzzer, `P1-seed`, …)                                                        |
| `added`             | yes            | Entry date, `YYYY-MM-DD`                                                                                |

Reproducers run in a default engine context with two globals injected:
`assert(cond, message?)` and `assertEquals(actual, expected, message?)`
(`===` semantics). Keep repros minimal and deterministic — no dates,
randomness, locales, or I/O.

## Admission rules

1. **Reproduce first.** A repro lands only after it is observed failing on
   the pre-fix code (or, for P1 seeds taken from already-landed fixes, after
   the linked unit test's pre-fix failure is confirmed from the fix commit)
   and passing on the fixed code. Unreproducible reports stay out; if they
   must be tracked, land them as `status = "open"` with no execution.
2. **One unit test at the responsible layer.** The `unit_test` field must
   name a real Rust test next to the fixed code — the JS repro guards the
   symptom, the unit test guards the mechanism. Bugs that cannot be observed
   from JS cannot enter the DB: unconstructible host objects (e.g.
   `IsHTMLDDA`) and engine errors that bypass JS `try`/`catch` by design
   (e.g. runtime-limit trips) stay covered by their Rust tests only.
3. **Triage links.** `test262_style` (or a written `no_test262_reason`)
   and `fuzz_seed` connect the entry to the suites that cover its area;
   P3.4 additionally files every entry's id in the triage queue.
4. **Four-artifact audit.** `scripts/audit-fix.sh <id>` verifies the P2
   fix-loop artifacts — reproducer, responsible-layer unit test,
   Test262-style test or justification, fuzz seed — and that the whole
   DB is green. Run it before landing a fix.

## Shared JSON-store conventions (P1.1)

All machine-readable stores in this program (`regressions.toml` manifests,
`crashes.json`, the P2.1 conformance-gap matrix, the P3.4 triage queue,
future differential stores) share these conventions:

- `schema_version: 1` integer; readers reject unknown versions loudly.
- `tool` + `tool_version` identify the writer (`boa_regression`,
  `boa_tester`, …); TOML manifests use `tool` + `updated`.
- `updated` is UTC RFC 3339 (`2026-10-02T00:00:00Z`).
- Test ids are test262-relative paths without extension joined with `/`
  (e.g. `test/built-ins/Array/from`), identical to tester crash records.
- Outcomes use the tester letters `O/I/F/P/T/C/H` (pass, ignored, failed,
  panic, timeout, crash, harness error); JSON stores spell them out only in
  prose fields, never as keys.
- Spec links are full `https://tc39.es/…` fragment URLs.

## Importing from the issue tracker

1. Pick an issue with a reproducer; minimize the repro to a few lines.
2. Confirm it fails on current `main` and passes with the fix (rule 1).
3. Add the responsible-layer unit test, then the DB entry + `cases/<id>/repro.js`.
4. Run `cargo test -p boa_regression` and land entry + fix together.
