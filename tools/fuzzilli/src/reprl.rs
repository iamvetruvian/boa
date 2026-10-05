//! The REPRL child wire protocol (mirrors Fuzzilli's `libreprl-posix.c`).
//!
//! Parent spawns us with four fixed fds: 100 (ctrl read), 101 (ctrl write),
//! 102 (script bytes in), 103 (fuzzout: `FUZZILLI_PRINT` output, one
//! `{msg}\n` per call; the parent maps the memfd and takes the length from
//! the shared file offset). Handshake is
//! `HELO` both ways; each iteration reads `exec` + u64 size, evaluates, and
//! writes one i32 status (`(code & 0xff) << 8`, matching the parent's
//! 4-byte status read in `reprl_execute`). The parent also seeks the data
//! fds before each execution, so plain `read`/`write` suffices.
//! stdout/stderr are capture files (block-buffered): flush every iteration.
//!
//! The magic is `exec`, not the `cexe` of older Fuzzilli/docs: verified
//! against `Sources/libreprl/libreprl-posix.c` (`write(ctx->ctrl_out,
//! "exec", 4)`), which is the normative reference for this module.

use std::io::Write as _;

use crate::{cov, eval::Outcome, eval::eval_once};

const CRFD: i32 = 100;
const CWFD: i32 = 101;
const DRFD: i32 = 102;

/// Reads exactly `buf.len()` bytes or returns `None` on EOF/error.
fn read_exact(fd: i32, buf: &mut [u8]) -> Option<()> {
    let mut filled = 0;
    while filled < buf.len() {
        // SAFETY: libc read into a live slice; ret checked before use.
        let ret = unsafe {
            libc::read(
                fd,
                buf.as_mut_ptr().add(filled).cast::<libc::c_void>(),
                buf.len() - filled,
            )
        };
        if ret < 0 {
            // SAFETY: errno read, no invariants.
            if unsafe { *libc::__errno_location() } == libc::EINTR {
                continue;
            }
            return None;
        }
        if ret == 0 {
            return None; // EOF: parent went away.
        }
        // `ret` is positive here (negative returns and zero handled above).
        #[allow(clippy::cast_sign_loss)]
        let advanced = ret as usize;
        filled += advanced;
    }
    Some(())
}

/// Writes the whole buffer or returns `None` on error.
fn write_all(fd: i32, mut buf: &[u8]) -> Option<()> {
    while !buf.is_empty() {
        // SAFETY: libc write over a live slice; ret checked before advancing.
        let ret = unsafe { libc::write(fd, buf.as_ptr().cast::<libc::c_void>(), buf.len()) };
        if ret <= 0 {
            // SAFETY: errno read, no invariants.
            if ret < 0 && unsafe { *libc::__errno_location() } == libc::EINTR {
                continue;
            }
            return None;
        }
        // `ret` is positive here (the non-positive cases return above).
        #[allow(clippy::cast_sign_loss)]
        let advanced = ret as usize;
        buf = &buf[advanced..];
    }
    Some(())
}

/// Runs the REPRL loop until the parent disconnects. Diverges only by exit.
pub(crate) fn run_loop(fuel: usize) -> ! {
    if write_all(CWFD, b"HELO").is_none() {
        std::process::exit(1);
    }
    let mut helo = [0u8; 4];
    if read_exact(CRFD, &mut helo).is_none() || &helo != b"HELO" {
        std::process::exit(1);
    }
    loop {
        let mut magic = [0u8; 4];
        if read_exact(CRFD, &mut magic).is_none() {
            std::process::exit(0); // Parent closed the pipes: clean shutdown.
        }
        if &magic != b"exec" {
            eprintln!("reprl: protocol desync (want exec)");
            std::process::exit(1);
        }
        let mut size_buf = [0u8; 8];
        if read_exact(CRFD, &mut size_buf).is_none() {
            std::process::exit(1);
        }
        // Script sizes are bounded by the 16 MiB data channel; the cast is
        // lossless on the 64-bit-only target this adapter supports.
        #[allow(clippy::cast_possible_truncation)]
        let size = u64::from_le_bytes(size_buf) as usize;
        let mut script = vec![0u8; size];
        if read_exact(DRFD, &mut script).is_none() {
            std::process::exit(1);
        }
        let outcome = eval_once(&String::from_utf8_lossy(&script), fuel);
        // Flush capture files before reporting (block-buffered under REPRL).
        std::io::stdout().flush().ok();
        std::io::stderr().flush().ok();
        // Reset coverage for the next execution (fresh-guards semantics).
        cov::reset_guards();
        let code: u32 = match outcome {
            Outcome::Completed => 0,
            Outcome::Threw(_) => 1,
        };
        if write_all(CWFD, &((code << 8).to_le_bytes())).is_none() {
            std::process::exit(0);
        }
    }
}
