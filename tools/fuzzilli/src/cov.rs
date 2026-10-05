//! `SanitizerCoverage` edge stub for Fuzzilli (Rust port of its `coverage.c`).
//!
//! Built with `-Cpasses=sancov-module
//! -Cllvm-args=-sanitizer-coverage-trace-pc-guard`, LLVM emits calls to
//! `__sanitizer_cov_trace_pc_guard{,_init}`; this module implements them
//! against Fuzzilli's SHM bitmap (`SHM_ID` env var, else a process-local
//! bitmap so the binary also runs standalone). Unlike upstream `coverage.c`
//! (single module only), ranges accumulate: Rust emits one guard array per
//! codegen unit, so init appends each distinct range and reset numbers
//! guards sequentially across all of them.

use std::sync::{Mutex, OnceLock};

const SHM_SIZE_U32: u32 = 0x20_0000;
const SHM_SIZE: usize = SHM_SIZE_U32 as usize;
const MAX_EDGES: u32 = (SHM_SIZE_U32 - 4) * 8;

/// One module's guard array (start address + guard count; addresses, not
/// pointers, because raw pointers are not `Sync` for statics).
static RANGES: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
/// Mapped bitmap (shared when `SHM_ID` is set, process-local otherwise).
static BITMAP: OnceLock<Bitmap> = OnceLock::new();

struct Bitmap {
    ptr: *mut u8,
    #[allow(dead_code)]
    len: usize,
}

// SAFETY: the bitmap is only touched on the single REPRL thread (trace hits
// run on the evaluating thread; init/reset run between evaluations), exactly
// like upstream coverage.c, whose documented benign race we inherit.
unsafe impl Send for Bitmap {}
unsafe impl Sync for Bitmap {}

/// Maps the SHM bitmap (or a local fallback) exactly once.
fn bitmap() -> &'static Bitmap {
    BITMAP.get_or_init(|| {
        let ptr = if let Ok(key) = std::env::var("SHM_ID") {
            map_shm(&key)
        } else {
            eprintln!("[COV] no shared memory bitmap available, using local");
            // SAFETY: 2MB zeroed box leaked for process lifetime.
            Box::leak(vec![0u8; SHM_SIZE].into_boxed_slice()).as_mut_ptr()
        };
        Bitmap { ptr, len: SHM_SIZE }
    })
}

/// Opens and maps Fuzzilli's SHM bitmap. Aborts loudly on failure (a silent
/// fallback here would run the whole trial coverage-blind).
fn map_shm(key: &str) -> *mut u8 {
    use std::ffi::CString;
    let name = CString::new(key.as_bytes()).expect("SHM_ID must be CString-safe");
    // SAFETY: shm_open/mmap with checked returns; aborts on failure.
    unsafe {
        let fd = libc::shm_open(name.as_ptr(), libc::O_RDWR, 0o600);
        if fd < 0 {
            eprintln!("[COV] shm_open({key}) failed; aborting (no blind runs)");
            std::process::abort();
        }
        let mapping = libc::mmap(
            std::ptr::null_mut(),
            SHM_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        );
        libc::close(fd);
        if mapping == libc::MAP_FAILED {
            eprintln!("[COV] shm mmap failed; aborting (no blind runs)");
            std::process::abort();
        }
        mapping.cast::<u8>()
    }
}

/// Writes the total edge count header (u32 at bitmap offset 0).
fn set_num_edges(total: u32) {
    // SAFETY: bitmap is SHM_SIZE bytes; offset 0 always mapped.
    unsafe {
        bitmap().ptr.cast::<u32>().write_unaligned(total);
    }
}

/// Public reset for the REPRL loop (fresh guards each execution).
pub(crate) fn reset_guards() {
    __sanitizer_cov_reset_edgeguards();
}

/// Assigns sequential guard indices across all registered ranges.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn __sanitizer_cov_reset_edgeguards() {
    let ranges = RANGES.lock().expect("cov ranges lock");
    let mut next: u32 = 0;
    for &(start_addr, len) in ranges.iter() {
        // SAFETY: addresses came from LLVM's own init calls for live guard
        // arrays; single-threaded between evaluations.
        let start = start_addr as *mut u32;
        for i in 0..len {
            if next >= MAX_EDGES {
                break;
            }
            next += 1;
            unsafe {
                start.add(i).write(next);
            }
        }
    }
    set_num_edges(next);
}

/// Registers one module's guard array (called once per codegen unit).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn __sanitizer_cov_trace_pc_guard_init(mut start: *mut u32, stop: *mut u32) {
    // SAFETY: LLVM passes valid range pointers; all derefs null/size-checked.
    unsafe {
        if start == stop || start.is_null() {
            return;
        }
        if start.read() != 0 {
            return; // Already initialized (matches upstream).
        }
        // Force bitmap mapping now so startup prints land before any exec.
        let _ = bitmap();
        let len = usize::try_from(stop.offset_from(start)).expect("guard range end precedes start");
        let mut ranges = RANGES.lock().expect("cov ranges lock");
        if ranges.iter().any(|&(s, _)| s == start as usize) {
            return; // Duplicate init for a known range.
        }
        ranges.push((start as usize, len));
        // Number the new range immediately so early edges resolve; the
        // per-exec reset renumbers everything anyway.
        let mut next: u32 = ranges
            .iter()
            .map(|&(_, l)| u32::try_from(l).unwrap_or(u32::MAX))
            .sum::<u32>()
            .saturating_sub(u32::try_from(len).unwrap_or(u32::MAX));
        while start != stop && next < MAX_EDGES {
            next += 1;
            start.write(next);
            start = start.add(1);
        }
        set_num_edges(next);
        eprintln!("[COV] registered guard range (total edges now {next})");
    }
}

/// Records one edge hit (called on every instrumented edge, first hit only).
#[unsafe(no_mangle)]
pub(crate) extern "C" fn __sanitizer_cov_trace_pc_guard(guard: *mut u32) {
    // SAFETY: LLVM passes a live guard pointer; the bitmap is mapped.
    // Inherits upstream's benign same-edge race (first edge ignored).
    unsafe {
        if guard.is_null() {
            return;
        }
        let index = guard.read();
        if index == 0 || index >= MAX_EDGES {
            return;
        }
        let byte = bitmap().ptr.add(4 + (index / 8) as usize);
        byte.write(byte.read() | (1 << (index % 8)));
        guard.write(0);
    }
}
