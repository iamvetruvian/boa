//! Oracle adapters: the `run_oracle` runner contract.
//!
//! Every oracle implements the same surface so adding engines (P3.5) is
//! mechanical: argument vector, version probe, and exit-code semantics.
//! Per-case execution goes through the shared supervisor ([`supervise`]),
//! so timeouts and pipe discipline are identical to the Boa side; parsing
//! the composed driver's marker lines is shared with it ([`capture`]).

use std::{ffi::OsString, path::Path};

use boa_outcomes::TestOutcomeResult;
use serde::{Deserialize, Serialize};

use crate::{
    capture::{RawObservation, parse_observation},
    supervise::{classify_oracle_child, supervise},
};

/// Supported oracle engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleKind {
    /// SpiderMonkey `jsshell` — the pinned v1 oracle.
    Jsshell,
    /// Node.js — the P3.5 second-oracle recipe demo.
    Node,
}

impl OracleKind {
    /// Parses the `--oracle-kind` CLI value.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "jsshell" => Some(Self::Jsshell),
            "node" => Some(Self::Node),
            _ => None,
        }
    }

    /// Adapter name for records and logs.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Jsshell => "jsshell",
            Self::Node => "node",
        }
    }

    /// Argument vector running `program` as a Classic script.
    ///
    /// Both shells run a file argument as a script with the filename in
    /// diagnostics (stable for the normalizer's location stripper).
    pub fn argv(program: &Path) -> Vec<OsString> {
        vec![program.as_os_str().to_owned()]
    }

    /// Arguments printing the engine version string.
    pub const fn version_argv(self) -> &'static [&'static str] {
        match self {
            Self::Jsshell | Self::Node => &["--version"],
        }
    }

    /// Probes the binary's version string (checked against the pin at startup).
    pub fn probe_version(self, binary: &Path) -> Result<String, String> {
        let child = supervise(binary, self.version_argv(), 30);
        if !matches!(child.end, crate::supervise::ChildEnd::Exited(0)) {
            return Err(format!("oracle version probe failed: {:?}", child.end));
        }
        Ok(child.stdout.trim().to_owned())
    }
}

/// One oracle-side case result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleOutcome {
    /// Side outcome (`Passed` always carries an observation).
    pub outcome: TestOutcomeResult,
    /// Human summary (empty on `Passed`).
    pub text: String,
    /// Parsed observation when the driver emitted valid markers.
    pub observation: Option<RawObservation>,
}

/// Runs one composed program under the oracle: `run_oracle(binary, program, timeout)`.
///
/// Returns the raw side result; comparing against the Boa side is the
/// normalizer's job. A naturally-exited oracle that emitted no valid
/// markers is a case-level `Failed` (the driver never ran to completion),
/// never a silent skip.
pub fn run_oracle(
    _kind: OracleKind,
    binary: &Path,
    composed_path: &Path,
    timeout_secs: u64,
) -> OracleOutcome {
    let child = supervise(binary, &OracleKind::argv(composed_path), timeout_secs);
    let (outcome, text) = classify_oracle_child(&child);
    if outcome != TestOutcomeResult::Passed {
        return OracleOutcome {
            outcome,
            text,
            observation: None,
        };
    }
    let lines: Vec<String> = child.stdout.lines().map(str::to_owned).collect();
    match parse_observation(&lines, &child.stderr) {
        Ok(observation) => OracleOutcome {
            outcome,
            text,
            observation: Some(observation),
        },
        Err(err) => OracleOutcome {
            outcome: TestOutcomeResult::Failed,
            text: format!("oracle emitted no valid markers: {err}"),
            observation: None,
        },
    }
}

/// Asserts an [`OsStr`] slice equals `expected` (argv-shape test helper).
#[cfg(test)]
fn argv_names(argv: &[OsString]) -> Vec<String> {
    argv.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{OracleKind, argv_names};
    use std::path::Path;

    #[test]
    fn oracle_kinds_parse() {
        assert_eq!(OracleKind::from_name("jsshell"), Some(OracleKind::Jsshell));
        assert_eq!(OracleKind::from_name("node"), Some(OracleKind::Node));
        assert_eq!(OracleKind::from_name("d8"), None);
    }

    #[test]
    fn argv_runs_program_file() {
        let argv = argv_names(&OracleKind::argv(Path::new("case.js")));
        assert_eq!(argv, ["case.js"]);
        for kind in [OracleKind::Jsshell, OracleKind::Node] {
            assert_eq!(kind.version_argv(), ["--version"]);
        }
    }
}
