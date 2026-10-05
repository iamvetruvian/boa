mod miri {
    use crate::{Trace, Tracer};
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn test_simple_types_trace() {
        let mut tracer = Tracer::new();
        unsafe {
            Instant::now().trace(&mut tracer);
            PathBuf::from(".").trace(&mut tracer);
        }
        assert!(
            tracer.is_empty(),
            "Simple types should not add anything to tracer"
        );
    }

    // MIRI-IGNORE: the `File` fixture needs `File::open`, which Miri
    // rejects as an unsupported operation (isolation aborts the test
    // instead of returning `Err`, so the `if let` below never engages).
    // Still runs in the normal suite.
    #[cfg(not(target_family = "wasm"))]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_file_trace() {
        use std::fs::File;
        if let Ok(file) = File::open("Cargo.toml") {
            let mut tracer = Tracer::new();
            unsafe {
                file.trace(&mut tracer);
            }
            assert!(
                tracer.is_empty(),
                "File handle should not add anything to tracer"
            );
        }
    }
}
