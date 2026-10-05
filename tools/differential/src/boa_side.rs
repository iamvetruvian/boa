//! In-process Boa driver: what the `run-case` child executes.
//!
//! Each case runs in a fresh [`Context`] (tester precedent: contexts are
//! never reused across cases, so promises and globals cannot leak). The
//! composed program (see [`compose`]) is evaluated once; `print` and all
//! `console.*` methods append to a single ordered buffer, so interleaving
//! matches the oracle's single stdout stream.
//!
//! The child prints exactly one JSON [`BoaCaseOutcome`] line and always
//! exits 0 — panics included ([`catch_unwind`], tester precedent). Any other
//! child end is classified by the supervisor ([`supervise`]).

use std::{panic::catch_unwind, path::Path};

use boa_engine::{
    Context, JsArgs, JsError, JsResult, JsValue, Source,
    context::time::FixedClock,
    gc::{Gc, GcRefCell},
    js_str,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    property::Attribute,
};
use boa_outcomes::TestOutcomeResult;
use boa_runtime::console::{Console, ConsoleState, Logger};
use serde::{Deserialize, Serialize};

use crate::capture::{RawObservation, parse_observation};

/// Outcome line printed by a `run-case` child on stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoaCaseOutcome {
    /// Side outcome (`Passed` always carries an observation; `Failed` may).
    #[serde(rename = "o")]
    pub outcome: TestOutcomeResult,
    /// Human summary (empty on `Passed`).
    #[serde(rename = "t")]
    pub text: String,
    /// Parsed observation when the driver emitted valid markers.
    #[serde(rename = "obs")]
    pub observation: Option<RawObservation>,
}

/// Shared ordered capture buffer for `print` + `console.*`.
type CaptureBuffer = Gc<GcRefCell<Vec<String>>>;

/// `console.*` logger appending to the shared [`CaptureBuffer`].
#[derive(Clone, Debug, Default, boa_engine::Trace, boa_engine::Finalize)]
struct CaptureLogger {
    /// Buffer rooted by the registered console object for the whole eval.
    buffer: CaptureBuffer,
}

impl Logger for CaptureLogger {
    fn log(&self, msg: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
        self.buffer.borrow_mut().push(msg);
        Ok(())
    }

    fn info(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }

    fn warn(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }

    fn error(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }
}

/// Runs one composed-program file and returns its outcome line payload.
pub fn run_case_file(program_path: &Path) -> BoaCaseOutcome {
    let text = match std::fs::read_to_string(program_path) {
        Ok(text) => text,
        Err(err) => {
            return BoaCaseOutcome {
                outcome: TestOutcomeResult::HarnessError,
                text: format!("could not read composed program: {err}"),
                observation: None,
            };
        }
    };
    catch_unwind(move || run_case_source(&text)).unwrap_or_else(|payload| BoaCaseOutcome {
        outcome: TestOutcomeResult::Panic,
        text: panic_message(payload.as_ref()),
        observation: None,
    })
}

/// Evaluates composed source in a fresh deterministic context.
fn run_case_source(source: &str) -> BoaCaseOutcome {
    let buffer: CaptureBuffer = Gc::new(GcRefCell::new(Vec::new()));
    // `[[CanBlock]]` is true on both sides: tester precedent (`exec/mod.rs`
    // sets false only under `CanBlockIsFalse`, which the corpus excludes
    // as `canblock-false`), and the pinned oracle's main thread blocks
    // (verified: finite-timeout `Atomics.wait` completes there).
    let mut context = Context::builder()
        .clock(std::rc::Rc::new(FixedClock::from_millis(0)))
        .can_block(true)
        .build()
        .expect("differential context uses the default global object");
    register_print_fn(&mut context, buffer.clone());
    Console::register_with_logger(
        CaptureLogger {
            buffer: buffer.clone(),
        },
        &mut context,
    )
    .expect("console registration cannot fail on a fresh context");

    let eval_error = match context.eval(Source::from_bytes(source)) {
        Ok(_) => None,
        Err(err) => Some(driver_throw_text(&err, &mut context)),
    };
    // The driver is synchronous, but draining jobs keeps the context honest
    // (a leaked rejected job is signal, not silence).
    let jobs_error = context.run_jobs().err().map(|err| err.to_string());

    let lines = buffer.borrow().clone();
    let observation = parse_observation(&lines, "").ok();
    let side_error = eval_error.or(jobs_error);
    match (side_error, observation) {
        (None, Some(observation)) => BoaCaseOutcome {
            outcome: TestOutcomeResult::Passed,
            text: String::new(),
            observation: Some(observation),
        },
        (None, None) => BoaCaseOutcome {
            outcome: TestOutcomeResult::Failed,
            text: "driver completed but emitted no valid markers".to_owned(),
            observation: None,
        },
        (Some(error), observation) => BoaCaseOutcome {
            outcome: TestOutcomeResult::Failed,
            text: format!("driver-level failure: {error}"),
            observation,
        },
    }
}

/// Registers a `print` function appending to the shared buffer.
fn register_print_fn(context: &mut Context, buffer: CaptureBuffer) {
    let js_function = FunctionObjectBuilder::new(
        context.realm(),
        // SAFETY: the buffer is rooted by the registered console logger for
        // the whole eval; this copy never outlives the context, and `String`
        // captures need no tracing.
        unsafe {
            NativeFunction::from_closure(move |_, args, context| {
                let message = args
                    .get_or_undefined(0)
                    .to_string(context)?
                    .to_std_string_escaped();
                buffer.borrow_mut().push(message);
                Ok(JsValue::undefined())
            })
        },
    )
    .name("print")
    .length(1)
    .build();

    context
        .register_global_property(
            js_str!("print"),
            js_function,
            Attribute::WRITABLE | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .expect("print registration cannot fail on a fresh context");
}

/// Renders a driver-level (outside the program `try`) throw as kind + message.
fn driver_throw_text(error: &JsError, context: &mut Context) -> String {
    let text = error.try_native(context).map_or_else(
        |_| format!("opaque throw {error}"),
        |native| format!("{}: {}", native.kind(), native.message()),
    );
    // `Display for JsError` appends a backtrace; keep the head for the log.
    text.chars().take(1000).collect()
}

/// Best-effort panic payload message.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "boa child panicked with an unprintable payload".to_owned()
    }
}
