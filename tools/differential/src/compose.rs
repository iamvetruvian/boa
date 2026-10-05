//! Identical-both-sides program composition.
//!
//! [`compose`] embeds a case program into a driver script that runs it via
//! indirect `eval`, then reports through the `__DIFF_OUTCOME__:` and
//! `__DIFF_STATE__:` marker lines. Boa and the oracle execute byte-identical
//! driver text, so any driver artifact (line offsets, `eval` indirection,
//! shimmed globals) is symmetric and cancels out of the comparison.
//!
//! The driver is strict-safe: every driver binding is a function-local
//! `var` (the whole driver runs inside one IIFE) or a `globalThis` property
//! assignment, so `"use strict"` programs cannot turn the driver itself
//! into a throw — and a program that freezes the global object cannot break
//! observation either (sloppy writes to a frozen global fail silently, but
//! locals never touch it).
//!
//! The driver is also intrinsic-poison-safe: every intrinsic the epilogue
//! needs (`Object.keys`, `Array.prototype.sort`, the report sink, ...) is
//! captured into a `var` BEFORE the program runs, and the epilogue calls
//! captured values directly — never a method lookup a program could have
//! replaced (no `x.sort()`, no `fn.call()`, only
//! `__diff_apply(captured, ...)`). Marker emission deliberately avoids
//! `JSON.stringify`: a program can poison serialization globally (an
//! inherited `Object.prototype.toJSON`, a `JSON.stringify` override), so
//! outcomes and state pairs are pre-serialized by a minimal `__diff_q`
//! quoter (indexed reads plus captured `charCodeAt` only) that emits
//! byte-identical JSON for every representable payload — lone surrogates,
//! unrepresentable downstream (Rust `String`), render as U+FFFD on both
//! sides. Cases that poison shared intrinsics would otherwise break
//! observation on both sides at once. The epilogue's working containers
//! are null-prototype objects, so indexed properties a program defines on
//! `Array.prototype` (e.g. a non-writable `"0"`) cannot break the
//! epilogue's own push/sort through prototype-chain `Set` lookup.
//! Rendered values are capped at 512 chars (`__diff_trunc`, with a length
//! suffix)
//! so a megabyte global cannot bloat the markers past the supervisor's
//! bounded capture and SIGPIPE both children.

use cow_utils::CowUtils;

use crate::capture::{OUTCOME_MARKER, STATE_MARKER};

/// Placeholder in [`DRIVER_TEMPLATE`] replaced with the JSON-escaped program.
const PROGRAM_PLACEHOLDER: &str = "__DIFF_PROGRAM_JSON__";

/// Driver template. `__DIFF_PROGRAM_JSON__` is the only substitution point.
///
/// `__diff_safe` renders values without ever throwing out of the driver;
/// `__diff_kind` classifies thrown values by constructor name (objects) or
/// `null` (primitives); the state epilogue diffs `globalThis` keys against
/// the pre-program snapshot so each engine's distinct host globals never
/// enter the comparison.
const DRIVER_TEMPLATE: &str = r#"
// Whole-driver IIFE: every driver binding below is a function-local `var`,
// so a program that freezes the global object (test262's own
// set-property-non-extensible case does) cannot break observation — sloppy
// writes to a frozen global fail silently, but locals never touch it. The
// program still runs at global scope (indirect eval), and only the
// print/console shims (installed before the program runs) are global.
(function () {
var __diff_src = __DIFF_PROGRAM_JSON__;
// The global object itself, captured before the program runs: tests can
// delete the `globalThis` binding (test262's property-descriptor test does
// via isConfigurable), and every post-program use must survive that. The
// `__diff_` prefix keeps this out of state diffs (name filter + baseline).
var __diff_globalThis = globalThis;
var __diff_apply = Reflect.apply;
var __diff_report = null;
if (typeof globalThis.print !== "undefined") { __diff_report = globalThis.print; }
if (__diff_report === null && typeof globalThis.console !== "undefined") {
    var __diff_console_log = globalThis.console.log;
    __diff_report = function (__diff_s) { return __diff_apply(__diff_console_log, null, [__diff_s]); };
}
function __diff_shim_report() {
    if (__diff_report === null) { return undefined; }
    return __diff_apply(__diff_report, null, arguments);
}
if (typeof globalThis.print === "undefined") { globalThis.print = __diff_shim_report; }
if (typeof globalThis.console === "undefined") {
    globalThis.console = {
        log: __diff_shim_report,
        info: __diff_shim_report,
        warn: __diff_shim_report,
        error: __diff_shim_report,
        debug: __diff_shim_report
    };
}
var __diff_Object_keys = Object.keys;
var __diff_String = String;
var __diff_Array_indexOf = Array.prototype.indexOf;
var __diff_Array_push = Array.prototype.push;
var __diff_Array_slice = Array.prototype.slice;
var __diff_Array_sort = Array.prototype.sort;
var __diff_String_slice = String.prototype.slice;
var __diff_String_charCodeAt = String.prototype.charCodeAt;
var __diff_Object_create = Object.create;
var __diff_before = __diff_Object_keys(globalThis);
function __diff_trunc(__diff_s) {
    if (__diff_s.length <= 512) { return __diff_s; }
    return __diff_apply(__diff_String_slice, __diff_s, [0, 512]) + "...<" + __diff_s.length + " chars>";
}
// Minimal JSON string quoter (see module docs): emits byte-identical
// output to JSON.stringify for every representable payload using only
// indexed reads and captured charCodeAt. Lone surrogates render as
// U+FFFD; valid pairs pass through raw, like stringify.
var __diff_hex = "0123456789abcdef";
function __diff_q(__diff_s) {
    var __diff_out = "\"";
    for (var __diff_i = 0; __diff_i < __diff_s.length; __diff_i++) {
        var __diff_c = __diff_apply(__diff_String_charCodeAt, __diff_s, [__diff_i]);
        if (__diff_c === 34) { __diff_out += "\\\""; }
        else if (__diff_c === 92) { __diff_out += "\\\\"; }
        else if (__diff_c === 10) { __diff_out += "\\n"; }
        else if (__diff_c === 13) { __diff_out += "\\r"; }
        else if (__diff_c === 9) { __diff_out += "\\t"; }
        else if (__diff_c === 8) { __diff_out += "\\b"; }
        else if (__diff_c === 12) { __diff_out += "\\f"; }
        else if (__diff_c < 32) { __diff_out += "\\u00" + __diff_hex[__diff_c >> 4] + __diff_hex[__diff_c & 15]; }
        else if (__diff_c >= 55296 && __diff_c <= 56319) {
            var __diff_lo = __diff_apply(__diff_String_charCodeAt, __diff_s, [__diff_i + 1]);
            if (__diff_lo >= 56320 && __diff_lo <= 57343) { __diff_out += __diff_s[__diff_i] + __diff_s[__diff_i + 1]; __diff_i++; }
            else { __diff_out += "\\ufffd"; }
        }
        else if (__diff_c >= 56320 && __diff_c <= 57343) { __diff_out += "\\ufffd"; }
        else { __diff_out += __diff_s[__diff_i]; }
    }
    return __diff_out + "\"";
}
// Safe-value JSON fragment, pre-serialized at the source.
function __diff_safe(__diff_v) {
    if (__diff_v === __diff_globalThis) { return "{\"g\":true}"; }
    var __diff_t = typeof __diff_v;
    if (__diff_t === "function") {
        var __diff_n;
        try { __diff_n = __diff_trunc(__diff_String(__diff_v.name)); } catch (__diff_e) { __diff_n = "?"; }
        return "{\"f\":" + __diff_q(__diff_n) + "}";
    }
    try { return "{\"s\":" + __diff_q(__diff_trunc(__diff_String(__diff_v))) + "}"; }
    catch (__diff_e) { return "{\"u\":true,\"t\":" + __diff_q(__diff_t) + "}"; }
}
// Error-kind JSON fragment: quoted name or null.
function __diff_kind(__diff_e) {
    if ((typeof __diff_e === "object" && __diff_e !== null) || typeof __diff_e === "function") {
        try {
            var __diff_c = __diff_e.constructor;
            if (__diff_c !== undefined && __diff_c !== null) { return __diff_q(__diff_String(__diff_c.name)); }
        } catch (__diff_x) {}
        return __diff_q(typeof __diff_e);
    }
    return "null";
}
// The IIFE adds no globals of its own, so this filter matches nothing in
// practice; it stays so state semantics cannot shift under programs that
// name their own `__diff_*` globals.
function __diff_is_driver_key(__diff_k) {
    return __diff_k[0] === "_" && __diff_k[1] === "_" && __diff_k[2] === "d" && __diff_k[3] === "i" && __diff_k[4] === "f" && __diff_k[5] === "f";
}
var __diff_outcome;
try {
    var __diff_completion = (0, eval)(__diff_src);
    __diff_outcome = "{\"ok\":true,\"k\":null,\"v\":" + __diff_safe(__diff_completion) + "}";
} catch (__diff_err) {
    __diff_outcome = "{\"ok\":false,\"k\":" + __diff_kind(__diff_err) + ",\"v\":" + __diff_safe(__diff_err) + "}";
}
if (__diff_report !== null) { __diff_report("__DIFF_OUTCOME__:" + __diff_outcome); }
// Null-prototype working containers: a program that defines indexed
// properties on Array.prototype (e.g. a non-writable "0") would otherwise
// break the epilogue's own push/sort via prototype-chain Set lookup.
// Captured generics operate on these without ever consulting a prototype.
var __diff_added = __diff_Object_create(null);
__diff_added.length = 0;
var __diff_keys = __diff_Object_keys(__diff_globalThis);
for (var __diff_i = 0; __diff_i < __diff_keys.length; __diff_i++) {
    var __diff_key = __diff_keys[__diff_i];
    if (__diff_is_driver_key(__diff_key)) { continue; }
    if (__diff_apply(__diff_Array_indexOf, __diff_before, [__diff_key]) !== -1) { continue; }
    __diff_apply(__diff_Array_push, __diff_added, [__diff_key]);
}
var __diff_order = __diff_apply(__diff_Array_slice, __diff_added, []);
__diff_apply(__diff_Array_sort, __diff_added, []);
var __diff_pairs = __diff_Object_create(null);
__diff_pairs.length = 0;
for (var __diff_j = 0; __diff_j < __diff_added.length; __diff_j++) {
    var __diff_name = __diff_added[__diff_j];
    var __diff_val;
    try { __diff_val = __diff_safe(__diff_globalThis[__diff_name]); }
    catch (__diff_x) { __diff_val = "{\"u\":true,\"t\":\"unknown\"}"; }
    __diff_apply(__diff_Array_push, __diff_pairs, ["[" + __diff_q(__diff_name) + "," + __diff_val + "]"]);
}
// Manual join: Array.prototype.join stays uncalled (a program could have
// replaced it). Indexed reads only, like the quoter above.
var __diff_b = "";
for (var __diff_m = 0; __diff_m < __diff_pairs.length; __diff_m++) {
    if (__diff_m > 0) { __diff_b += ","; }
    __diff_b += __diff_pairs[__diff_m];
}
var __diff_qs = "";
for (var __diff_p = 0; __diff_p < __diff_order.length; __diff_p++) {
    if (__diff_p > 0) { __diff_qs += ","; }
    __diff_qs += __diff_q(__diff_order[__diff_p]);
}
if (__diff_report !== null) { __diff_report("__DIFF_STATE__:{\"b\":[" + __diff_b + "],\"q\":[" + __diff_qs + "]}"); }
})();
"#;

/// Composes a case program into the shared driver script.
///
/// The program is embedded as a JSON string literal (JSON strings are valid
/// JS string literals), so arbitrary program bytes — quotes, newlines,
/// unicode — survive embedding unchanged.
pub fn compose(program: &str) -> String {
    let embedded = serde_json::to_string(program).unwrap_or_else(|_| "\"\"".to_owned());
    DRIVER_TEMPLATE
        .cow_replace(PROGRAM_PLACEHOLDER, embedded.as_str())
        .into_owned()
}

/// Rejects programs that could forge driver markers or clobber driver state.
///
/// A program containing a marker prefix could print a second marker line
/// (capture treats duplicates as a protocol violation), and a program
/// mentioning `__diff_` could read or overwrite driver bindings. Corpus
/// loading applies this check to every program; the composed driver itself
/// is exempt because it is generated, not loaded.
pub fn check_program_clean(program: &str) -> Result<(), String> {
    for forbidden in [OUTCOME_MARKER, STATE_MARKER, "__diff_"] {
        if program.contains(forbidden) {
            return Err(format!(
                "program contains forbidden driver token {forbidden:?}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check_program_clean, compose};

    #[test]
    fn embeds_program_as_escaped_literal() {
        let composed = compose("print(\"a\\nb\"); 'x\\\\y';");
        let embedded = serde_json::to_string("print(\"a\\nb\"); 'x\\\\y';").expect("json");
        assert!(
            composed.contains(&format!("var __diff_src = {embedded};")),
            "{composed}"
        );
        assert!(!composed.contains("__DIFF_PROGRAM_JSON__"));
    }

    #[test]
    fn driver_assigns_only_declared_bindings() {
        // Strict-safety: a bare `name = ...` is legal under "use strict"
        // only when `name` is an already-declared binding. Collect the
        // driver's declared identifiers (`var`/`function`/parameters/catch
        // bindings), then require every bare assignment target to be one.
        let template = super::DRIVER_TEMPLATE;
        let mut declared = std::collections::BTreeSet::new();
        for name in ["print", "console", "eval", "Object", "JSON", "String"] {
            declared.insert(name.to_owned());
        }
        let mut words = template
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
            .peekable();
        while let Some(word) = words.next() {
            if (word == "var" || word == "function")
                && let Some(name) = words.next()
                && !name.is_empty()
            {
                declared.insert(name.to_owned());
            }
            if word == "catch" {
                for next in words.by_ref() {
                    if next.is_empty() {
                        continue;
                    }
                    declared.insert(next.to_owned());
                    break;
                }
            }
        }
        // Function parameters are declared bindings too.
        for segment in template.split("function") {
            if let Some(params) = segment
                .strip_prefix('(')
                .and_then(|rest| rest.split(')').next())
            {
                for param in params
                    .split(',')
                    .map(str::trim)
                    .filter(|param| !param.is_empty())
                {
                    declared.insert(param.to_owned());
                }
            } else if let Some(params) = segment
                .split('(')
                .nth(1)
                .and_then(|rest| rest.split(')').next())
            {
                for param in params
                    .split(',')
                    .map(str::trim)
                    .filter(|param| !param.is_empty())
                {
                    if param
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                    {
                        declared.insert(param.to_owned());
                    }
                }
            }
        }
        for line in template.lines() {
            for segment in line.split([';', '{', '}']) {
                let trimmed = segment.trim();
                let Some((target, _)) = trimmed.split_once('=') else {
                    continue;
                };
                let target = target.trim();
                // Skip comparisons, properties, declarations, and keywords.
                if target.contains([
                    ' ', '.', '(', ')', '[', ']', '"', '\'', ':', ',', '!', '<', '>', '+', '-',
                    '*', '/', '%', '&', '|',
                ]) {
                    continue;
                }
                if target.is_empty() || target == "var" || target == "return" {
                    continue;
                }
                assert!(
                    declared.contains(target),
                    "driver assigns undeclared binding {target:?} (throws under \"use strict\"): {line}"
                );
            }
        }
    }

    #[test]
    fn rejects_marker_forging_programs() {
        assert!(check_program_clean("print(1);").is_ok());
        for bad in [
            "print(\"__DIFF_OUTCOME__:evil\");",
            "print(\"__DIFF_STATE__:evil\");",
            "var __diff_src = 1;",
        ] {
            assert!(check_program_clean(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn driver_emits_markers_without_stringify_call() {
        // Marker emission must never call JSON.stringify: an inherited
        // Object.prototype.toJSON (or a stringify override) used to turn
        // markers unparsable on both sides at once (no-boolean-toJSON).
        let template = super::DRIVER_TEMPLATE;
        assert!(!template.contains("JSON.stringify("), "{template}");
        assert!(!template.contains("__diff_JSON_stringify"), "{template}");
    }

    #[test]
    fn driver_state_is_function_scoped() {
        // A program that freezes the global object (set-property-
        // non-extensible) silently breaks every sloppy global write after
        // the freeze; function-locals never touch the global.
        let template = super::DRIVER_TEMPLATE;
        assert!(template.contains("(function () {"), "{template}");
        assert!(template.trim_end().ends_with("})();"), "{template}");
    }
}
