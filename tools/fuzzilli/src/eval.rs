//! One script evaluation: fresh context, fuel cap, Fuzzilli globals.
//!
//! Outcome mapping (crash fidelity is the whole point of this adapter):
//! - value returned → completed (REPRL status 0).
//! - JS exception incl. fuel exhaustion → failed (REPRL status 1).
//! - `EngineError::Panic` surfaced as `JsError`, or a Rust panic escaping
//!   `eval` → `process::abort()` (a Rust unwind would exit 101, which
//!   Fuzzilli reads as mere failure and the crash would be LOST).

use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue, Source, native_function::NativeFunction,
    object::FunctionObjectBuilder, property::Attribute, string::JsString,
};

/// What one script evaluation produced (crashes never return).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Completed,
    Threw(String),
}

/// Builds a fresh instrumented context with the Fuzzilli globals.
fn build_context(fuel: usize) -> Context {
    let mut context = Context::builder()
        .instructions_remaining(fuel)
        .build()
        .expect("reprl context must build");
    for (name, func) in [
        ("fuzzilli", NativeFunction::from_fn_ptr(fuzzilli_fn)),
        ("print", NativeFunction::from_fn_ptr(print_fn)),
    ] {
        let js_function = FunctionObjectBuilder::new(context.realm(), func)
            .name(name)
            .length(2)
            .build();
        context
            .register_global_property(JsString::from(name), js_function, Attribute::all())
            .expect("register reprl global");
    }
    context
}

/// Evaluates `source` once under `fuel`. Never returns on engine panic.
pub(crate) fn eval_once(source: &str, fuel: usize) -> Outcome {
    let result = std::panic::catch_unwind(|| {
        let mut context = build_context(fuel);
        context.eval(Source::from_bytes(source))
    });
    let Ok(evaluation) = result else {
        eprintln!("reprl: Rust panic escaped eval; aborting for crash fidelity");
        std::process::abort();
    };
    match evaluation {
        Ok(_) => Outcome::Completed,
        Err(err) => {
            if let Some(engine) = err.as_engine() {
                let class = format!("{engine:?}");
                // Fuel exhaustion is an ordinary failure (bounded programs
                // time out this way instead of hanging the runner).
                if !class.contains("NoInstructionsRemain") && class.contains("Panic") {
                    eprintln!("reprl: engine panicked: {err}; aborting");
                    std::process::abort();
                }
                return Outcome::Threw(class);
            }
            if let Some(native) = err.as_native() {
                return Outcome::Threw(native.kind().to_string());
            }
            Outcome::Threw(String::from("thrown-value"))
        }
    }
}

/// REPRL child data-out fd: the fuzzout channel (a memfd the parent maps;
/// it derives the length from the shared file offset, so plain writes
/// append one message after another; the parent seeks to 0 before each
/// execution). Mirrors upstream `Targets/QJS/...Fuzzilli-instrumentation`.
const REPRL_DWFD: i32 = 103;

/// Writes one fuzzout message (`{msg}\n`, upstream `%s\n` shape). Falls
/// back to stdout when fd 103 is unavailable (standalone `--script` runs).
fn fuzzout_write(message: &str) {
    let mut buf = Vec::with_capacity(message.len() + 1);
    buf.extend_from_slice(message.as_bytes());
    buf.push(b'\n');
    let mut rest = buf.as_slice();
    while !rest.is_empty() {
        // SAFETY: libc write over a live slice; `written` is checked
        // positive before advancing, so the `as usize` cannot lose a sign.
        let written =
            unsafe { libc::write(REPRL_DWFD, rest.as_ptr().cast::<libc::c_void>(), rest.len()) };
        if written <= 0 {
            println!("{message}");
            return;
        }
        #[allow(clippy::cast_sign_loss)]
        let advanced = written as usize;
        rest = &rest[advanced..];
    }
}

/// The `fuzzilli` integration builtin (startup self-tests + crash checks).
///
/// - `fuzzilli('FUZZILLI_PRINT', msg)` emits `{msg}\n` on the fuzzout
///   channel (fd 103) and returns `undefined`.
/// - `fuzzilli('FUZZILLI_CRASH', 0)` aborts the process.
/// - `fuzzilli('FUZZILLI_CRASH', 1)` faults (null write, SIGSEGV).
///
/// Anything else throws a `TypeError`. The generator never emits calls to
/// this (it is intentionally absent from the profile's builtins); only the
/// startup tests use it, so unknown inputs fail closed as throws.
fn fuzzilli_fn(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let command = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    match command.as_str() {
        "FUZZILLI_PRINT" => {
            let message = args
                .get_or_undefined(1)
                .to_string(context)?
                .to_std_string_escaped();
            fuzzout_write(&message);
            Ok(JsValue::undefined())
        }
        "FUZZILLI_CRASH" => {
            // Crash codes are 0/1/2 by contract; any other value (including
            // saturation artifacts of huge floats) takes the abort arm.
            #[allow(clippy::cast_possible_truncation)]
            let code = args.get_or_undefined(1).as_number().unwrap_or(0.0) as i32;
            if code == 1 {
                // Deliberate null write: the crash-detection self-test.
                // SAFETY: intentional fault; the parent expects the signal.
                unsafe {
                    std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 0);
                }
                Ok(JsValue::undefined())
            } else {
                eprintln!("reprl: FUZZILLI_CRASH({code}); aborting");
                std::process::abort();
            }
        }
        _ => Err(JsNativeError::typ()
            .with_message(format!("unknown fuzzilli command: {command}"))
            .into()),
    }
}

/// `print(...args)`: space-joined `to_string` rendering to stdout.
fn print_fn(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        parts.push(arg.to_string(context)?.to_std_string_escaped());
    }
    println!("{}", parts.join(" "));
    Ok(JsValue::undefined())
}
