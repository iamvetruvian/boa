//! Explicit behavior-normalization table.
//!
//! [`compare`] normalizes both sides' [`RawObservation`] through every rule
//! below, then diffs field by field. Rules remove only known-benign,
//! engine-defined variation (message text, stack traces, timing identities);
//! everything else compares exactly. Each rule has fixtures proving what it
//! erases AND what it can never mask (a companion field that still diffs).
//!
//! Deliberately absent: `undefined`-completion vs empty-stdout unification
//! (the composed driver always emits markers, so both sides are symmetric),
//! error-kind aliasing (exact constructor-name match; any future alias needs
//! its own rule + fixtures).

use serde::{Deserialize, Serialize};

use crate::capture::{RawObservation, ValueRepr};

/// One normalized field difference between the Boa and oracle sides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Field name (`completion`, `kind`, `value`, `console`, `state`,
    /// `state-order`, `stderr`).
    pub field: String,
    /// Boa side rendering.
    pub boa: String,
    /// Oracle side rendering.
    pub oracle: String,
}

/// Normalized observation: all engine-defined text erased, everything else kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// Program completed (`true`) vs threw (`false`).
    pub completed: bool,
    /// Thrown constructor name. Compared exactly (no alias table in v1).
    pub kind: Option<String>,
    /// Completion/thrown value rendering with timing redactions applied.
    pub value: String,
    /// Console lines in emission order with timing redactions applied.
    pub console: Vec<String>,
    /// Sorted `(name, rendering)` state bindings with redactions applied.
    pub state: Vec<(String, String)>,
    /// Added keys in engine enumeration order (compared exactly).
    pub state_order: Vec<String>,
    /// Stderr with stack/caret/location noise dropped (R2).
    pub stderr: String,
}

/// R1 — error message text is never compared.
///
/// Only the thrown constructor name enters [`Normalized::kind`]; message
/// text is engine-defined (and the driver never reports it). What this can
/// never mask: a kind change still diffs (fixture below).
fn rule_error_kind_only(kind: Option<&str>) -> Option<String> {
    kind.map(str::to_owned)
}

/// Native error kinds whose messages are engine-defined.
///
/// A thrown `TypeError`'s `String()` rendering is `TypeError: <engine
/// message>` — the name compares via R1, the message must be erased (R1b).
/// Custom error names carry user-supplied messages (identical on both sides
/// for the same program), so only these known kinds canonicalize.
const NATIVE_ERROR_KINDS: &[&str] = &[
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
];

/// R1b — thrown native-error renderings canonicalize to their kind.
///
/// `String(err)` for a native error embeds the engine's message text
/// (often with line/column notes), which legitimately differs between
/// engines. What this can never mask: non-error throws (`throw 42`,
/// `throw "x"`) and custom-named throws compare exactly (fixtures below).
fn rule_thrown_value(ok: bool, kind: Option<&str>, rendered: String) -> String {
    if !ok
        && let Some(kind) = kind
        && NATIVE_ERROR_KINDS.contains(&kind)
    {
        return format!("throw:{kind}");
    }
    rendered
}

/// R2 — stack traces, caret lines, and `file:line:col` prefixes are dropped.
///
/// Applies to stderr (oracle engine diagnostics). The first surviving line
/// still compares, so a stderr-bearing success on one side only is a loud
/// mismatch — stderr is never silently ignored.
fn rule_stderr(stderr: &str) -> String {
    let mut kept = Vec::new();
    let mut in_stack = false;
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed == "Stack:" {
            in_stack = true;
            continue;
        }
        if in_stack {
            if trimmed.starts_with('@') || (trimmed.contains('@') && trimmed.contains(".js:")) {
                continue;
            }
            in_stack = false;
        }
        // Caret diagnostics (`....^`) and echoed source lines carry no signal.
        if trimmed.chars().all(|c| c == '.' || c == '^' || c == ' ') && trimmed.contains('^') {
            continue;
        }
        // Strip a leading `file:line:col ` prefix; keep the message.
        kept.push(strip_location_prefix(trimmed).to_owned());
    }
    kept.join("\n")
}

/// Strips a leading `<path>:<line>:<col> ` prefix when present.
///
/// Only commits when both location segments are all digits; anything else
/// (e.g. `warning: something`, `TypeError: boom`) is returned unchanged.
fn strip_location_prefix(line: &str) -> &str {
    let Some((path, rest)) = line.split_once(':') else {
        return line;
    };
    if path.contains(' ') {
        return line;
    }
    let Some((lineno, rest)) = rest.split_once(':') else {
        return line;
    };
    if lineno.is_empty() || !lineno.bytes().all(|b| b.is_ascii_digit()) {
        return line;
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return line;
    }
    rest[digits..]
        .strip_prefix(' ')
        .map_or(line, str::trim_start)
}

/// R3 — bare millisecond-timestamp lines are timing identities.
///
/// A console line (or whole value rendering) that is exactly 13 ASCII
/// digits is `Date.now()`-shaped wall-clock output: Boa runs on a frozen
/// clock, the oracle on real time, so these can never agree literally.
/// Redaction fires ONLY on full-string matches — digits embedded in longer
/// text still compare (never-masks fixture below).
fn rule_redact_timestamp(text: &str) -> String {
    if text.len() == 13 && text.bytes().all(|b| b.is_ascii_digit()) {
        "<TIME-MS>".to_owned()
    } else {
        text.to_owned()
    }
}

/// R5 — functions compare by name only (source text is engine-defined).
///
/// Enforcement starts at capture ([`ValueRepr::Func`] carries no source);
/// this renders the compared form. [`ValueRepr::Global`] (identity with
/// `globalThis`, R10) renders canonically: the global's `toStringTag` is
/// normative `"global"`
/// (<https://tc39.es/ecma262/#sec-setdefaultglobalbindings>), so identity —
/// not the rendering — is compared. What this can never mask: a genuine
/// global-vs-plain disagreement still diffs (only the identical side
/// renders `<globalThis>`); and a global-tag rendering diff on a
/// non-identical path (array element, thrown message) is a real engine
/// signal (regression `globalthis-tostringtag-missing`), never benign
/// host variation.
fn rule_render_value(value: &ValueRepr) -> String {
    match value {
        ValueRepr::Global { .. } => "<globalThis>".to_owned(),
        ValueRepr::Str { s } => rule_redact_timestamp(s),
        ValueRepr::Func { f } => format!("function:{f}"),
        ValueRepr::Unstringifiable { t, .. } => format!("<unstringifiable:{t}>"),
    }
}

/// Normalizes one side's observation.
pub fn normalize(obs: &RawObservation) -> Normalized {
    Normalized {
        completed: obs.outcome.ok,
        kind: rule_error_kind_only(obs.outcome.k.as_deref()),
        value: rule_thrown_value(
            obs.outcome.ok,
            obs.outcome.k.as_deref(),
            rule_render_value(&obs.outcome.v),
        ),
        console: obs
            .console
            .iter()
            .map(|line| rule_redact_timestamp(line))
            .collect(),
        state: obs
            .state
            .b
            .iter()
            .map(|(name, value)| (name.clone(), rule_render_value(value)))
            .collect(),
        state_order: obs.state.q.clone(),
        stderr: rule_stderr(&obs.stderr_text),
    }
}

/// Compares two observations after normalization.
///
/// Returns the field diffs (empty = agree). Console order is significant
/// (R7); state order is capture-sorted on both sides (R8).
pub fn compare(boa: &RawObservation, oracle: &RawObservation) -> Vec<FieldDiff> {
    let boa = normalize(boa);
    let oracle = normalize(oracle);
    let mut diffs = Vec::new();
    if boa.completed != oracle.completed {
        diffs.push(FieldDiff {
            field: "completion".to_owned(),
            boa: completed_word(boa.completed).to_owned(),
            oracle: completed_word(oracle.completed).to_owned(),
        });
    }
    if boa.kind != oracle.kind {
        diffs.push(FieldDiff {
            field: "kind".to_owned(),
            boa: kind_word(boa.kind.as_deref()).to_owned(),
            oracle: kind_word(oracle.kind.as_deref()).to_owned(),
        });
    }
    if boa.value != oracle.value {
        diffs.push(FieldDiff {
            field: "value".to_owned(),
            boa: boa.value,
            oracle: oracle.value,
        });
    }
    if boa.console != oracle.console {
        diffs.push(FieldDiff {
            field: "console".to_owned(),
            boa: format!("{:?}", boa.console),
            oracle: format!("{:?}", oracle.console),
        });
    }
    if boa.state != oracle.state {
        diffs.push(FieldDiff {
            field: "state".to_owned(),
            boa: format!("{:?}", boa.state),
            oracle: format!("{:?}", oracle.state),
        });
    }
    if boa.state_order != oracle.state_order {
        diffs.push(FieldDiff {
            field: "state-order".to_owned(),
            boa: format!("{:?}", boa.state_order),
            oracle: format!("{:?}", oracle.state_order),
        });
    }
    if boa.stderr != oracle.stderr {
        diffs.push(FieldDiff {
            field: "stderr".to_owned(),
            boa: boa.stderr,
            oracle: oracle.stderr,
        });
    }
    diffs
}

/// Human word for a completion flag.
const fn completed_word(completed: bool) -> &'static str {
    if completed { "completed" } else { "threw" }
}

/// Human word for an optional thrown kind.
fn kind_word(kind: Option<&str>) -> &str {
    kind.unwrap_or("<no-throw>")
}

#[cfg(test)]
mod tests {
    use super::{compare, rule_redact_timestamp, rule_stderr};
    use crate::capture::{OutcomeLine, RawObservation, StateDump, ValueRepr};

    fn observation(ok: bool, kind: Option<&str>, value: &str, console: &[&str]) -> RawObservation {
        RawObservation {
            outcome: OutcomeLine {
                ok,
                k: kind.map(str::to_owned),
                v: ValueRepr::Str {
                    s: value.to_owned(),
                },
            },
            state: StateDump {
                b: Vec::new(),
                q: Vec::new(),
            },
            console: console.iter().map(|line| (*line).to_owned()).collect(),
            stderr_text: String::new(),
        }
    }

    #[test]
    fn identical_observations_agree() {
        let obs = observation(true, None, "42", &["hi"]);
        assert!(compare(&obs, &obs).is_empty());
    }

    #[test]
    fn r1_kind_change_still_diffs() {
        // Message text never enters the observation (driver reports kinds),
        // so same-kind/different-message trivially agrees; a changed kind
        // must diff (this rule can never mask it).
        let boa = observation(false, Some("TypeError"), "x", &[]);
        let oracle = observation(false, Some("RangeError"), "x", &[]);
        let diffs = compare(&boa, &oracle);
        assert!(diffs.iter().any(|diff| diff.field == "kind"), "{diffs:?}");
    }

    #[test]
    fn r1b_native_error_messages_canonicalize() {
        // Same-kind native throws with different engine messages agree
        // (observed live: Boa vs jsshell SyntaxError renderings).
        let boa = observation(
            false,
            Some("SyntaxError"),
            "SyntaxError: unexpected token ';' at line 1",
            &[],
        );
        let oracle = observation(
            false,
            Some("SyntaxError"),
            "SyntaxError: expected expression, got ';'",
            &[],
        );
        assert!(compare(&boa, &oracle).is_empty());
        // ...but primitive and custom-named throws compare exactly: this
        // rule can never mask them.
        let throw42 = observation(false, None, "42", &[]);
        let throw43 = observation(false, None, "43", &[]);
        assert!(!compare(&throw42, &throw43).is_empty());
        let custom_a = observation(false, Some("MyErr"), "MyErr: m1", &[]);
        let custom_b = observation(false, Some("MyErr"), "MyErr: m2", &[]);
        assert!(!compare(&custom_a, &custom_b).is_empty());
        // Completion values are untouched by the throw rule.
        let done_a = observation(true, None, "1", &[]);
        let done_b = observation(true, None, "2", &[]);
        assert!(!compare(&done_a, &done_b).is_empty());
    }

    #[test]
    fn r2_drops_stacks_but_keeps_first_line() {
        let stderr = "case.js:1:7 TypeError: boom\nStack:\n  @case.js:1:7\n";
        assert_eq!(rule_stderr(stderr), "TypeError: boom");
        // A stderr-bearing success on one side only still diffs: stderr is
        // compared, never silently ignored.
        let mut boa = observation(true, None, "1", &[]);
        boa.stderr_text = "warning: something\n".to_owned();
        let oracle = observation(true, None, "1", &[]);
        let diffs = compare(&boa, &oracle);
        assert!(diffs.iter().any(|diff| diff.field == "stderr"), "{diffs:?}");
    }

    #[test]
    fn r3_redacts_bare_timestamps_only() {
        assert_eq!(rule_redact_timestamp("1759420000000"), "<TIME-MS>");
        // Embedded digits are data, not identities: never redacted.
        assert_eq!(
            rule_redact_timestamp("count=1759420000000"),
            "count=1759420000000"
        );
        assert_eq!(rule_redact_timestamp("0"), "0");
        let boa = observation(true, None, "1759420000000", &[]);
        let oracle = observation(true, None, "1759420001234", &[]);
        assert!(compare(&boa, &oracle).is_empty());
        // ...but a timestamp-adjacent real difference still diffs.
        let oracle2 = observation(true, None, "1759420001234!", &[]);
        assert!(!compare(&boa, &oracle2).is_empty());
    }

    #[test]
    fn r10_global_identity_canonicalizes() {
        let global = RawObservation {
            outcome: OutcomeLine {
                ok: true,
                k: None,
                v: ValueRepr::Global { g: true },
            },
            state: StateDump {
                b: Vec::new(),
                q: Vec::new(),
            },
            console: Vec::new(),
            stderr_text: String::new(),
        };
        assert!(compare(&global, &global).is_empty());
        // A genuine global-vs-plain disagreement still diffs: only the
        // identical side renders `<globalThis>`.
        let plain = observation(true, None, "[object Object]", &[]);
        assert!(!compare(&global, &plain).is_empty());
    }

    #[test]
    fn r5_functions_compare_by_name() {
        let func = |name: &str| RawObservation {
            outcome: OutcomeLine {
                ok: true,
                k: None,
                v: ValueRepr::Func { f: name.to_owned() },
            },
            state: StateDump {
                b: Vec::new(),
                q: Vec::new(),
            },
            console: Vec::new(),
            stderr_text: String::new(),
        };
        assert!(compare(&func("f"), &func("f")).is_empty());
        assert!(!compare(&func("f"), &func("g")).is_empty());
    }

    #[test]
    fn state_order_diffs_while_bindings_agree() {
        // Same bindings, different engine enumeration order: the order
        // field diffs (this is how the global-var key-order bug surfaces
        // directly; values render in sorted order so coercions agree).
        let mut boa = observation(true, None, "1", &[]);
        let mut oracle = observation(true, None, "1", &[]);
        let pair = |name: &str| (name.to_owned(), ValueRepr::Str { s: "0".to_owned() });
        boa.state.b = vec![pair("a"), pair("b")];
        boa.state.q = vec!["b".to_owned(), "a".to_owned()];
        oracle.state.b = vec![pair("a"), pair("b")];
        oracle.state.q = vec!["a".to_owned(), "b".to_owned()];
        let diffs = compare(&boa, &oracle);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "state-order");
    }

    #[test]
    fn r7_console_order_is_significant() {
        let boa = observation(true, None, "1", &["a", "b"]);
        let oracle = observation(true, None, "1", &["b", "a"]);
        let diffs = compare(&boa, &oracle);
        assert!(
            diffs.iter().any(|diff| diff.field == "console"),
            "{diffs:?}"
        );
    }

    #[test]
    fn completion_and_value_diffs_named() {
        let boa = observation(true, None, "1", &[]);
        let oracle = observation(false, Some("Error"), "1", &[]);
        let fields: Vec<_> = compare(&boa, &oracle)
            .iter()
            .map(|diff| diff.field.clone())
            .collect();
        assert!(fields.contains(&"completion".to_owned()), "{fields:?}");
        assert!(fields.contains(&"kind".to_owned()), "{fields:?}");
    }
}
