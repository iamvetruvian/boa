# P5.4 opcode/operand coverage matrix

Measures which VM opcodes the fuzzer-visible universe executes, with what
operand shapes, and ratchets the result so coverage never silently regresses.

## Files

- `opcodes.txt`: the 195 real opcode names (61 `Reserved*` excluded),
  generated from the `generate_opcodes!` macro. Regen when opcodes change
  (see below); the `--check` new-cell rule catches drift between regens.
- `ratchet.json`: committed floors from the deterministic feeds (10k
  battery programs + seeds + probes, plain and optimized, plus Test262
  static). Every row is a minimum: a fresh run below any floor fails.
- `justifications.toml`: `[zero]` reasons for zero-floor ratchet cells
  (empty today: every closed-universe cell has deterministic evidence
  except the deep-only ones) and `[deep]` presence pins for cells with
  evidence only in `test262-dynamic.json`.
- `test262-dynamic.json`: the full Test262 dynamic merge (339 rows).
  Periodic regen, never gated (the campaign needs timeouts: pathological
  regex/temporal tests spin past any in-process budget).
- `regen.py`: regenerates `ratchet.json` + `justifications.toml` from the
  deterministic merge + the deep merge, then self-verifies with `--check`.

## Cell vocabulary

Base opcode cells plus refinements where operands genuinely vary:
`|taken`/`|nottaken` (9 conditional jumps), `|argv-0..7`/`|argv-many`
(4 fixed-arity calls), `|k-str`/`|k-bigint` (`StoreLiteral`), `|err-T`
(surfacing error kind; open family). `|k-mistype`/`|k-oob` are corruption
alarms: observed anywhere, the gate fails unjustifiably. See
`core/engine/src/vm/coverage.rs` for the single grammar both the dynamic
hooks and the static scan share.

## Canonical commands (from `tests/fuzz`)

Deterministic feeds (~2 min, CI-gateable):

```sh
cargo run -q --release --example matrix -- det-battery.json \
  --programs 10000 --seeds --probes
cargo run -q --release --example matrix -- det-battery-opt.json \
  --programs 10000 --seeds --probes --optimize
find ../../test262/test -name '*.js' | sort > t262-files.txt
cargo run -q --release --example matrix -- det-static.json \
  --programs 0 --files-from t262-files.txt --static-only
cargo run -q --release --example matrix -- --check \
  --ratchet matrix/ratchet.json --justifications matrix/justifications.toml \
  --deep matrix/test262-dynamic.json \
  det-battery.json det-battery-opt.json det-static.json
```

Full Test262 dynamic (deep evidence; ~30 min at 8-way, chunked because
`built-ins/RegExp`, `built-ins/decodeURI*`, and `staging/sm/*` spin past
a 300 s in-process budget — pre-existing engine performance pathologies,
not hook overhead, which measures ~zero):

```sh
# 148 disjoint depth-3 chunks (+26 orphan files), 300 s each, 8-way;
# timed-out chunks subdivide to depth-4, then to files at 60 s.
boa_tester run -s test/<chunk> --coverage-out chunks/<name>.json
cargo run -q --release --example matrix -- --merge deep.json chunks/*.json
```

Regen (rare: opcode/battery changes, deliberate floor bumps):

```sh
python3 matrix/regen.py deterministic.json test262-dynamic.json
```

`regen.py` warns on template `[zero]` reasons: each must be replaced with
a named probe (preferred — see `../probes/`) or a written reachability
reason before commit. `opcodes.txt` regen:

```sh
python3 -c "
import re
src = open('../../core/engine/src/vm/opcode/mod.rs').read()
body = src.split('generate_opcodes! {', 1)[1]
body = body[:body.index(chr(10) + '}' + chr(10))]
names = [n for n in re.findall(r'^    ([A-Z][A-Za-z0-9]*)', body, re.M)
         if not n.startswith('Reserved')]
open('matrix/opcodes.txt', 'w').write(chr(10).join(names) + chr(10))"
```

## Gate rules (`--check`)

Battery version match; no defensive cells; no new cells (add with floors);
no stale cells (regen); every floor met; every zero cell justified (exact
entry, or base entry for refinements of a zero base); no stale
justifications; every `[deep]` cell present in the deep file and absent
from fresh (present means promote to a ratchet row). The deep file's
`battery_version` is 0 by construction (no battery content; presence-only).
