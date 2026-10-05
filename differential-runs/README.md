# P3.5 full-corpus differential run — durable snapshot

Snapshot taken 2026-10-03 (run completed 2026-10-03 ~09:18 +0530).
Copied from `target/diff-full/` (gitignored build output) so no re-run is
ever needed: this directory plus the tarball below are the complete record.

## Provenance

- Tool: `boa_differential` 0.22.0, schema 1, per-case `timeout_secs: 30`.
- Oracle: jsshell `JavaScript-C128.14.0` (`target/oracle/js`; re-fetchable
  via `scripts/fetch-oracle.sh`, not archived here).
- Corpora: `full-builtins`, `full-language`, `full-intl402`,
  `full-staging`, `full-annexB` (names recorded per file in
  `diff-results.json` under `corpus`; case ids + exclusions are inside
  each `diff-results.json`, so the corpus is recoverable from these files
  alone).

## Counts

| shard    | total | agree | mismatch | one-sided | excluded |
|----------|-------|-------|----------|-----------|----------|
| builtins | 43159 | 18664 | 24419    | 76        | 2148     |
| language | 34099 | 21545 | 12463    | 91        | 6053     |
| intl402  | 5232  | 956   | 4276     | 0         | 741      |
| staging  | 2220  | 1295  | 920      | 5         | 316      |
| annexB   | 1194  | 715   | 479      | 0         | 106      |
| total    | 85904 | 43175 | 42557    | 172       | 9364     |

## Triage state at snapshot time

| queue    | entries | untriaged |
|----------|---------|-----------|
| builtins | 24495   | 85 (incl. 18 canblock runner-artifacts, no verdict by design) |
| language | 12554   | 12190 (364 pre-triaged by skip-minimization) |
| intl402  | 4276    | 4276 |
| staging  | 925     | 925 |
| annexB   | 479     | 479 |

Builtins verdicts applied through bulk rule 27 (see session notes).
Re-snapshot this directory when triage completes.

## Layout

- `<shard>/diff-results.json` — immutable run record (cases, counts,
  exclusions, corpus, oracle).
- `<shard>/triage-queue.json` — triage working state (verdicts marked here).
- `<shard>/triage-queue.json.bak` — pre-last-bulk-run backup (builtins only).
- `logs/` — per-shard stdout logs.
- `workflow-snapshot/` — triage workflow scripts + adjudication sets as of
  snapshot time (bulk rules 1–27, censuses, node/V8 adjudicators). Scratch
  copies live in `/tmp` (lost on reboot — this is the backup).
- `full-corpus-2026-10-03.tar.zst` — compressed archive of all of the
  above (verify with `zstd -t`, list with `tar --list -I zstd -f ...`).
- `SHA256SUMS` — checksums of the raw files.

## Restore

```sh
# Work directly on these files, or copy back:
cp -a differential-runs/builtins differential-runs/language \
  differential-runs/intl402 differential-runs/staging differential-runs/annexB \
  target/diff-full/
```

This directory is gitignored (2.2 GB raw): it survives `cargo clean`
but NOT `git clean -fdx`. The tarball is the compact second copy.
