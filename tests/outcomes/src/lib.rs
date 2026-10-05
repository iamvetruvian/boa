//! Shared conformance outcome taxonomy for Boa's test harnesses.
//!
//! Every harness (Test262 `boa_tester`, WPT `boa_wpt`, and the P3
//! differential pipeline) counts the same closed outcome set and stores the
//! same single-letter codes (`O/I/F/P/T/C/H`, per the P1.1 shared
//! JSON-store conventions). Depending on this crate — instead of
//! duplicating the taxonomy — keeps all conformance stores mutually
//! queryable.

use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};

/// Outcome of a single conformance test.
///
/// The `serde` representation is the stable single-letter code shared by
/// every store; never renumber or repurpose a letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestOutcomeResult {
    /// The test passed.
    #[serde(rename = "O")]
    Passed,
    /// The test was skipped via the ignore list.
    #[serde(rename = "I")]
    Ignored,
    /// The test ran and failed its assertions.
    #[serde(rename = "F")]
    Failed,
    /// The test panicked its runner thread.
    #[serde(rename = "P")]
    Panic,
    /// Killed after exceeding the per-test wall-clock budget (`--timeout`).
    #[serde(rename = "T")]
    Timeout,
    /// The runner subprocess died abnormally (abort, signal, OOM kill).
    #[serde(rename = "C")]
    Crash,
    /// The harness itself could not run the test (spawn/IO/protocol failure).
    #[serde(rename = "H")]
    HarnessError,
}

impl TestOutcomeResult {
    /// `true` for abnormal outcomes that always deserve attention: panics,
    /// timeouts, crashes, and harness errors.
    #[must_use]
    pub const fn is_abnormal(self) -> bool {
        matches!(
            self,
            Self::Panic | Self::Timeout | Self::Crash | Self::HarnessError
        )
    }
}

/// Represents a tests statistic.
#[derive(Default, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Statistics {
    /// Total counted tests.
    #[serde(rename = "t")]
    pub total: usize,
    /// Passed tests.
    #[serde(rename = "o")]
    pub passed: usize,
    /// Ignored (skipped) tests.
    #[serde(rename = "i")]
    pub ignored: usize,
    /// Panicked tests.
    #[serde(rename = "p")]
    pub panic: usize,
    /// Tests killed after exceeding the per-test wall-clock budget (`--timeout`).
    #[serde(rename = "to", default)]
    pub timeout: usize,
    /// Tests whose runner subprocess died abnormally (abort, signal, OOM).
    #[serde(rename = "c", default)]
    pub crash: usize,
    /// Tests the harness itself could not run (spawn/IO/protocol failures).
    #[serde(rename = "h", default)]
    pub harness_error: usize,
}

impl Statistics {
    /// Number of plain failed tests: every counted test that is not passed,
    /// ignored, or one of the abnormal outcomes.
    #[must_use]
    pub const fn failed(self) -> usize {
        self.total
            .saturating_sub(self.passed)
            .saturating_sub(self.ignored)
            .saturating_sub(self.panic)
            .saturating_sub(self.timeout)
            .saturating_sub(self.crash)
            .saturating_sub(self.harness_error)
    }
}

impl Add for Statistics {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            total: self.total + rhs.total,
            passed: self.passed + rhs.passed,
            ignored: self.ignored + rhs.ignored,
            panic: self.panic + rhs.panic,
            timeout: self.timeout + rhs.timeout,
            crash: self.crash + rhs.crash,
            harness_error: self.harness_error + rhs.harness_error,
        }
    }
}

impl AddAssign for Statistics {
    fn add_assign(&mut self, rhs: Self) {
        self.total += rhs.total;
        self.passed += rhs.passed;
        self.ignored += rhs.ignored;
        self.panic += rhs.panic;
        self.timeout += rhs.timeout;
        self.crash += rhs.crash;
        self.harness_error += rhs.harness_error;
    }
}

#[cfg(test)]
mod tests {
    use super::{Statistics, TestOutcomeResult};

    /// The single-letter wire codes are a cross-crate contract (P1.1
    /// conventions): pin every letter in both directions.
    #[test]
    fn outcome_letters_round_trip() {
        let cases = [
            (TestOutcomeResult::Passed, "O"),
            (TestOutcomeResult::Ignored, "I"),
            (TestOutcomeResult::Failed, "F"),
            (TestOutcomeResult::Panic, "P"),
            (TestOutcomeResult::Timeout, "T"),
            (TestOutcomeResult::Crash, "C"),
            (TestOutcomeResult::HarnessError, "H"),
        ];
        for (outcome, letter) in cases {
            let json = serde_json::to_string(&outcome).expect("serialize");
            assert_eq!(json, format!("\"{letter}\""));
            let back: TestOutcomeResult = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, outcome);
        }
    }

    #[test]
    fn failed_derives_from_other_counters() {
        let stats = Statistics {
            total: 10,
            passed: 5,
            ignored: 1,
            panic: 1,
            timeout: 1,
            crash: 0,
            harness_error: 0,
        };
        assert_eq!(stats.failed(), 2);
        assert_eq!(Statistics::default().failed(), 0);
    }
}
