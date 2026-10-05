# Mutation survivors (P7.2)

Every survivor (missed mutant) gets a new killing test or a written
justification reviewed like an ignore entry (plan §7.2). This file is the
machine-checked justification registry: `scripts/run-mutation.sh` fails the
threshold gate unless every `MissedMutant`/`Timeout` record's exact name
appears below as a `JUSTIFIED: <name>` line. `JUSTIFIED` lines are exact
(`grep -xF` semantics against `scenario.Mutant.name`); the prose above each
group states the reason and reviewer.

Timeout triage bar: a `Timeout` counts as killed only with a `JUSTIFIED` line
whose prose names (a) the hanging test, (b) the concurrent fast failures if
any, and (c) a rerun confirming the hang is deterministic, not a flake.
Untriaged timeouts count as unkilled.

## Class R: capacity-hint equivalence (reviewed 2026-10-04, P7.2 pilot)

`reserve`/`reserve_exact` are allocation hints: every push path grows on
demand, so deleting the hint (or over/under-reserving via a flipped
comparison) changes only allocation counts, never observable behavior.
Killing these mutants would require asserting capacities or allocation
counts — brittle white-box assertions rejected as suite damage. The
`checked_add`/`alloc_overflow` paths stay sound under every comparison flip
(`additional < free` implies `len + additional < capacity`: no overflow).
Equivalent mutants; no tests added.

JUSTIFIED: core/string/src/builder.rs:250:9: replace JsStringBuilder<D>::reserve with ()
JUSTIFIED: core/string/src/builder.rs:250:23: replace > with < in JsStringBuilder<D>::reserve
JUSTIFIED: core/string/src/builder.rs:250:23: replace > with == in JsStringBuilder<D>::reserve
JUSTIFIED: core/string/src/builder.rs:274:9: replace JsStringBuilder<D>::reserve_exact with ()
JUSTIFIED: core/string/src/builder.rs:250:23: replace > with >= in JsStringBuilder<D>::reserve
JUSTIFIED: core/string/src/builder.rs:274:23: replace > with == in JsStringBuilder<D>::reserve_exact
JUSTIFIED: core/string/src/builder.rs:274:23: replace > with < in JsStringBuilder<D>::reserve_exact
JUSTIFIED: core/string/src/builder.rs:274:23: replace > with >= in JsStringBuilder<D>::reserve_exact
JUSTIFIED: core/string/src/builder.rs:719:9: replace CommonJsStringBuilder<'seg>::reserve with ()
JUSTIFIED: core/string/src/builder.rs:727:9: replace CommonJsStringBuilder<'seg>::reserve_exact with ()
