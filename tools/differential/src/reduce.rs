//! Line-based mismatch minimizer.
//!
//! Greedily deletes lines and balanced blocks while a caller-supplied
//! predicate still reports a mismatch. The predicate re-runs both sides
//! ([`main`] wires it to live execution); the algorithm itself is engine-free
//! and deterministic, so it unit-tests with fake predicates.
//!
//! This is the P3 minimizer behind the queue's `minimized_by` record. P4
//! replaces the hook with `tmin` once the fuzz fleet lands; the queue
//! schema already versions the producer per entry.

/// Per-entry minimization budget.
///
/// The fixpoint loop re-sweeps while any line drops, so slow-converging
/// entries (many load-bearing lines) cost O(sweeps x lines) probes on one
/// thread — observed 7k+ probes and counting on a 1317-line generated test,
/// stalling a whole 43k-case run behind a single entry. Whichever limit hits
/// first stops the loop with best-so-far; callers report exhausted entries
/// loudly (distinct producer mark + count + over-budget report), and the
/// repro stays usable for signature triage either way.
pub struct Budget {
    /// Deletion probes still allowed (the initial satisfiability probe is free).
    remaining: usize,
    /// Wall-clock stop time.
    deadline: std::time::Instant,
}

impl Budget {
    /// Budget of `max_candidates` deletion probes or `time`, whichever binds first.
    pub fn new(max_candidates: usize, time: std::time::Duration) -> Self {
        Self {
            remaining: max_candidates,
            deadline: std::time::Instant::now() + time,
        }
    }

    /// Unlimited budget, for unit tests with instant fake predicates.
    #[cfg(test)]
    pub fn unlimited() -> Self {
        Self::new(usize::MAX, std::time::Duration::from_secs(3600))
    }

    /// Consumes one probe; false once either limit is hit.
    fn consume(&mut self) -> bool {
        if self.remaining == 0 || std::time::Instant::now() >= self.deadline {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

/// Minimizes `program` to a mismatch-preserving subset of its lines.
///
/// Alternates single-line deletion with balanced-block deletion to a joint
/// fixpoint: lines are tried in order, multi-line brace/paren/bracket
/// ranges largest-first, each deletion kept only while `preserves_mismatch`
/// holds. The block pass exists because lone brace lines can never go alone
/// (removing one half breaks the parse), which stalls a pure line deleter
/// far above the budget on brace-heavy bodies. The input must already satisfy
/// the predicate; otherwise it is returned unchanged. Returns the minimized
/// program plus whether the [`Budget`] was exhausted (best-so-far then).
pub fn minimize(
    program: &str,
    preserves_mismatch: &dyn Fn(&str) -> bool,
    budget: &mut Budget,
) -> (String, bool) {
    if !preserves_mismatch(program) {
        return (program.to_owned(), false);
    }
    let mut lines: Vec<&str> = program.lines().collect();
    if delete_halving_chunks(&mut lines, preserves_mismatch, budget) {
        return (lines.join("\n"), true);
    }
    loop {
        let before = lines.len();
        if delete_single_lines(&mut lines, preserves_mismatch, budget) {
            return (lines.join("\n"), true);
        }
        if delete_balanced_ranges(&mut lines, preserves_mismatch, budget) {
            return (lines.join("\n"), true);
        }
        if lines.len() == before {
            break;
        }
    }
    (lines.join("\n"), false)
}

/// One ddmin-lite chunk-removal pre-pass ahead of the fixpoint loop.
///
/// Splits the body into `granularity` chunks and tries removing each; any
/// removal restarts the scan at halves, otherwise granularity doubles.
/// Removal-heavy shapes (generated assertion walls where most lines go)
/// collapse logarithmically here instead of line-by-line in the fixpoint
/// loop — ~25 probes for 1300 lines instead of ~2000. Fully load-bearing
/// bodies cost at most one extra line-sweep-equivalent of probes (2+4+8…).
/// Stops when chunks shrink below two lines (the line pass owns that
/// scale); bodies under 8 lines skip this entirely. Every probe is
/// predicate-gated and budget-counted, so this can only waste probes, never
/// corrupt the repro. Returns true when the [`Budget`] ran out.
fn delete_halving_chunks(
    lines: &mut Vec<&str>,
    preserves_mismatch: &dyn Fn(&str) -> bool,
    budget: &mut Budget,
) -> bool {
    if lines.len() < 8 {
        return false;
    }
    let mut granularity = 2;
    while granularity <= lines.len() {
        let chunk = lines.len().div_ceil(granularity);
        if chunk < 2 {
            break;
        }
        let mut removed = false;
        let mut start = 0;
        while start < lines.len() {
            let end = (start + chunk).min(lines.len());
            if !budget.consume() {
                return true;
            }
            let mut candidate = lines.clone();
            candidate.drain(start..end);
            if preserves_mismatch(&candidate.join("\n")) {
                *lines = candidate;
                removed = true;
                break;
            }
            start = end;
        }
        if removed {
            granularity = 2;
        } else {
            granularity *= 2;
        }
    }
    false
}

/// One sweep of single-line deletions, in order, while the predicate holds.
///
/// Returns true when the [`Budget`] ran out mid-sweep (partial progress kept).
fn delete_single_lines(
    lines: &mut Vec<&str>,
    preserves_mismatch: &dyn Fn(&str) -> bool,
    budget: &mut Budget,
) -> bool {
    let mut index = 0;
    while index < lines.len() {
        if !budget.consume() {
            return true;
        }
        let mut candidate = lines.clone();
        candidate.remove(index);
        if preserves_mismatch(&candidate.join("\n")) {
            *lines = candidate;
        } else {
            index += 1;
        }
    }
    false
}

/// Deletes multi-line balanced-delimiter ranges the line pass cannot.
///
/// Finds line ranges spanned by matched brace/paren/bracket pairs and
/// greedily deletes whole ranges, largest first, rescanning after each
/// success (indices shift). Every deletion is predicate-gated, so a
/// mis-scanned range only wastes probes, never corrupts the repro.
/// Returns true when the [`Budget`] ran out mid-pass (partial progress kept).
fn delete_balanced_ranges(
    lines: &mut Vec<&str>,
    preserves_mismatch: &dyn Fn(&str) -> bool,
    budget: &mut Budget,
) -> bool {
    loop {
        let mut ranges = balanced_ranges(&lines.join("\n"));
        ranges.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.0.cmp(&b.0)));
        let mut removed = false;
        for (start, end) in ranges {
            if !budget.consume() {
                return true;
            }
            let mut candidate = lines.clone();
            candidate.drain(start..=end);
            if preserves_mismatch(&candidate.join("\n")) {
                *lines = candidate;
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
    }
    false
}

/// Scanner mode for [`balanced_ranges`].
#[derive(PartialEq, Eq, Clone, Copy)]
enum Scan {
    Code,
    LineComment,
    BlockComment,
    /// Inside a quoted string; the byte is the quote (`'`, `"`, or `` ` ``).
    Str(u8),
}

/// Line ranges spanned by multi-line balanced delimiter pairs.
///
/// Scans `text` tracking `{...}`, `(...)`, `[...]` nesting while skipping
/// line/block comments, single/double-quoted strings, and template literals
/// (with `${}` interpolation re-entering code mode). Returns inclusive
/// `(start, end)` line indices for pairs spanning more than one line.
/// Best-effort: regex literals are not recognized, so a brace inside one can
/// skew nesting — the minimization predicate gates every deletion, so a
/// mis-scan only wastes probes.
fn balanced_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    // Open delimiter plus its line; `b'$'` marks a `${` interpolation whose
    // closer also exits back to template mode.
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut modes = vec![Scan::Code];
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut line = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match modes.last().copied().unwrap_or(Scan::Code) {
            Scan::LineComment => {
                if byte == b'\n' {
                    modes.pop();
                }
            }
            Scan::BlockComment => {
                if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    modes.pop();
                    index += 1;
                }
            }
            Scan::Str(quote) => {
                if byte == b'\\' {
                    // Skip the escaped byte with the backslash, still
                    // counting a line continuation as a newline.
                    if bytes.get(index + 1) == Some(&b'\n') {
                        line += 1;
                    }
                    index += 1;
                } else if byte == quote {
                    modes.pop();
                } else if quote == b'`' && byte == b'$' && bytes.get(index + 1) == Some(&b'{') {
                    modes.push(Scan::Code);
                    stack.push((b'$', line));
                    index += 1;
                }
            }
            Scan::Code => match byte {
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    modes.push(Scan::LineComment);
                    index += 1;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    modes.push(Scan::BlockComment);
                    index += 1;
                }
                b'\'' | b'"' | b'`' => modes.push(Scan::Str(byte)),
                b'{' | b'(' | b'[' => stack.push((byte, line)),
                closer @ (b'}' | b')' | b']') => {
                    let opener = match closer {
                        b'}' => b'{',
                        b')' => b'(',
                        _ => b'[',
                    };
                    // A `${` interpolation closes with `}` and returns to
                    // template mode; anything else must match its opener.
                    let matches = match stack.last() {
                        Some((b'$', _)) => closer == b'}',
                        Some((open, _)) => *open == opener,
                        None => false,
                    };
                    if matches {
                        let (open, open_line) = stack.pop().expect("checked above");
                        if open == b'$' {
                            modes.pop();
                        }
                        if line > open_line {
                            ranges.push((open_line, line));
                        }
                    }
                }
                _ => {}
            },
        }
        if byte == b'\n' {
            line += 1;
        }
        index += 1;
    }
    ranges
}

/// Counts lines the same way [`minimize`] splits them.
pub fn count_lines(program: &str) -> usize {
    program.lines().count()
}

/// Adopted line budget: a minimized test body must fit in 25 lines.
///
/// The budget governs the case's own lines (harness/prelude excluded, since
/// infrastructure is fixed); the pipeline drill asserts every queued entry
/// complies via its `minimized_lines` record.
pub const MINIMIZED_BODY_BUDGET: usize = 25;

/// Minimizes only the test body, keeping the leading `prefix_lines` fixed.
///
/// The predicate always receives the full candidate (prefix + body), so
/// preservation is evaluated on runnable programs; only the body shrinks.
/// Returns the minimized full program plus the minimized body line count
/// (the budgeted quantity) plus whether the [`Budget`] was exhausted.
/// Bodies at or under 10 lines skip the loop.
pub fn minimize_with_prefix(
    program: &str,
    prefix_lines: usize,
    preserves_mismatch: &dyn Fn(&str) -> bool,
    budget: &mut Budget,
) -> (String, usize, bool) {
    debug_assert!(
        prefix_lines <= count_lines(program),
        "prefix exceeds program length"
    );
    let (prefix, body) = program.split_at(nth_line_boundary(program, prefix_lines));
    if count_lines(body) <= 10 {
        return (program.to_owned(), count_lines(body), false);
    }
    let (minimized_body, exhausted) = minimize(
        body,
        &|candidate| preserves_mismatch(&format!("{prefix}{candidate}")),
        budget,
    );
    let body_lines = count_lines(&minimized_body);
    (format!("{prefix}{minimized_body}"), body_lines, exhausted)
}

/// Byte index where the `n`th line ends (start of line `n`, 0-based count).
///
/// Saturates at the string end when `n` exceeds the line count.
fn nth_line_boundary(program: &str, n: usize) -> usize {
    let mut start = 0;
    for _ in 0..n {
        match program[start..].find('\n') {
            Some(relative) => start += relative + 1,
            None => return program.len(),
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use super::{Budget, count_lines, minimize};

    #[test]
    fn keeps_only_essential_lines() {
        let program = "var a = 1;\nvar b = 2;\nprint(a + b);\n";
        let (minimized, exhausted) = minimize(
            program,
            &|candidate| candidate.contains("var b") && candidate.contains("print"),
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, "var b = 2;\nprint(a + b);");
    }

    #[test]
    fn already_minimal_is_a_fixpoint() {
        let program = "throw x;";
        let (minimized, exhausted) = minimize(
            program,
            &|candidate| candidate.contains("throw"),
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, program);
    }

    #[test]
    fn unsatisfying_input_returned_unchanged() {
        let program = "print(1);";
        let (minimized, exhausted) = minimize(program, &|_| false, &mut Budget::unlimited());
        assert!(!exhausted);
        assert_eq!(minimized, program);
    }

    #[test]
    fn halving_collapses_removal_heavy_bodies_logarithmically() {
        use std::cell::Cell;
        use std::fmt::Write as _;
        // 100 removable fillers + 1 essential line: a pure line sweep needs
        // ~100 probes; the halving pre-pass must do it in a fraction.
        let mut program = String::new();
        for i in 0..100 {
            writeln!(program, "filler-{i}").expect("write to String");
        }
        program.push_str("keep-me\n");
        let probes = Cell::new(0);
        let (minimized, exhausted) = minimize(
            &program,
            &|candidate| {
                probes.set(probes.get() + 1);
                candidate.contains("keep-me")
            },
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, "keep-me");
        assert!(probes.get() < 40, "probes: {}", probes.get());
    }

    #[test]
    fn budget_exhaustion_returns_best_so_far() {
        // Zero-candidate budget: the input probe runs, no deletion probe
        // does — deterministic exhaustion without depending on the clock.
        let program = "drop-me;\nkeep-me;\n";
        let (minimized, exhausted) = minimize(
            program,
            &|candidate| candidate.contains("keep-me"),
            &mut Budget::new(0, std::time::Duration::from_secs(60)),
        );
        assert!(exhausted);
        assert_eq!(minimized, "drop-me;\nkeep-me;");
    }

    #[test]
    fn count_lines_matches_split() {
        assert_eq!(count_lines("a\nb\nc"), 3);
        assert_eq!(count_lines(""), 0);
    }

    #[test]
    fn prefix_lines_stay_fixed_and_uncounted() {
        use super::minimize_with_prefix;
        use std::fmt::Write as _;
        let mut program = String::from("harness1\nharness2\nkeep-a\n");
        for i in 0..12 {
            writeln!(program, "filler-{i}").expect("write to String");
        }
        program.push_str("keep-b\n");
        // Predicate needs both keep-lines plus the harness to "mismatch".
        let (minimized, body_lines, exhausted) = minimize_with_prefix(
            &program,
            2,
            &|candidate| {
                candidate.contains("harness1")
                    && candidate.contains("keep-a")
                    && candidate.contains("keep-b")
            },
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, "harness1\nharness2\nkeep-a\nkeep-b");
        assert_eq!(body_lines, 2);
    }

    #[test]
    fn short_body_skips_the_loop() {
        use super::minimize_with_prefix;
        let program = "harness\nonly-body\n";
        let (minimized, body_lines, exhausted) =
            minimize_with_prefix(program, 1, &|_| true, &mut Budget::unlimited());
        assert!(!exhausted);
        assert_eq!(minimized, program);
        assert_eq!(body_lines, 1);
    }

    /// Fake parse check: balanced braces, standing in for "both sides
    /// still execute" in the live predicate.
    fn braces_balanced(candidate: &str) -> bool {
        let mut depth = 0;
        for byte in candidate.bytes() {
            match byte {
                b'{' => depth += 1,
                b'}' if depth > 0 => depth -= 1,
                b'}' => return false,
                _ => {}
            }
        }
        depth == 0
    }

    #[test]
    fn block_pass_removes_what_single_lines_cannot() {
        // No single line goes alone (each removal unbalances), but the
        // whole block goes — the stall the block pass exists to fix.
        let program = "function dropped() {\n  return 1;\n}\nkeep();\n";
        let (minimized, exhausted) = minimize(
            program,
            &|candidate| candidate.contains("keep") && braces_balanced(candidate),
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, "keep();");
    }

    #[test]
    fn block_pass_keeps_essential_blocks() {
        // Every line carries a unique required token: neither pass goes.
        let program = "function needed() {\n  return 1;\n}\nneeded();\n";
        let (minimized, exhausted) = minimize(
            program,
            &|candidate| {
                candidate.contains("function")
                    && candidate.contains("return")
                    && candidate.contains('}')
                    && candidate.contains("needed();")
                    && braces_balanced(candidate)
            },
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, program.trim_end());
    }

    #[test]
    fn block_pass_handles_parens_and_brackets() {
        let program = "check(value, [\n  1,\n  2,\n]);\nkeep();\n";
        let (minimized, exhausted) = minimize(
            program,
            &|candidate| candidate.contains("keep"),
            &mut Budget::unlimited(),
        );
        assert!(!exhausted);
        assert_eq!(minimized, "keep();");
    }

    #[test]
    fn scanner_ignores_braces_in_strings_and_comments() {
        use super::balanced_ranges;
        let text = "var s = \"{\";\n// {\n/*\n{\n*/\nkeep();\n";
        assert!(balanced_ranges(text).is_empty());
    }

    #[test]
    fn scanner_tracks_template_interpolation() {
        use super::balanced_ranges;
        // The `${...}` block spans lines 0-3; literal braces stay skipped.
        let text = "var t = `a${\n  f({\n  })\n}b`;\nkeep();\n";
        let mut ranges = balanced_ranges(text);
        ranges.sort_unstable();
        assert!(ranges.contains(&(0, 3)), "ranges: {ranges:?}");
    }
}
