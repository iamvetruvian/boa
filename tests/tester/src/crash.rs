//! Crash and abnormal-outcome accounting for the test runner.
//!
//! Every abnormal test outcome (panic, timeout, crash, harness error) produces
//! a [`CrashRecord`] carrying a dedupe signature, the failure message, the
//! attributed crate, and the captured backtrace. Records are persisted beside
//! `latest.json` as `crashes.json` (see [`write_crashes`]), so crash trends
//! and new-signature gates can be computed by `compare`.

use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::TestOutcomeResult;

/// Maximum bytes kept from a panic message or backtrace frame line.
const MAX_MESSAGE_LEN: usize = 2000;

/// Maximum backtrace frames stored per record.
const MAX_FRAMES: usize = 40;

/// A single abnormal test outcome with diagnosis payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CrashRecord {
    /// Stable test id: test262-relative path without extension + test name.
    pub(crate) test: String,
    /// The abnormal outcome (`Panic`, `Timeout`, `Crash`, or `HarnessError`).
    pub(crate) outcome: TestOutcomeResult,
    /// One-line dedupe signature, e.g. `panic@boa_engine: message…`.
    pub(crate) signature: String,
    /// Full(ish) failure message or panic payload.
    pub(crate) message: String,
    /// Crate attributed from backtrace frames, or `"unknown"`.
    pub(crate) attributed_crate: String,
    /// Top meaningful backtrace frame, or `""`.
    pub(crate) location: String,
    /// Captured backtrace frames, newest first, capped at [`MAX_FRAMES`].
    pub(crate) backtrace: Vec<String>,
}

impl CrashRecord {
    /// Builds a record from a caught panic payload.
    pub(crate) fn from_panic(test_id: &str, payload: &dyn Any) -> Self {
        let message = panic_message(payload);
        let backtrace = capture_frames();
        let (attributed_crate, location) = attribute_frames(&backtrace);
        let signature = format!("panic@{attributed_crate}: {}", first_line(&message, 160));
        Self {
            test: test_id.into(),
            outcome: TestOutcomeResult::Panic,
            signature,
            message: truncate(message, MAX_MESSAGE_LEN),
            attributed_crate,
            location,
            backtrace,
        }
    }

    /// Builds a record for a non-panic abnormal outcome.
    pub(crate) fn new(
        test_id: &str,
        outcome: TestOutcomeResult,
        message: String,
        backtrace: Vec<String>,
    ) -> Self {
        debug_assert!(outcome.is_abnormal());
        let (attributed_crate, location) = attribute_frames(&backtrace);
        let kind = match outcome {
            TestOutcomeResult::Timeout => "timeout",
            TestOutcomeResult::Crash => "crash",
            TestOutcomeResult::HarnessError => "harness",
            _ => "panic",
        };
        let signature = format!("{kind}@{attributed_crate}: {}", first_line(&message, 160));
        Self {
            test: test_id.into(),
            outcome,
            signature,
            message: truncate(message, MAX_MESSAGE_LEN),
            attributed_crate,
            location,
            backtrace: backtrace.into_iter().take(MAX_FRAMES).collect(),
        }
    }
}

/// Extracts a message from a panic payload.
fn panic_message(payload: &dyn Any) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        String::from("<non-string panic payload>")
    }
}

/// Captures the current backtrace as cleaned frame lines.
fn capture_frames() -> Vec<String> {
    let capture = std::backtrace::Backtrace::force_capture().to_string();
    capture
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .take(MAX_FRAMES)
        .collect()
}

/// Attributes a backtrace to the first `boa_*` crate frame that is not the
/// test harness itself, returning `(crate, frame)`.
fn attribute_frames(frames: &[String]) -> (String, String) {
    for frame in frames {
        if let Some(idx) = frame.find("boa_") {
            let after = &frame[idx + "boa_".len()..];
            let end = after
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(after.len());
            let name = format!("boa_{}", &after[..end]);
            if name != "boa_" && name != "boa_tester" {
                return (name, truncate(frame.clone(), 300));
            }
        }
    }
    // Fall back to a harness frame if nothing else matched.
    for frame in frames {
        if frame.contains("boa_tester") {
            return (String::from("boa_tester"), truncate(frame.clone(), 300));
        }
    }
    (String::from("unknown"), String::new())
}

/// First line of `text`, truncated to `max` chars.
fn first_line(text: &str, max: usize) -> String {
    truncate(text.lines().next().unwrap_or("").trim().to_string(), max)
}

/// Truncates `text` to at most `max` chars (ASCII-safe: truncates on bytes
/// only at char boundaries).
pub(crate) fn truncate(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{attribute_frames, first_line, panic_message, truncate};

    #[test]
    fn extracts_str_and_string_payloads() {
        let s: &dyn std::any::Any = &"boom";
        assert_eq!(panic_message(s), "boom");
        let owned: &dyn std::any::Any = &String::from("bang");
        assert_eq!(panic_message(owned), "bang");
        let other: &dyn std::any::Any = &42_u32;
        assert_eq!(panic_message(other), "<non-string panic payload>");
    }

    #[test]
    fn attributes_first_engine_frame() {
        let frames = [
            "0: std::panicking::begin_panic_handler".to_string(),
            "1: boa_tester::exec::run_once".to_string(),
            "2: boa_engine::vm::execute_one".to_string(),
            "3: boa_gc::lib::collect".to_string(),
        ];
        let (krate, location) = attribute_frames(&frames);
        assert_eq!(krate, "boa_engine");
        assert!(location.contains("execute_one"));
    }

    #[test]
    fn attributes_unknown_without_boa_frames() {
        let frames = ["0: std::panicking::begin_panic_handler".to_string()];
        let (krate, location) = attribute_frames(&frames);
        assert_eq!(krate, "unknown");
        assert!(location.is_empty());
    }

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate(String::from("abcdef"), 4), "abcd");
        assert_eq!(truncate(String::from("a⚠b"), 2), "a");
        assert_eq!(first_line("one\ntwo", 10), "one");
    }
}
