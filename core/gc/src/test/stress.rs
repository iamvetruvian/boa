mod miri {
    use std::num::NonZeroU64;

    use super::super::{Harness, run_test};
    use crate::{
        BOA_GC, Gc, StressGuard, force_collect, gc_collections, gc_set_stress, gc_stress_every,
        gc_total_allocs,
    };

    fn every(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).expect("test cadence is nonzero")
    }

    /// Allocates `count` garbage `u8` nodes, dropping each handle immediately.
    fn alloc_garbage(count: usize) {
        for _ in 0..count {
            drop(Gc::new(0_u8));
        }
    }

    #[test]
    fn stress_collects_every_n_allocations() {
        run_test(|| {
            gc_set_stress(Some(every(16)));
            // Warmup absorbs any lazy one-time allocations; the measured
            // batch then proves the exact cadence.
            alloc_garbage(100);
            let before = gc_collections();
            alloc_garbage(160);
            assert_eq!(gc_collections(), before + 10);

            // Live roots survive stress collections.
            let live = Gc::new(42_u8);
            alloc_garbage(160);
            assert_eq!(*live, 42);
            assert_eq!(gc_collections(), before + 20);
        });
    }

    #[test]
    fn stress_bypasses_threshold_and_adaptive_growth() {
        run_test(|| {
            // A cadence far above this test's allocation count: no stress
            // collection can fire, so any collection would have to come from
            // the byte threshold — which must stay bypassed.
            gc_set_stress(Some(every(1_000_000)));
            let mut roots = Vec::with_capacity(600);
            for _ in 0..600 {
                roots.push(Gc::new([0_u8; 4096]));
            }
            Harness::assert_collections(0);
            let threshold = BOA_GC.with(|current| current.borrow().config.threshold);
            assert_eq!(threshold, 1_048_576);
            drop(roots);
            force_collect();
        });
    }

    #[test]
    fn clearing_stress_restores_threshold_mode() {
        run_test(|| {
            gc_set_stress(Some(every(16)));
            gc_set_stress(None);
            assert_eq!(gc_stress_every(), None);
            let mut roots = Vec::with_capacity(600);
            for _ in 0..600 {
                roots.push(Gc::new([0_u8; 4096]));
            }
            // 600 × ~4 KiB crosses the 1 MiB threshold: normal mode collects.
            assert!(gc_collections() >= 1);
            Harness::assert_bytes_allocated();
            drop(roots);
        });
    }

    #[test]
    fn total_allocs_counts_every_allocation() {
        run_test(|| {
            alloc_garbage(10);
            let before = gc_total_allocs();
            alloc_garbage(50);
            assert_eq!(gc_total_allocs(), before + 50);
        });
    }

    #[test]
    fn stress_guard_restores_previous_setting() {
        run_test(|| {
            assert_eq!(gc_stress_every(), None);
            let outer = StressGuard::stress(every(7));
            assert_eq!(gc_stress_every(), Some(every(7)));
            let inner = StressGuard::stress(every(3));
            assert_eq!(gc_stress_every(), Some(every(3)));
            drop(inner);
            assert_eq!(gc_stress_every(), Some(every(7)));
            drop(outer);
            assert_eq!(gc_stress_every(), None);
        });
    }

    #[test]
    fn stress_guard_restores_on_panic() {
        run_test(|| {
            gc_set_stress(Some(every(11)));
            let caught = std::panic::catch_unwind(|| {
                let _guard = StressGuard::stress(every(5));
                assert_eq!(gc_stress_every(), Some(every(5)));
                panic!("intentional unwind");
            });
            assert!(caught.is_err());
            assert_eq!(gc_stress_every(), Some(every(11)));
        });
    }
}
