//! Shared harness library for the `boa_fuzz` targets.
//!
//! Splitting generation ([`common`]) and oracle-domain normalization
//! ([`canonicalize`]) into a library keeps the `fuzz_targets/*` binaries thin
//! and — more importantly — makes the harness logic unit-testable with plain
//! `cargo test` (no nightly, no libFuzzer).

pub mod adversarial;
pub mod battery;
pub mod canonicalize;
pub mod common;
pub mod matrix;
pub mod metamorphic;
pub mod mutate;
pub mod semantic;
pub mod universe;
