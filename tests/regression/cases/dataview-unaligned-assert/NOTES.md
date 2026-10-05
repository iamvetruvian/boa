# dataview-unaligned-assert

Batched atomic copies assumed Phase 1 always reaches `BATCH_SIZE`
alignment; small copies (`chunks == 0`, e.g. one `DataView.getUint8` on a
shared buffer) tripped an over-strict `debug_assert!` and aborted dev
builds even though the copy itself was correct. Fixed by early-returning
the sub-batch copy (a6101fe1).

Admitted via the P4 crash-gate merge-back demo: reintroduced through
`tests/fuzz/drill/crash-patches/dataview-unaligned-assert.break.patch`,
rediscovered by the `cli-exec` fleet binary with signature
`array_buffer/utils.rs:654`, minimized (`tmin-ok`), then carried here
(seed + rule + ratchet) as the end-to-end pipeline exhibit.
