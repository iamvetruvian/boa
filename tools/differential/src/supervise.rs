//! Subprocess supervisor: identical timeouts and resource caps for both sides.
//!
//! This is the P3 instantiation of the P1.2 per-case isolation mechanism
//! (owned by the tester: concurrent pipe drains, poll loop with wall-clock
//! kill, signal-death vs nonzero-exit classification, bounded captures).
//! It lives here rather than behind a tester dependency because the tester
//! is a binary crate with `pub(crate)` internals, and its supervisor is
//! Test262-specific (spawns `run-single`, parses `SingleOutcome`); the
//! mechanism — bounds, kill discipline, classification — is identical.
//!
//! Both sides of every case go through [`supervise`]: the Boa side as a
//! `run-case` child of this binary, the oracle side as the pinned shell.
//! Timeout budgets are therefore wall-clock-identical by construction.

use boa_outcomes::TestOutcomeResult;
use std::{
    ffi::OsStr,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Maximum child output bytes captured per stream.
///
/// Deliberately larger than the tester's `64 KiB` bound: differential state
/// markers for legitimate tests (e.g. the 4300-variable Unicode identifier
/// lists, `~200 KiB` of marker) exceed it, while `1 MiB` still bounds
/// pathological output.
const MAX_CHILD_OUTPUT: u64 = 1_048_576;

/// Poll interval while waiting on a supervised child.
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// How a supervised child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildEnd {
    /// Exited naturally with a status code.
    Exited(i32),
    /// Died on a signal (Unix; the tester precedent classifies Windows
    /// abnormal deaths without signal info at the caller).
    Signaled(i32),
    /// Killed after exceeding the wall-clock budget.
    TimedOut,
    /// The child could not be spawned.
    SpawnFailed(String),
}

/// A finished supervised child: its end state plus bounded captures.
#[derive(Debug, Clone)]
pub struct SupervisedChild {
    /// How the child ended.
    pub end: ChildEnd,
    /// Bounded stdout capture (lossy UTF-8).
    pub stdout: String,
    /// Bounded stderr capture (lossy UTF-8).
    pub stderr: String,
}

/// Runs `program args...` with a wall-clock budget, draining both pipes.
///
/// Never blocks past `timeout_secs` (+ poll granularity): the child is
/// killed on expiry. Both streams are drained on dedicated threads while
/// waiting, so a chatty child can never deadlock the supervisor against a
/// full OS pipe buffer (the P1 `regress-567152` lesson).
pub fn supervise(program: &Path, args: &[impl AsRef<OsStr>], timeout_secs: u64) -> SupervisedChild {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return SupervisedChild {
                end: ChildEnd::SpawnFailed(err.to_string()),
                stdout: String::new(),
                stderr: String::new(),
            };
        }
    };

    let stdout_drain = spawn_drain(child.stdout.take());
    let stderr_drain = spawn_drain(child.stderr.take());

    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return SupervisedChild {
                    end: child_end(status),
                    stdout: join_drain(stdout_drain),
                    stderr: join_drain(stderr_drain),
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                drop(child.kill());
                drop(child.wait());
                drop(join_drain(stdout_drain));
                drop(join_drain(stderr_drain));
                return SupervisedChild {
                    end: ChildEnd::TimedOut,
                    stdout: String::new(),
                    stderr: String::new(),
                };
            }
            Ok(None) => std::thread::sleep(CHILD_POLL_INTERVAL),
            Err(err) => {
                return SupervisedChild {
                    end: ChildEnd::SpawnFailed(format!("could not wait on child: {err}")),
                    stdout: join_drain(stdout_drain),
                    stderr: join_drain(stderr_drain),
                };
            }
        }
    }
}

/// Maps a natural exit to [`ChildEnd`], splitting signals from codes.
fn child_end(status: std::process::ExitStatus) -> ChildEnd {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return ChildEnd::Signaled(signal);
    }
    ChildEnd::Exited(status.code().unwrap_or(-1))
}

/// Classifies a finished Boa `run-case` child.
///
/// Our own binary must always exit 0 with an outcome line: any other end
/// is a harness failure, except kills (timeout) and signal deaths (crash).
pub fn classify_boa_child(child: &SupervisedChild) -> (TestOutcomeResult, String) {
    match &child.end {
        ChildEnd::Exited(0) => (TestOutcomeResult::Passed, String::new()),
        ChildEnd::Exited(code) => (
            TestOutcomeResult::HarnessError,
            format!("boa child exited with code {code}: {}", stderr_head(child)),
        ),
        ChildEnd::Signaled(signal) => (
            TestOutcomeResult::Crash,
            format!("boa child died on signal {signal}: {}", stderr_head(child)),
        ),
        ChildEnd::TimedOut => (
            TestOutcomeResult::Timeout,
            "boa child exceeded the wall-clock budget".to_owned(),
        ),
        ChildEnd::SpawnFailed(err) => (
            TestOutcomeResult::HarnessError,
            format!("could not spawn boa child: {err}"),
        ),
    }
}

/// Classifies a finished oracle child.
///
/// Unlike our own binary, the oracle may exit nonzero for engine-level
/// reasons outside the driver (out of memory, startup failure, a throw
/// above the driver) — that is a case-level oracle failure, not a harness
/// failure. Signal deaths are crashes; kills are timeouts.
pub fn classify_oracle_child(child: &SupervisedChild) -> (TestOutcomeResult, String) {
    match &child.end {
        ChildEnd::Exited(0) => (TestOutcomeResult::Passed, String::new()),
        ChildEnd::Exited(code) => (
            TestOutcomeResult::Failed,
            format!("oracle exited with code {code}: {}", stderr_head(child)),
        ),
        ChildEnd::Signaled(signal) => (
            TestOutcomeResult::Crash,
            format!("oracle died on signal {signal}: {}", stderr_head(child)),
        ),
        ChildEnd::TimedOut => (
            TestOutcomeResult::Timeout,
            "oracle exceeded the wall-clock budget".to_owned(),
        ),
        ChildEnd::SpawnFailed(err) => (
            TestOutcomeResult::HarnessError,
            format!("could not spawn oracle: {err}"),
        ),
    }
}

/// First 300 chars of stderr for classification messages.
fn stderr_head(child: &SupervisedChild) -> String {
    child.stderr.trim().chars().take(300).collect()
}

/// Spawns a thread draining one child pipe into a bounded buffer.
///
/// The capture keeps only the first [`MAX_CHILD_OUTPUT`] bytes, but the
/// thread keeps draining to EOF regardless: stopping at the cap drops the
/// read end and SIGPIPEs a child that keeps writing (observed on both
/// sides as broken-pipe deaths). EOF arrives when the child exits; a
/// hung child is killed by the poll loop, which closes the pipes.
fn spawn_drain(
    pipe: Option<impl Read + Send + 'static>,
) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    pipe.map(|pipe| {
        std::thread::spawn(move || {
            let mut pipe = pipe;
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let remaining = MAX_CHILD_OUTPUT.saturating_sub(buf.len() as u64);
                        let room = usize::try_from(remaining).unwrap_or(usize::MAX);
                        buf.extend_from_slice(&chunk[..n.min(room)]);
                    }
                    Err(err) => {
                        if err.kind() != std::io::ErrorKind::Interrupted {
                            break;
                        }
                    }
                }
            }
            buf
        })
    })
}

/// Joins a drain thread into a lossy string, tolerating any failure.
fn join_drain(handle: Option<std::thread::JoinHandle<Vec<u8>>>) -> String {
    let buf = handle.map_or_else(Vec::new, |handle| handle.join().unwrap_or_default());
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{ChildEnd, SupervisedChild, classify_boa_child, classify_oracle_child, supervise};
    use boa_outcomes::TestOutcomeResult;

    fn child(end: ChildEnd, stderr: &str) -> SupervisedChild {
        SupervisedChild {
            end,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn boa_nonzero_exit_is_harness_error() {
        let (outcome, message) = classify_boa_child(&child(ChildEnd::Exited(1), "boom"));
        assert_eq!(outcome, TestOutcomeResult::HarnessError);
        assert!(message.contains("code 1"), "{message}");
    }

    #[test]
    fn oracle_nonzero_exit_is_case_failed() {
        // Exit 3 with a TypeError on stderr is the oracle's normal "the
        // driver itself failed" shape — engine-level, never harness-level.
        let (outcome, message) =
            classify_oracle_child(&child(ChildEnd::Exited(3), "case.js:1:1 TypeError: x\n"));
        assert_eq!(outcome, TestOutcomeResult::Failed);
        assert!(message.contains("code 3"), "{message}");
    }

    #[test]
    fn signals_timeouts_and_spawn_failures_classify() {
        assert_eq!(
            classify_boa_child(&child(ChildEnd::Signaled(11), "")).0,
            TestOutcomeResult::Crash
        );
        assert_eq!(
            classify_oracle_child(&child(ChildEnd::TimedOut, "")).0,
            TestOutcomeResult::Timeout
        );
        assert_eq!(
            classify_boa_child(&child(ChildEnd::SpawnFailed("no".to_owned()), "")).0,
            TestOutcomeResult::HarnessError
        );
    }

    #[test]
    #[cfg(unix)]
    fn supervises_exit_code_and_captures_streams() {
        let child = supervise(
            std::path::Path::new("sh"),
            &["-c", "echo out; echo err >&2; exit 3"],
            10,
        );
        assert_eq!(child.end, ChildEnd::Exited(3));
        assert_eq!(child.stdout, "out\n");
        assert_eq!(child.stderr, "err\n");
    }

    #[test]
    #[cfg(unix)]
    fn kills_on_timeout() {
        let child = supervise(std::path::Path::new("sh"), &["-c", "sleep 30"], 1);
        assert_eq!(child.end, ChildEnd::TimedOut);
    }

    #[test]
    #[cfg(unix)]
    fn over_cap_output_drains_without_sigpipe() {
        // 2000000 bytes through a 1048576-byte capture: the child must exit
        // normally (pre-fix the drain stopped at the cap and SIGPIPEd it).
        let child = supervise(
            std::path::Path::new("sh"),
            &["-c", "yes | head -c 2000000"],
            30,
        );
        assert_eq!(child.end, ChildEnd::Exited(0));
        assert_eq!(child.stdout.len() as u64, super::MAX_CHILD_OUTPUT);
    }

    #[test]
    fn spawn_failure_reports() {
        let args = ["x"];
        let child = supervise(
            std::path::Path::new("/definitely/not/a/binary-boa-diff"),
            &args,
            5,
        );
        assert!(
            matches!(child.end, ChildEnd::SpawnFailed(_)),
            "{:?}",
            child.end
        );
    }
}
