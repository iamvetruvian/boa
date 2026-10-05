# Bug #13 — Prototype-cycle walk abort/hang: discovery notes

Filing: `cyclic-proto-walk-stack-overflow` in `regressions.toml` (landed).
Repro: `repro.js` (proxy-mediated cycle + `leaf.toString` → catchable
`RangeError`). Fuzz seed: `tests/fuzz/seeds/cyclic-proto-walk-stack-overflow.js`.
Unit test: `core/engine/src/object/tests.rs::cyclic_prototype_walk_throws`
(7 probes: get/set/has/instanceof/isPrototypeOf/lookupGetter/lookupSetter).

## Observed behavior (pre-fix)

- The seed aborted the process: `cli-exec` died with SIGSEGV (exit 139) on
  its first fuzz run from this seeded input — no sanitizer artifact (see
  `drill/README.md`, "Triaging a crash without sanitizer artifacts").
- `ordinary_get` recursed natively once per prototype link
  (`object/internal_methods/mod.rs`, `parent.__get__(…)`); on the cycle
  `root → leaf → proxy → target → root` this never terminates.
- The iterative walks (`instanceof`, `isPrototypeOf`, legacy
  `__lookupGetter__`/`__lookupSetter__`) did not crash — they HUNG (stack-
  flat `loop`, infinite on a cycle). Found during the fix, not before it.

## Why the filed fix model was wrong

The filing suggested "consulting the VM recursion limit". That limit
(`Context::check_runtime_limits`, `vm/mod.rs`) counts **VM frames**
(`frames.len() + host_call_depth ≤ 512`) — and the property walk pushes
no frames (the proxy in the cycle has no traps, so no JS ever runs). The
check is blind here; wiring it in would have changed nothing. Verified by
reading both sides, not by guessing.

## Reference-engine behavior (verified, not assumed)

- jsshell (pinned oracle): `InternalError: too much recursion` (exit 3).
- node v26 (V8): catchable `RangeError: Maximum call stack size exceeded`
  for property get AND `instanceof` (via `[Symbol.hasInstance]`) AND
  non-member `isPrototypeOf` AND `__lookupGetter__`. (Careful when
  re-probing `isPrototypeOf`: `isPrototypeOf.call(root, leaf)` FINDS root
  in 3 steps and returns — you must query a non-member to traverse the
  cycle.)
- The fix matches V8 exactly: native catchable `RangeError` with V8's
  exact message, on all 7 walk shapes.

## The fix (two mechanisms, not one)

Recursive walks (`ordinary_get`, `ordinary_set`, `ordinary_has_property`):
stack-aware guard via `rust-lang/stacker` (already in the workspace for
bug #22), 256 KB red zone, throwing the V8-identical `RangeError`.

Iterative walks (`ordinary_has_instance`, `is_prototype_of`,
`legacy_lookup_getter`, `legacy_lookup_setter`): a **100k-link iteration
budget** throwing the same error. A stack check provably cannot work in a
stack-flat loop — the first version of this fix hung the unit test at 99%
CPU for 25 minutes before that was understood. The budget also covers a
hostile `getPrototypeOf` trap returning fresh objects forever (infinite
novel chain, no cycle, no stack growth — untrippable by any stack check).

Completeness argument (audited, in the code docs): proxy targets are
immutable, so proxy forwarding always terminates at a non-proxy object —
only `__proto__`-mutable links can cycle, and every such walk passes
through one of the six guarded sites. `ordinary_set_prototype_of`'s own
cycle-check loop needs no guard (it only follows ordinary links, which
form a DAG by construction, and breaks at the first non-ordinary proto).
Accessor calls, traps, and `super` go through frame-counted call
machinery (existing limits apply).

Deliberate divergences, documented:

- Plain JS recursion still throws Boa's uncatchable engine
  `RuntimeLimitError` (verified: try/catch cannot catch it); the walks
  throw catchable `RangeError`. Unifying both to V8's catchable
  `RangeError` is project-1 follow-up.
- The 100k budget errs toward accepting deep-but-finite chains (V8's
  stack-implied trip zone starts lower); succeeding where V8 errors is
  benign (bounded ~0.5 s worst case), crashing/hanging is not.

## Verification performed

- All 7 CLI probes → catchable `RangeError`, exit 0 under try/catch; seed
  alone → clean uncaught `RangeError`, exit 1; normal lookups byte-
  identical (`1 false true true` sanity).
- Engine suite 1,128/1,128; Test262 `built-ins/{Object,Proxy,Reflect}` at
  100%; full release suite CI-exact with strict `--fail-on=regression`
  gate exit 0 (see bug #22's notes for the numbers).
- Regression entry landed with `throw RangeError` and mutation-verified
  to execute (wronging the expectation fails naming the entry).

## Follow-ups (project 1, not foundations)

- Unify JS-recursion `RuntimeLimitError` to catchable `RangeError` (V8
  parity for `function f(){f()}f()`).
- `structuredClone`/own-graph deep-nesting recursion is a different walk
  family with existing handling — untouched; the fuzzer owns it.
- OOM-guard policy (`huge-sparse-join-abort`, still live drill bait) is
  engine design work, separate from this fix.
