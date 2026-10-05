//! Shared measurement battery (P5.4): deterministic valid-by-construction programs.
//!
//! Single generator shared by the P5.3 soundness gate (transform-dense
//! programs) and the P5.4 opcode matrix (coverage feed). Determinism
//! contract: `battery_program(seed)` is stable across runs and checkouts —
//! ratchets and failure seeds refer to programs by seed, so any generator
//! change must bump the battery version in the report format (see
//! [`crate::matrix::BATTERY_VERSION`]).

// ------------------------------------------------------------------
// Battery program generator: valid-by-construction templates with dense
// transform sites (`FuzzData` from noise yields empty/failing programs —
// libFuzzer evolves from structured seeds instead — so the battery rolls
// its own hermetic generator).
// ------------------------------------------------------------------

/// Deterministic 64-bit LCG for battery program generation.
struct BatteryRng(u64);

impl BatteryRng {
    fn new(seed: usize) -> Self {
        Self((seed as u64 ^ 0x9E3779B97F4A7C15).wrapping_mul(0xBF58476D1CE4E5B9))
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// Value in `0..n` (`n > 0`). Low-bit weakness is harmless:
    /// consumption order is fixed, so generation stays deterministic.
    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 11) as usize % n
    }

    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())]
    }
}

/// Valid-by-construction program with dense transform sites.
///
/// Terminates by construction (bounded loops only; no recursion — bodies
/// never name their own function — and no eval/getters/proxies) and
/// parses by construction: `??` appears only in fully-parenthesized
/// same-operator chains with leaf operands (bare `??` under `&&`/`||` is
/// a syntax error), assignments are statement-level only, member/call
/// bases are parenthesized (a numeric literal before `.prop` mis-lexes),
/// and there are no `let`/label duplicates, no bare commas, and no
/// regex/template/object literals (string/regex reprinting has known
/// escape hazards, so strings use a printer-safe alphabet).
pub fn battery_program(seed: usize) -> String {
    let mut rng = BatteryRng::new(seed);
    let bound = !rng.one_in(4);
    let mut src = String::new();
    if bound {
        for var in ["a", "b", "c", "d"] {
            src.push_str(&format!("let {var} = {};\n", battery_lit(&mut rng)));
        }
    }
    for _ in 0..rng.below(4) + 1 {
        src.push_str(&battery_stmt(&mut rng, bound, 0));
    }
    src
}

/// Printer-safe literal (pure literals only, so binop operands fold).
fn battery_lit(rng: &mut BatteryRng) -> String {
    match rng.below(10) {
        0 => rng.below(10).to_string(),
        1 => (rng.below(2001) as i64 - 1000).to_string(),
        2 => String::from("2147483647"),
        3 => String::from("0"),
        4 => String::from(rng.pick(&["0.1", "0.5", "2.5", "1e999", "0.0"])),
        5 => String::from(rng.pick(&["true", "false"])),
        6 => String::from("null"),
        7 => String::from("undefined"),
        8 => {
            let mut text = String::from("\"");
            for _ in 0..rng.below(7) {
                text.push_str(rng.pick(&["a", "b", "c", "x", "1", "2", " "]));
            }
            text.push('"');
            text
        }
        _ => String::from("3"),
    }
}

/// Bound (`a`–`d`) or unbound (`x`–`z`, reference-error paths) variable.
fn battery_var(rng: &mut BatteryRng, bound: bool) -> String {
    if bound && !rng.one_in(4) {
        String::from(rng.pick(&["a", "b", "c", "d"]))
    } else {
        String::from(rng.pick(&["x", "y", "z"]))
    }
}

fn battery_leaf(rng: &mut BatteryRng, bound: bool) -> String {
    if rng.one_in(2) {
        battery_lit(rng)
    } else {
        battery_var(rng, bound)
    }
}

/// Arithmetic operand: literals half the time (foldable pairs).
fn battery_operand(rng: &mut BatteryRng, bound: bool, depth: u8) -> String {
    match rng.below(4) {
        0 | 1 => battery_lit(rng),
        2 => battery_var(rng, bound),
        _ => battery_expr(rng, bound, depth + 1),
    }
}

/// Bitwise operand: small int literals half the time (foldable pairs).
fn battery_int_operand(rng: &mut BatteryRng, bound: bool, depth: u8) -> String {
    match rng.below(4) {
        0 | 1 => rng.below(64).to_string(),
        2 => battery_var(rng, bound),
        _ => battery_expr(rng, bound, depth + 1),
    }
}

/// Same-operator logical chain with random paren placement (regroup
/// fodder). `??` chains use leaf operands plus full parens (see
/// [`battery_program`]).
fn battery_chain(rng: &mut BatteryRng, bound: bool, depth: u8) -> String {
    let op = rng.pick(&["&&", "||", "??"]);
    if op == "??" {
        let (a, b, c) = (
            battery_leaf(rng, bound),
            battery_leaf(rng, bound),
            battery_leaf(rng, bound),
        );
        return format!("(({a} ?? {b}) ?? {c})");
    }
    let (a, b, c) = (
        battery_expr(rng, bound, depth + 1),
        battery_expr(rng, bound, depth + 1),
        battery_expr(rng, bound, depth + 1),
    );
    match rng.below(3) {
        0 => format!("(({a} {op} {b}) {op} {c})"),
        1 => format!("({a} {op} {b}) {op} {c}"),
        _ => format!("{a} {op} {b} {op} {c}"),
    }
}

fn battery_expr(rng: &mut BatteryRng, bound: bool, depth: u8) -> String {
    if depth > 3 {
        return battery_leaf(rng, bound);
    }
    let nested = |rng: &mut BatteryRng| battery_expr(rng, bound, depth + 1);
    match rng.below(15) {
        0 | 1 => {
            let op = rng.pick(&["+", "-", "*"]);
            format!(
                "{} {op} {}",
                battery_operand(rng, bound, depth),
                battery_operand(rng, bound, depth)
            )
        }
        2 => {
            let op = rng.pick(&["&", "|", "^"]);
            format!(
                "{} {op} {}",
                battery_int_operand(rng, bound, depth),
                battery_int_operand(rng, bound, depth)
            )
        }
        3 => {
            let op = rng.pick(&["<", ">", "===", "!=="]);
            format!("{} {op} {}", nested(rng), nested(rng))
        }
        // Logical pair (never `??`: see [`battery_chain`]).
        4 => {
            let op = rng.pick(&["&&", "||"]);
            format!("{} {op} {}", nested(rng), nested(rng))
        }
        5 => battery_chain(rng, bound, depth),
        // Unary over leaves (foldable) or vars.
        6 => format!("{} {}", rng.pick(&["-", "!"]), battery_leaf(rng, bound)),
        // Redundant parens.
        7 => format!("({})", nested(rng)),
        // Ternary (literal conditions fold).
        8 => {
            let cond = if rng.one_in(3) {
                battery_lit(rng)
            } else {
                nested(rng)
            };
            format!("{} ? {} : {}", cond, nested(rng), nested(rng))
        }
        // Calls (completing builtins or throwing unbound `f`).
        9 => {
            let callee = rng.pick(&["Math.max", "String", "Number", "f"]);
            format!("{callee}({}, {})", nested(rng), nested(rng))
        }
        // Member access / computed call (bases parenthesized: a numeric
        // literal before `.prop` mis-lexes).
        10 => {
            let base = nested(rng);
            match rng.below(3) {
                0 => format!("({base}).length"),
                1 => format!("({base}).b"),
                _ => format!("({base})(0)"),
            }
        }
        // Update (binds tight: safe as a subexpression).
        11 => {
            let var = battery_var(rng, bound);
            if rng.one_in(2) {
                format!("{var}++")
            } else {
                format!("--{var}")
            }
        }
        12 => format!("{}{}", rng.pick(&["typeof ", "void "]), nested(rng)),
        // Array literal (object literals skipped: a leading `{` parses
        // as a block in statement position).
        13 => format!("[{}, {}]", nested(rng), nested(rng)),
        _ => battery_leaf(rng, bound),
    }
}

fn battery_block(rng: &mut BatteryRng, bound: bool, depth: u8) -> String {
    let mut src = String::from("{ ");
    for _ in 0..rng.below(2) + 1 {
        src.push_str(&battery_stmt(rng, bound, depth + 1));
    }
    src.push('}');
    src
}

fn battery_stmt(rng: &mut BatteryRng, bound: bool, depth: u8) -> String {
    if depth > 2 {
        return format!("{};\n", battery_expr(rng, bound, 0));
    }
    match rng.below(12) {
        0 | 1 => format!("{};\n", battery_expr(rng, bound, 0)),
        // Assignment is statement-level only (bare `=` under an operator
        // mis-parses); unbound targets create sloppy globals.
        2 => format!(
            "{} = {};\n",
            battery_var(rng, bound),
            battery_expr(rng, bound, 0)
        ),
        // `if` with or without `else`; literal conditions fold.
        3 | 4 => {
            let cond = if rng.one_in(3) {
                battery_lit(rng)
            } else {
                battery_expr(rng, bound, 0)
            };
            let then = battery_block(rng, bound, depth);
            if rng.one_in(2) {
                format!(
                    "if ({cond}) {then}else {}\n",
                    battery_block(rng, bound, depth)
                )
            } else {
                format!("if ({cond}) {then}\n")
            }
        }
        5 => format!("{}\n", battery_block(rng, bound, depth)),
        // Bounded loop (the body is one expression: no break/continue).
        6 => format!(
            "for (let i = 0; i < {}; i++) {{ {};\n}}\n",
            rng.below(4),
            battery_expr(rng, bound, 0)
        ),
        7 => format!(
            "do {{ {};\n}} while (false);\n",
            battery_expr(rng, bound, 0)
        ),
        8 => {
            let extra = if rng.one_in(2) {
                format!("finally {{ {};\n}}", battery_expr(rng, bound, 0))
            } else {
                String::new()
            };
            format!(
                "try {{ {};\n}} catch (e) {{ {};\n}}{extra}\n",
                battery_expr(rng, bound, 0),
                battery_expr(rng, bound, 0)
            )
        }
        9 => format!(
            "switch ({}) {{ case {}: {};\nbreak;\ndefault: {};\n}}\n",
            battery_expr(rng, bound, 0),
            battery_lit(rng),
            battery_expr(rng, bound, 0),
            battery_expr(rng, bound, 0)
        ),
        10 => format!("throw {};\n", battery_expr(rng, bound, 0)),
        // Function declaration (duplicate names are legal) plus an
        // optional call; bodies cannot recurse (nothing names `gN`).
        _ => {
            let name = format!("g{}", rng.below(4));
            let mut src = format!(
                "function {name}() {{ {};\nreturn {};\n}}\n",
                battery_expr(rng, bound, 0),
                battery_expr(rng, bound, 0)
            );
            if rng.one_in(2) {
                src.push_str(&format!("{name}();\n"));
            }
            src
        }
    }
}
#[cfg(test)]
mod tests {
    use super::battery_program;
    use crate::metamorphic::metamorphic_variants;
    use crate::semantic::eval_outcome;

    #[test]
    fn same_seed_same_program() {
        for seed in [0, 1, 7, 1024, 9999] {
            assert_eq!(battery_program(seed), battery_program(seed));
        }
    }

    #[test]
    fn programs_parse_and_fire_transforms_densely() {
        // Parsing proof plus density proof: a program that fails to parse
        // yields no variants, so a dense battery parses almost everywhere.
        let mut firing = 0;
        for seed in 0..50 {
            if !metamorphic_variants(&battery_program(seed)).is_empty() {
                firing += 1;
            }
        }
        assert!(
            firing >= 40,
            "battery lost density: {firing}/50 programs fired a transform"
        );
    }

    #[test]
    fn programs_terminate_within_fuel() {
        // Termination proof: throwing (unbound vars, `throw` statements)
        // still terminates — only fuel exhaustion means non-termination.
        for seed in 0..20 {
            let outcome = eval_outcome(&battery_program(seed));
            assert_ne!(
                outcome.error_class.as_deref(),
                Some("FuelExhausted"),
                "battery program {seed} did not terminate: {outcome:?}"
            );
        }
    }
}
