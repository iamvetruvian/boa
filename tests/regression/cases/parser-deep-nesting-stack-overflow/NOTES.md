# Bug #22 — Parser stack overflow on deep nesting: discovery notes

Filing: `parser-deep-nesting-stack-overflow` in `regressions.toml` (landed).
Repro: `repro.js` (100k-deep parens → `SyntaxError`, deterministic on every
stack/profile). Fuzz seed: `tests/fuzz/seeds/parser-deep-nesting-stack-overflow.js`.
Unit test: `core/parser/src/parser/tests/mod.rs::deep_nesting_hits_depth_limit`.

## Observed behavior (pre-fix)

- `boa` CLI aborted with `thread 'main' has overflowed its stack`
  (SIGABRT, exit 134) on ~300 nested parens; 200 evaluated fine (32 MB
  default main stack on the dev machine — check yours with `ulimit -s`).
- Same abort for brackets `[[[…]]]`, braces `{a:{a:…}}`; blocks `{ { … } }`
  survived much deeper (cheaper path, see below).
- jsshell (pinned oracle `JavaScript-C128.14.0`) evaluates 500-deep fine and
  throws catchable `InternalError: too much recursion` (exit 3) from ~1000
  up. Reference shape: accept hundreds, clean error beyond — never abort.
- `JSON.parse` of deep input was already safe (pre-existing
  `serde_json`-level "recursion limit exceeded" error, separate mechanism).

## Root-cause measurements

Per-construct abort thresholds, dev-profile CLI (`target/debug/boa`):

| construct      | 2 MB stack | 8 MB stack | 32 MB stack |
|----------------|------------|------------|-------------|
| parens `(((`   | 10 ✓ / 15 ✗ | 50 ✓ / 100 ✗ | 200 ✓ / 300 ✗ |
| brackets `[[[` | 10 ✓ / 15 ✗ | — | 500 ✗ |
| braces `{a:`   | 10 ✓ / 15 ✗ | — | — |
| blocks `{`     | 50+ ✓ | — | — |

Method: `python3 -c` generates the file, `( ulimit -s <KB>; ./target/debug/boa
file )` pins the main-thread stack. Note the dev machine default is
32 MB (`ulimit -s` → 32768), NOT 8 MB — an early 8 MB-explicit run
contradicted the default-stack runs until this was found.

gdb backtrace at the overflow (test-thread probe, since aborts kill the
runner — probe one depth per process via env var) showed a clean 18-frame
cycle per paren level (binary-op cascade ×7 + conditional + assignment +
expression + cover-primary + primary + member + lhs + update + exponent —
no hidden multiplier, no backtracking; the cover grammar is single-pass).
Per-frame stack deltas (dev): 4–19 KB each, **~150 KB per level total**.

The bloat is NOT the AST values (`size_of::<Expression>()` is 192 B,
`Statement` 464 B, `Token` small). It is unoptimized codegen: large `match`
temporaries with no stack-slot reuse across arms, × 18 frames. Release
frames are far smaller (inlining), so any fixed depth limit fit for
dev/2 MB (~8!) would cripple release (which handles ~450+ today), and any
limit fit for release would still abort dev/2 MB. Hence a fixed counter
was rejected (see below).

## Rejected alternatives

- **Fixed depth counter on `Parser`**: doesn't work structurally (recursion
  flows through `TokenParser::parse` on `&mut Cursor`, never touching
  `Parser`), and no single value is safe on 2 MB dev yet non-crippling on
  8 MB release. Dead on both counts.
- **Boxing one big AST type**: measured — no big type exists (see sizes
  above). Dead.
- **`stacker::grow` (segmented stack)**: unbounded heap on adversarial
  input (1 MB of parens → GBs of segments); still needs a cap, which lands
  back at the fixed-limit problem. Dead.
- **Pratt-parser rewrite / cascade compression**: correct long-term fix
  (frame diet), but project-1-scale surgery. Explicitly deferred; see
  follow-ups.
- **Thread-per-parse with a big stack**: breaks wasm (no threads),
  per-parse spawn overhead, and STILL needs a profile-dependent backstop.
  Dead.
- **Hand-rolled libc stack check**: reimplementing `psm` badly (platform
  `unsafe`, ASAN/Miri/wasm edge cases). Use the real crate instead.

## The fix

Stack-aware guard via `rust-lang/stacker 0.1.25`
(`remaining_stack()`, ~5 ns/check — the OS limit is cached in TLS, so no
per-check libc cost), 256 KB red zone, at the 8 recursion choke points
audited by reading every `TokenParser::parse` for self-recursion:
`PrimaryExpression`, `Statement`, both binding patterns (mutually
recursive), `UnaryExpression`, `AssignmentExpression`,
`ExponentiationExpression`, `MemberExpression` (`new`-callee). Everything
else funnels through these (conditional/yield/spread → assignment;
update/await → unary; call args/elements/substitutions → assignment;
optional chains and statement lists are iterative `loop`s — verified, not
assumed). Tripping returns catchable `SyntaxError: Maximum call stack size
exceeded` (V8's wording).

Structural notes for future editors:

- The guard is a free function (`cursor::check_stack`), not a method:
  call sites hold a live `&Token` borrow from `peek`, so any `&mut cursor`
  call — including a second `peek` — fails borrowck. The free function
  takes only the owned `Position`.
- Error uses `Error::general` (existing catch-all variant); no new error
  variant was needed.
- wasm/Miri (`remaining_stack() == None`) keep historical behavior
  (documented in code): a wasm trap aborts the instance, not the host.
- The parse guard protects all downstream recursion (scope analysis,
  compilation, printing) by construction: the AST can never exceed the
  guard-implied depth, and downstream frames are far smaller than parse
  frames on the same (unwound) stack.
- Effective depth adapts to stack/profile like SpiderMonkey/V8. Only
  clearly-shallow (must parse) and clearly-absurd (must error) depths are
  asserted anywhere; middle depths are environment-dependent by design.

## Test262 margin

Crude whole-corpus scan (strip strings/comments, track `()[]{}` depth):
max nesting is 64 in `test/language/statements/function/S13.2.1_A1_T1.js`
(mixed brackets; blocks are cheap). That suite passes 451/451 even in
dev-mode (stricter than CI's release). Full release suite, CI-exact
invocation: 51,440 pass (+3 vs P0 baseline, all pre-existing P1/P2 fixes),
0 panics/timeouts/crashes, strict `--fail-on=regression` gate exit 0.

## Follow-ups (project 1, not foundations)

- Frame diet (cascade compression / Pratt parsing) to raise accepted depth
  toward jsshell (~500–1000) while keeping the guard as the backstop.
- Threshold tuning if conformance tests ever nest near the trip zone.
- `stacker` version bumps track with the workspace lockfile.
