//! Per-side observation types for the differential pipeline.
//!
//! Both sides execute the identical composed program (see [`compose`]) and
//! report through the same two marker lines; this module parses those lines
//! plus the console stream into a [`RawObservation`]. Comparing observations
//! is the normalizer's job ([`normalize`]); running programs is split between
//! the Boa child ([`boa_side`]) and the oracle adapters ([`oracle`]).

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Value rendering produced by the composed program's `__diff_safe` helper.
///
/// Shapes are disjoint single-key objects so `untagged` parsing is exact:
/// `{s}` for stringified values, `{f}` for functions (source text is
/// engine-defined and never compared), `{u,t}` for values `String()` rejects,
/// `{g}` for the global object itself (host-defined `toStringTag`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ValueRepr {
    /// The value is identical to `globalThis` (host-defined rendering erased).
    Global {
        /// Always `true`; keeps the shape disjoint.
        g: bool,
    },
    /// `String(value)` succeeded.
    Str {
        /// Stringified value.
        s: String,
    },
    /// `typeof value === "function"`: compared by name only.
    Func {
        /// Function name (`"?"` when unreadable).
        f: String,
    },
    /// `String(value)` threw (e.g. `Object.create(null)` with no `toString`).
    Unstringifiable {
        /// Always `true`; keeps the shape disjoint from [`ValueRepr::Str`].
        u: bool,
        /// `typeof` the value (`"unknown"` when even access threw).
        t: String,
    },
}

/// Parsed `__DIFF_OUTCOME__:` marker line: what the program did.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OutcomeLine {
    /// `true` when the program completed without throwing.
    pub ok: bool,
    /// Thrown-value classification (`e.constructor.name` for objects,
    /// `null` for non-object throws); always `None` when `ok`.
    pub k: Option<String>,
    /// Completion value (`ok`) or thrown value (`!ok`) rendering.
    pub v: ValueRepr,
}

/// Parsed `__DIFF_STATE__:` marker line: globals the program added.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StateDump {
    /// `[name, rendering]` pairs in driver-sorted order (values rendered in
    /// sorted-key order, so coercion side effects are identical both sides).
    pub b: Vec<(String, ValueRepr)>,
    /// Added keys in engine enumeration order (compared exactly: order bugs
    /// are first-class signals, not rendering pollution).
    #[serde(default)]
    pub q: Vec<String>,
}

/// One side's complete observation of a case.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RawObservation {
    /// Parsed outcome marker.
    pub outcome: OutcomeLine,
    /// Parsed state marker (bindings sorted by name).
    pub state: StateDump,
    /// Non-marker stdout lines in emission order (program console output).
    pub console: Vec<String>,
    /// Raw stderr (oracle engine diagnostics; empty on the Boa side).
    pub stderr_text: String,
}

/// Marker prefixes emitted by the composed program.
pub const OUTCOME_MARKER: &str = "__DIFF_OUTCOME__:";
/// Marker prefixes emitted by the composed program.
pub const STATE_MARKER: &str = "__DIFF_STATE__:";

/// Parses side output into a [`RawObservation`].
///
/// `stdout_lines` is the child's stdout split into lines; `stderr_text` is
/// kept verbatim for engine-diagnostic rules. Requires exactly one outcome
/// marker and exactly one state marker — zero means the driver never ran
/// (engine-level failure), more than one means the program forged markers
/// (corpus construction rejects these statically; a second sighting here is
/// a loud protocol violation, never silently resolved).
pub fn parse_observation(
    stdout_lines: &[String],
    stderr_text: &str,
) -> Result<RawObservation, String> {
    let mut outcome_raw = None;
    let mut outcome_count = 0;
    let mut state_raw = None;
    let mut state_count = 0;
    let mut console = Vec::new();
    for line in stdout_lines {
        if let Some(payload) = line.strip_prefix(OUTCOME_MARKER) {
            outcome_count += 1;
            outcome_raw = Some(payload);
        } else if let Some(payload) = line.strip_prefix(STATE_MARKER) {
            state_count += 1;
            state_raw = Some(payload);
        } else {
            console.push(line.clone());
        }
    }
    if outcome_count != 1 {
        return Err(format!(
            "expected exactly 1 outcome marker, saw {outcome_count}"
        ));
    }
    if state_count != 1 {
        return Err(format!(
            "expected exactly 1 state marker, saw {state_count}"
        ));
    }
    let outcome: OutcomeLine =
        serde_json::from_str(&escape_lone_surrogates(outcome_raw.unwrap_or_default()))
            .map_err(|err| format!("unparsable outcome marker: {err}"))?;
    let mut state: StateDump =
        serde_json::from_str(&escape_lone_surrogates(state_raw.unwrap_or_default()))
            .map_err(|err| format!("unparsable state marker: {err}"))?;
    state.b.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(RawObservation {
        outcome,
        state,
        console,
        stderr_text: stderr_text.to_owned(),
    })
}

/// Re-escapes lone surrogates in marker JSON so `serde_json` accepts it.
///
/// `JSON.stringify` (identical on both engines) emits lone surrogates as
/// bare `\uXXXX` escapes, which strict JSON parsers reject; without this,
/// any observed string with a lone surrogate (well-formed-string tests,
/// surrogate-pair iteration) kills the whole observation on both sides.
/// Only sequences that fail to parse today are touched — valid JSON,
/// including well-formed surrogate pairs, passes through byte-identical —
/// so signatures for all comparable cases are unchanged. Residual masking
/// window (same honesty class as the 512-char render cap): an encoded lone
/// half parses to the 6-char text `\ud800`, colliding with a naturally
/// occurring backslash-u sequence; both sides share the codec, so this only
/// matters on exact cross-collisions.
fn escape_lone_surrogates(payload: &str) -> Cow<'_, str> {
    if !payload.contains("\\u") && !payload.contains("\\U") {
        return Cow::Borrowed(payload);
    }
    let bytes = payload.as_bytes();
    let mut out: Option<String> = None;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            let next = bytes.get(index + 1);
            if next == Some(&b'\\') {
                // Escaped backslash: literal text, not an escape leader.
                if let Some(owned) = out.as_mut() {
                    owned.push_str("\\\\");
                }
                index += 2;
                continue;
            }
            if (next == Some(&b'u') || next == Some(&b'U'))
                && let Some(half) = parse_hex4(bytes, index + 2)
            {
                let end = index + 6;
                if is_high_surrogate(half) && follows_low_surrogate(bytes, end) {
                    // Well-formed pair: copy both escapes verbatim.
                    if let Some(owned) = out.as_mut() {
                        owned.push_str(&payload[index..end + 6]);
                    }
                    index = end + 6;
                    continue;
                }
                if is_surrogate(half) {
                    // Lone half: double the backslash so it parses as the
                    // 6-char text, preserving comparability both sides.
                    let owned = out.get_or_insert_with(|| payload[..index].to_owned());
                    owned.push_str("\\\\");
                    owned.push_str(&payload[index + 1..end]);
                    index = end;
                    continue;
                }
                // Non-surrogate escape: copy verbatim.
                if let Some(owned) = out.as_mut() {
                    owned.push_str(&payload[index..end]);
                }
                index = end;
                continue;
            }
        }
        // Plain byte: copy one full char (never split UTF-8).
        let rest = &payload[index..];
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        if let Some(owned) = out.as_mut() {
            owned.push(ch);
        }
        index += ch.len_utf8();
    }
    match out {
        Some(owned) => Cow::Owned(owned),
        None => Cow::Borrowed(payload),
    }
}

/// Parses 4 hex digits at `start` (caller bounds-checks via `get`).
fn parse_hex4(bytes: &[u8], start: usize) -> Option<u16> {
    let mut value: u16 = 0;
    for offset in 0..4 {
        let digit = *bytes.get(start + offset)? as char;
        let nibble = u16::try_from(digit.to_digit(16)?).ok()?;
        value = value.checked_mul(16)?.checked_add(nibble)?;
    }
    Some(value)
}

/// True for UTF-16 high surrogates (`D800-DBFF`).
fn is_high_surrogate(half: u16) -> bool {
    (0xD800..0xDC00).contains(&half)
}

/// True for any UTF-16 surrogate half (`D800-DFFF`).
fn is_surrogate(half: u16) -> bool {
    (0xD800..0xE000).contains(&half)
}

/// True when a low-surrogate `\uXXXX` escape starts at `pos`.
fn follows_low_surrogate(bytes: &[u8], pos: usize) -> bool {
    if bytes.get(pos) != Some(&b'\\') {
        return false;
    }
    let next = bytes.get(pos + 1);
    if next != Some(&b'u') && next != Some(&b'U') {
        return false;
    }
    parse_hex4(bytes, pos + 2).is_some_and(|half| (0xDC00..0xE000).contains(&half))
}

#[cfg(test)]
mod tests {
    use super::{OUTCOME_MARKER, STATE_MARKER, ValueRepr, parse_observation};

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn parses_markers_and_splits_console() {
        let stdout = lines(&format!(
            "hello\n{OUTCOME_MARKER}{{\"ok\":true,\"k\":null,\"v\":{{\"s\":\"42\"}}}}\nworld\n{STATE_MARKER}{{\"b\":[[\"b\",{{\"s\":\"1\"}}],[\"a\",{{\"s\":\"2\"}}]]}}"
        ));
        let obs = parse_observation(&stdout, "").expect("parse");
        assert!(obs.outcome.ok);
        assert_eq!(obs.outcome.v, ValueRepr::Str { s: "42".to_owned() });
        assert_eq!(obs.console, ["hello", "world"]);
        // State bindings are sorted at parse so engines with different
        // enumeration orders still compare.
        let names: Vec<_> = obs.state.b.iter().map(|(name, _)| name.clone()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn missing_marker_is_an_error() {
        let stdout = lines("just console\n");
        let err = parse_observation(&stdout, "engine exploded").expect_err("must fail");
        assert!(err.contains("outcome marker"), "{err}");
    }

    #[test]
    fn forged_duplicate_marker_is_an_error() {
        let stdout = lines(&format!(
            "{OUTCOME_MARKER}{{\"ok\":true,\"k\":null,\"v\":{{\"s\":\"1\"}}}}\n{OUTCOME_MARKER}{{\"ok\":true,\"k\":null,\"v\":{{\"s\":\"2\"}}}}\n{STATE_MARKER}{{\"b\":[]}}"
        ));
        let err = parse_observation(&stdout, "").expect_err("must fail");
        assert!(err.contains("exactly 1 outcome"), "{err}");
    }

    #[test]
    fn unparsable_payload_is_an_error() {
        let stdout = lines(&format!(
            "{OUTCOME_MARKER}not-json\n{STATE_MARKER}{{\"b\":[]}}"
        ));
        let err = parse_observation(&stdout, "").expect_err("must fail");
        assert!(err.contains("unparsable outcome"), "{err}");
    }

    #[test]
    fn lone_surrogates_parse_as_escaped_text() {
        use super::escape_lone_surrogates;
        // Lone high half: strict JSON rejects; the codec doubles the
        // backslash so it parses as the 6-char text.
        let stdout = lines(&format!(
            "{OUTCOME_MARKER}{{\"ok\":true,\"k\":null,\"v\":{{\"s\":\"a\\ud800b\"}}}}\n{STATE_MARKER}{{\"b\":[]}}"
        ));
        let obs = parse_observation(&stdout, "").expect("lone high parses");
        assert_eq!(
            obs.outcome.v,
            ValueRepr::Str {
                s: "a\\ud800b".to_owned()
            }
        );
        // Lone low half: same treatment.
        let escaped = escape_lone_surrogates("{\"s\":\"x\\udc00\"}");
        assert_eq!(escaped.as_ref(), "{\"s\":\"x\\\\udc00\"}");
    }

    #[test]
    fn well_formed_pairs_and_plain_text_pass_through_untouched() {
        use super::escape_lone_surrogates;
        use std::borrow::Cow;
        // Surrogate pair still decodes to the astral character.
        let stdout = lines(&format!(
            "{OUTCOME_MARKER}{{\"ok\":true,\"k\":null,\"v\":{{\"s\":\"\\ud83d\\ude00\"}}}}\n{STATE_MARKER}{{\"b\":[]}}"
        ));
        let obs = parse_observation(&stdout, "").expect("pair parses");
        assert_eq!(
            obs.outcome.v,
            ValueRepr::Str {
                s: "\u{1F600}".to_owned()
            }
        );
        // No-u-escape payloads take the borrowed fast path (byte-identical).
        let plain = "{\"b\":[[\"a\",{\"s\":\"plain \\\\ backslash\"}]]}";
        assert!(matches!(escape_lone_surrogates(plain), Cow::Borrowed(_)));
        // Escaped backslash before 'u' is literal text, not an escape.
        let backslashed = "{\"s\":\"\\\\ud800\"}";
        assert!(matches!(
            escape_lone_surrogates(backslashed),
            Cow::Borrowed(_)
        ));
    }
}
