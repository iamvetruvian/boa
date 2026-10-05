//! Execution module for the test runner.

use boa_runtime::test262 as js262;

use crate::{
    Harness, Outcome, Phase, SpecEdition, Statistics, SuiteResult, Test, TestFlags,
    TestOutcomeResult, TestResult, TestSuite, VersionedStats, crash::CrashRecord, read::ErrorType,
};
use boa_engine::{
    Context, JsArgs, JsError, JsNativeErrorKind, JsResult, JsValue, Source,
    builtins::promise::PromiseState,
    gc::StressGuard,
    js_str,
    module::{Module, SimpleModuleLoader},
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    optimizer::OptimizerOptions,
    parser::source::ReadChar,
    property::Attribute,
    script::Script,
};
use colored::Colorize;
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    eprintln,
    io::Read,
    num::NonZeroU64,
    path::Path,
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

use js262::WorkerHandles;

impl TestSuite {
    /// Runs the test suite.
    ///
    /// Returns the aggregated [`SuiteResult`] plus one [`CrashRecord`] per
    /// abnormal test outcome in this suite's subtree.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &self,
        harness: &Harness,
        verbose: u8,
        parallel: bool,
        max_edition: SpecEdition,
        optimizer_options: OptimizerOptions,
        console: bool,
        test262_root: &Path,
        timeout: Option<u64>,
        gc_stress: Option<NonZeroU64>,
    ) -> (SuiteResult, Vec<CrashRecord>) {
        if verbose != 0 {
            println!("Suite {}:", self.path.display());
        }

        let runs: Vec<_> = if parallel {
            self.suites
                .par_iter()
                .map(|suite| {
                    suite.run(
                        harness,
                        verbose,
                        parallel,
                        max_edition,
                        optimizer_options,
                        console,
                        test262_root,
                        timeout,
                        gc_stress,
                    )
                })
                .collect()
        } else {
            self.suites
                .iter()
                .map(|suite| {
                    suite.run(
                        harness,
                        verbose,
                        parallel,
                        max_edition,
                        optimizer_options,
                        console,
                        test262_root,
                        timeout,
                        gc_stress,
                    )
                })
                .collect()
        };

        let mut suites = Vec::with_capacity(runs.len());
        let mut crashes = Vec::new();
        for (suite, mut suite_crashes) in runs {
            suites.push(suite);
            crashes.append(&mut suite_crashes);
        }

        let tests: Vec<_> = if parallel {
            self.tests
                .par_iter()
                .filter(|test| test.edition <= max_edition)
                .map(|test| {
                    test.run(
                        harness,
                        verbose,
                        optimizer_options,
                        console,
                        test262_root,
                        timeout,
                        gc_stress,
                    )
                })
                .collect()
        } else {
            self.tests
                .iter()
                .filter(|test| test.edition <= max_edition)
                .map(|test| {
                    test.run(
                        harness,
                        verbose,
                        optimizer_options,
                        console,
                        test262_root,
                        timeout,
                        gc_stress,
                    )
                })
                .collect()
        };

        let mut results = Vec::with_capacity(tests.len());
        for (result, crash) in tests {
            results.push(result);
            crashes.extend(crash);
        }
        let tests = results;

        let mut features = FxHashSet::default();
        for test_iter in &*self.tests {
            features.extend(test_iter.features.iter().map(ToString::to_string));
        }

        if verbose != 0 {
            println!();
        }

        // Count passed tests and es specs
        let mut versioned_stats = VersionedStats::default();
        let mut es_next = Statistics::default();

        for test in &tests {
            match test.result {
                TestOutcomeResult::Passed => {
                    versioned_stats.apply(test.edition, |stats| {
                        stats.passed += 1;
                    });
                    es_next.passed += 1;
                }
                TestOutcomeResult::Ignored => {
                    versioned_stats.apply(test.edition, |stats| {
                        stats.ignored += 1;
                    });
                    es_next.ignored += 1;
                }
                TestOutcomeResult::Panic => {
                    versioned_stats.apply(test.edition, |stats| {
                        stats.panic += 1;
                    });
                    es_next.panic += 1;
                }
                TestOutcomeResult::Timeout => {
                    versioned_stats.apply(test.edition, |stats| {
                        stats.timeout += 1;
                    });
                    es_next.timeout += 1;
                }
                TestOutcomeResult::Crash => {
                    versioned_stats.apply(test.edition, |stats| {
                        stats.crash += 1;
                    });
                    es_next.crash += 1;
                }
                TestOutcomeResult::HarnessError => {
                    versioned_stats.apply(test.edition, |stats| {
                        stats.harness_error += 1;
                    });
                    es_next.harness_error += 1;
                }
                TestOutcomeResult::Failed => {}
            }
            versioned_stats.apply(test.edition, |stats| {
                stats.total += 1;
            });
            es_next.total += 1;
        }

        // Count total tests
        for suite in &suites {
            versioned_stats += suite.versioned_stats;
            es_next += suite.stats;
            features.extend(suite.features.iter().cloned());
        }

        if verbose != 0 {
            let mut abnormal = Vec::new();
            if es_next.panic != 0 {
                abnormal.push(format!("{} panics", es_next.panic));
            }
            if es_next.timeout != 0 {
                abnormal.push(format!("{} timeouts", es_next.timeout));
            }
            if es_next.crash != 0 {
                abnormal.push(format!("{} crashes", es_next.crash));
            }
            if es_next.harness_error != 0 {
                abnormal.push(format!("{} harness errors", es_next.harness_error));
            }
            let abnormal = if abnormal.is_empty() {
                String::new()
            } else {
                format!("({})", abnormal.join(", ").red())
            };
            println!(
                "Suite {} results: total: {}, passed: {}, ignored: {}, failed: {} {}, conformance: {:.2}%",
                self.path.display(),
                es_next.total,
                es_next.passed.to_string().green(),
                es_next.ignored.to_string().yellow(),
                es_next.failed().to_string().red(),
                abnormal,
                (es_next.passed as f64 / es_next.total as f64) * 100.0
            );
        }
        let result = SuiteResult {
            name: self.name.clone(),
            stats: es_next,
            versioned_stats,
            suites,
            tests,
            features,
        };
        (result, crashes)
    }
}

impl Test {
    /// Runs the test.
    ///
    /// Returns the [`TestResult`] plus a [`CrashRecord`] when the outcome is
    /// abnormal (panic, timeout, crash, or harness error).
    ///
    /// When `timeout` is set, each strictness attempt runs in a child
    /// process that is killed after the budget expires; otherwise the test
    /// runs in-process.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &self,
        harness: &Harness,
        verbose: u8,
        optimizer_options: OptimizerOptions,
        console: bool,
        test262_root: &Path,
        timeout: Option<u64>,
        gc_stress: Option<NonZeroU64>,
    ) -> (TestResult, Option<CrashRecord>) {
        if let Some(timeout_secs) = timeout {
            return self.run_isolated(
                verbose,
                optimizer_options,
                console,
                test262_root,
                timeout_secs,
                gc_stress,
            );
        }

        if self.flags.contains(TestFlags::MODULE) || self.flags.contains(TestFlags::RAW) {
            return self.run_once(
                harness,
                false,
                verbose,
                optimizer_options,
                console,
                test262_root,
                false,
                gc_stress,
            );
        }

        if self
            .flags
            .contains(TestFlags::STRICT | TestFlags::NO_STRICT)
        {
            let (r, crash) = self.run_once(
                harness,
                false,
                verbose,
                optimizer_options,
                console,
                test262_root,
                false,
                gc_stress,
            );
            if r.result != TestOutcomeResult::Passed {
                return (r, crash);
            }
            self.run_once(
                harness,
                true,
                verbose,
                optimizer_options,
                console,
                test262_root,
                false,
                gc_stress,
            )
        } else {
            self.run_once(
                harness,
                self.flags.contains(TestFlags::STRICT),
                verbose,
                optimizer_options,
                console,
                test262_root,
                false,
                gc_stress,
            )
        }
    }

    /// Runs one strictness attempt in an isolated child process.
    ///
    /// This mirrors [`Test::run`]'s strictness branching; the per-attempt
    /// mechanics live in [`run_child`].
    fn run_isolated(
        &self,
        verbose: u8,
        optimizer_options: OptimizerOptions,
        console: bool,
        test262_root: &Path,
        timeout_secs: u64,
        gc_stress: Option<NonZeroU64>,
    ) -> (TestResult, Option<CrashRecord>) {
        // Mirror `run_once`'s pre-checks without spawning.
        if Source::from_filepath(&self.path).is_err() {
            return (
                self.create_result(
                    TestOutcomeResult::Failed,
                    "Could not read test file",
                    false,
                    verbose,
                    false,
                ),
                None,
            );
        }
        if self.ignored {
            return (
                self.create_result(TestOutcomeResult::Ignored, "", false, verbose, false),
                None,
            );
        }

        if self.flags.contains(TestFlags::MODULE) || self.flags.contains(TestFlags::RAW) {
            return self.run_single_child(
                false,
                verbose,
                optimizer_options,
                console,
                test262_root,
                timeout_secs,
                gc_stress,
            );
        }

        if self
            .flags
            .contains(TestFlags::STRICT | TestFlags::NO_STRICT)
        {
            let (r, crash) = self.run_single_child(
                false,
                verbose,
                optimizer_options,
                console,
                test262_root,
                timeout_secs,
                gc_stress,
            );
            if r.result != TestOutcomeResult::Passed {
                return (r, crash);
            }
            return self.run_single_child(
                true,
                verbose,
                optimizer_options,
                console,
                test262_root,
                timeout_secs,
                gc_stress,
            );
        }

        self.run_single_child(
            self.flags.contains(TestFlags::STRICT),
            verbose,
            optimizer_options,
            console,
            test262_root,
            timeout_secs,
            gc_stress,
        )
    }

    /// Spawns one isolated attempt and builds its [`TestResult`].
    #[allow(clippy::too_many_arguments)]
    fn run_single_child(
        &self,
        strict: bool,
        verbose: u8,
        optimizer_options: OptimizerOptions,
        console: bool,
        test262_root: &Path,
        timeout_secs: u64,
        gc_stress: Option<NonZeroU64>,
    ) -> (TestResult, Option<CrashRecord>) {
        let (outcome, text, crash) = run_child(
            &self.path,
            test262_root,
            strict,
            optimizer_options == OptimizerOptions::OPTIMIZE_ALL,
            console,
            timeout_secs,
            &self.test_id(test262_root),
            gc_stress,
        );
        (
            self.create_result(outcome, text, strict, verbose, false),
            crash,
        )
    }

    /// Stable test id: test262-relative path without the `.js` extension.
    ///
    /// This id is what joins per-test outcomes across runs, baselines, the
    /// regression database, and triage stores.
    pub(crate) fn test_id(&self, test262_root: &Path) -> String {
        let relative = self
            .path
            .strip_prefix(test262_root)
            .map_or_else(|_| self.path.to_path_buf(), Path::to_path_buf);
        let mut id = relative.to_string_lossy().into_owned();
        if let Some(stripped) = id.strip_suffix(".js") {
            id = stripped.to_string();
        }
        id
    }

    /// Creates the test result from the outcome and message.
    ///
    /// When `quiet`, nothing is printed: isolated children must keep stdout
    /// clean for the outcome JSON line.
    fn create_result<S: Into<Box<str>>>(
        &self,
        outcome: TestOutcomeResult,
        text: S,
        strict: bool,
        verbosity: u8,
        quiet: bool,
    ) -> TestResult {
        let result_text = text.into();

        if quiet {
            // Isolated child: stdout carries only the outcome JSON line.
        } else if verbosity > 1 {
            println!(
                "`{}`{}: {}",
                self.path.display(),
                if strict { " (strict)" } else { "" },
                match outcome {
                    TestOutcomeResult::Passed => "Passed".green(),
                    TestOutcomeResult::Ignored => "Ignored".yellow(),
                    TestOutcomeResult::Failed => "Failed".red(),
                    TestOutcomeResult::Panic => "⚠ Panic ⚠".red(),
                    TestOutcomeResult::Timeout => "⚠ Timeout ⚠".red(),
                    TestOutcomeResult::Crash => "⚠ Crash ⚠".red(),
                    TestOutcomeResult::HarnessError => "⚠ Harness error ⚠".red(),
                }
            );
        } else {
            let symbol = match outcome {
                TestOutcomeResult::Passed => ".".green(),
                TestOutcomeResult::Ignored => "-".yellow(),
                TestOutcomeResult::Failed => "F".red(),
                TestOutcomeResult::Panic => "P".red(),
                TestOutcomeResult::Timeout => "T".red(),
                TestOutcomeResult::Crash => "C".red(),
                TestOutcomeResult::HarnessError => "H".red(),
            };

            print!("{symbol}");
        }

        if verbosity > 2 {
            println!(
                "`{}`{}: result text\n{result_text}\n",
                self.path.display(),
                if strict { " (strict)" } else { "" },
            );
        }

        TestResult {
            name: self.name.clone(),
            edition: self.edition,
            result_text,
            result: outcome,
        }
    }

    /// Runs the test once, in strict or non-strict mode.
    ///
    /// When `quiet`, nothing is printed to stdout: isolated children must
    /// keep stdout clean for the outcome JSON line.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_once(
        &self,
        harness: &Harness,
        strict: bool,
        verbosity: u8,
        optimizer_options: OptimizerOptions,
        console: bool,
        test262_root: &Path,
        quiet: bool,
        gc_stress: Option<NonZeroU64>,
    ) -> (TestResult, Option<CrashRecord>) {
        let test_id = self.test_id(test262_root);
        let Ok(source) = Source::from_filepath(&self.path) else {
            return (
                self.create_result(
                    TestOutcomeResult::Failed,
                    "Could not read test file",
                    strict,
                    verbosity,
                    quiet,
                ),
                None,
            );
        };

        if self.ignored {
            return (
                self.create_result(TestOutcomeResult::Ignored, "", strict, verbosity, quiet),
                None,
            );
        }

        // GC stress (P4.4): each test sets the thread-local cadence itself,
        // so rayon workers and serial runs are covered alike. The guard
        // restores the previous setting on return or panic (caught below),
        // so stress can never leak onto a later test sharing this thread.
        let _stress = gc_stress.map(StressGuard::stress);

        if verbosity > 1 && !quiet {
            println!(
                "`{}`{}: starting",
                self.path.display(),
                if strict { " (strict mode)" } else { "" }
            );
        }

        let result = std::panic::catch_unwind(|| match self.expected_outcome {
            Outcome::Positive => {
                let (ref mut context, async_result, mut handles) =
                    match self.create_context(harness, optimizer_options, console) {
                        Ok(r) => r,
                        Err(e) => return (false, e),
                    };

                // TODO: timeout
                let value = if self.is_module() {
                    let module = match parse_module_and_register(source, &self.path, context) {
                        Ok(module) => module,
                        Err(err) => return (false, format!("Uncaught {err}")),
                    };

                    let promise = module.load_link_evaluate(context);

                    if let Err(err) = context.run_jobs() {
                        return (false, format!("Uncaught {err}"));
                    }

                    match promise.state() {
                        PromiseState::Pending => {
                            return (false, "module should have been executed".to_string());
                        }
                        PromiseState::Fulfilled(v) => v,
                        PromiseState::Rejected(err) => {
                            let output = JsError::from_opaque(err.clone())
                                .try_native(context)
                                .map_or_else(
                                    |_| format!("Uncaught {}", err.display()),
                                    |err| {
                                        format!(
                                            "Uncaught {err}{}",
                                            err.cause().map_or_else(String::new, |cause| format!(
                                                "\n  caused by {cause}"
                                            ))
                                        )
                                    },
                                );

                            return (false, output);
                        }
                    }
                } else {
                    context.strict(strict);
                    match context.eval(source) {
                        Ok(v) => v,
                        Err(err) => return (false, format!("Uncaught {err}")),
                    }
                };

                if let Err(err) = context.run_jobs() {
                    return (false, format!("Uncaught {err}"));
                }

                match *async_result.inner.borrow() {
                    UninitResult::Err(ref e) => return (false, format!("Uncaught {e}")),
                    UninitResult::Uninit if self.flags.contains(TestFlags::ASYNC) => {
                        return (
                            false,
                            "async test did not print \"Test262:AsyncTestComplete\"".to_string(),
                        );
                    }
                    _ => {}
                }

                for result in handles.join_all() {
                    match result {
                        js262::WorkerResult::Err(msg) => return (false, msg),
                        js262::WorkerResult::Panic(msg) => panic!("Worker thread panicked: {msg}"),
                        js262::WorkerResult::Ok => {}
                    }
                }

                (true, value.display().to_string())
            }
            Outcome::Negative {
                phase: Phase::Parse,
                error_type,
            } => {
                assert_eq!(
                    error_type,
                    ErrorType::SyntaxError,
                    "non-SyntaxError parsing/early error found in {}",
                    self.path.display()
                );

                let context = &mut Context::default();

                if self.is_module() {
                    match Module::parse(source, None, context) {
                        Ok(_) => (false, "ModuleItemList parsing should fail".to_owned()),
                        Err(e) => (true, format!("Uncaught {e}")),
                    }
                } else {
                    context.strict(strict);
                    match Script::parse(source, None, context) {
                        Ok(_) => (false, "StatementList parsing should fail".to_owned()),
                        Err(e) => (true, format!("Uncaught {e}")),
                    }
                }
            }
            Outcome::Negative {
                phase: Phase::Resolution,
                error_type,
            } => {
                let context = &mut match self.create_context(harness, optimizer_options, console) {
                    Ok(r) => r,
                    Err(e) => return (false, e),
                }
                .0;

                let module = match parse_module_and_register(source, &self.path, context) {
                    Ok(module) => module,
                    Err(err) => return (false, format!("Uncaught {err}")),
                };

                let promise = module.load(context);

                if let Err(err) = context.run_jobs() {
                    return (false, format!("Uncaught {err}"));
                }

                match promise.state() {
                    PromiseState::Pending => {
                        return (false, "module didn't try to load".to_string());
                    }
                    PromiseState::Fulfilled(_) => {
                        // Try to link to see if the resolution error shows there.
                    }
                    PromiseState::Rejected(err) => {
                        let err = JsError::from_opaque(err);
                        return (
                            is_error_type(&err, error_type, context),
                            format!("Uncaught {err}"),
                        );
                    }
                }

                if let Err(err) = module.link(context) {
                    (
                        is_error_type(&err, error_type, context),
                        format!("Uncaught {err}"),
                    )
                } else {
                    (false, "module resolution didn't fail".to_string())
                }
            }
            Outcome::Negative {
                phase: Phase::Runtime,
                error_type,
            } => {
                let (ref mut context, _async_result, mut handles) =
                    match self.create_context(harness, optimizer_options, console) {
                        Ok(r) => r,
                        Err(e) => return (false, e),
                    };

                let error = if self.is_module() {
                    let module = match parse_module_and_register(source, &self.path, context) {
                        Ok(module) => module,
                        Err(err) => return (false, format!("Uncaught {err}")),
                    };

                    let promise = module.load(context);

                    if let Err(err) = context.run_jobs() {
                        return (false, format!("Uncaught {err}"));
                    }

                    match promise.state() {
                        PromiseState::Pending => {
                            return (false, "module didn't try to load".to_string());
                        }
                        PromiseState::Fulfilled(_) => {}
                        PromiseState::Rejected(err) => {
                            return (false, format!("Uncaught {}", err.display()));
                        }
                    }

                    if let Err(err) = module.link(context) {
                        return (false, format!("Uncaught {err}"));
                    }

                    let promise = match module.evaluate(context) {
                        Ok(p) => p,
                        Err(err) => return (false, format!("Uncaught {err}")),
                    };

                    if let Err(err) = context.run_jobs() {
                        return (false, format!("Uncaught {err}"));
                    }

                    match promise.state() {
                        PromiseState::Pending => {
                            return (false, "module didn't try to evaluate".to_string());
                        }
                        PromiseState::Fulfilled(val) => return (false, val.display().to_string()),
                        PromiseState::Rejected(err) => JsError::from_opaque(err),
                    }
                } else {
                    context.strict(strict);
                    let script = match Script::parse(source, None, context) {
                        Ok(code) => code,
                        Err(e) => return (false, format!("Uncaught {e}")),
                    };

                    match script.evaluate(context) {
                        Ok(_) => return (false, "Script execution should fail".to_owned()),
                        Err(e) => e,
                    }
                };

                for result in handles.join_all() {
                    match result {
                        js262::WorkerResult::Err(msg) => return (false, msg),
                        js262::WorkerResult::Panic(msg) => panic!("Worker thread panicked: {msg}"),
                        js262::WorkerResult::Ok => {}
                    }
                }

                (
                    is_error_type(&error, error_type, context),
                    format!("Uncaught {error}"),
                )
            }
        });

        let (result, result_text, crash) = result.map_or_else(
            |payload| {
                let record = CrashRecord::from_panic(&test_id, payload.as_ref());
                eprintln!(
                    "last panic was on test \"{}\" [{}]",
                    self.path.display(),
                    record.signature
                );
                (
                    TestOutcomeResult::Panic,
                    record.message.clone(),
                    Some(record),
                )
            },
            |(res, text)| {
                if res {
                    (TestOutcomeResult::Passed, text, None)
                } else {
                    (TestOutcomeResult::Failed, text, None)
                }
            },
        );

        (
            self.create_result(result, result_text, strict, verbosity, quiet),
            crash,
        )
    }

    /// Creates the context to run the test.
    fn create_context(
        &self,
        harness: &Harness,
        optimizer_options: OptimizerOptions,
        console: bool,
    ) -> Result<(Context, AsyncResult, WorkerHandles), String> {
        let async_result = AsyncResult::default();
        let handles = WorkerHandles::new();
        let loader = Rc::new(
            SimpleModuleLoader::new(self.path.parent().expect("test should have a parent dir"))
                .expect("test path should be canonicalizable"),
        );
        let mut context = Context::builder()
            .module_loader(loader.clone())
            .can_block(!self.flags.contains(TestFlags::CAN_BLOCK_IS_FALSE))
            .build()
            .expect("cannot fail with default global object");

        context.set_optimizer_options(optimizer_options);

        // Register the print() function.
        register_print_fn(&mut context, async_result.clone());

        // add the $262 object.
        let _js262 = js262::register_js262(handles.clone(), console, &mut context);

        if console {
            let console = boa_runtime::Console::init(&mut context);
            context
                .register_global_property(boa_runtime::Console::NAME, console, Attribute::all())
                .expect("the console builtin shouldn't exist");
        }

        if self.flags.contains(TestFlags::RAW) {
            return Ok((context, async_result, handles));
        }

        let assert = Source::from_reader(
            harness.assert.content.as_bytes(),
            Some(&harness.assert.path),
        );
        let sta = Source::from_reader(harness.sta.content.as_bytes(), Some(&harness.sta.path));

        context
            .eval(assert)
            .map_err(|e| format!("could not run assert.js:\n{e}"))?;
        context
            .eval(sta)
            .map_err(|e| format!("could not run sta.js:\n{e}"))?;

        if self.flags.contains(TestFlags::ASYNC) {
            let dph = Source::from_reader(
                harness.doneprint_handle.content.as_bytes(),
                Some(&harness.doneprint_handle.path),
            );
            context
                .eval(dph)
                .map_err(|e| format!("could not run doneprintHandle.js:\n{e}"))?;
        }

        for include_name in &self.includes {
            // `testTypedArray.js` asserts a `TypedArray` global aliasing the
            // %TypedArray% intrinsic, which the spec deliberately does not
            // expose (SS 18 lists no such binding) and the engine therefore
            // does not provide. Other runners inject the alias; so do we,
            // scoped to exactly the tests that include this harness file.
            if &**include_name == "testTypedArray.js" {
                context
                    .eval(Source::from_bytes(
                        "var TypedArray = Object.getPrototypeOf(Uint8Array);",
                    ))
                    .map_err(|e| format!("could not inject the TypedArray alias:\n{e}"))?;
            }
            let include = harness
                .includes
                .get(include_name)
                .ok_or_else(|| format!("could not find the {include_name} include file."))?;
            let source = Source::from_reader(include.content.as_bytes(), Some(&include.path));
            context.eval(source).map_err(|e| {
                format!("could not run the harness `{include_name}`:\nUncaught {e}")
            })?;
        }

        Ok((context, async_result, handles))
    }
}

/// Returns `true` if `error` is a `target_type` error.
fn is_error_type(error: &JsError, target_type: ErrorType, context: &mut Context) -> bool {
    if let Ok(error) = error.try_native(context) {
        match error.kind() {
            JsNativeErrorKind::Syntax if target_type == ErrorType::SyntaxError => {}
            JsNativeErrorKind::Reference if target_type == ErrorType::ReferenceError => {}
            JsNativeErrorKind::Range if target_type == ErrorType::RangeError => {}
            JsNativeErrorKind::Type if target_type == ErrorType::TypeError => {}
            _ => return false,
        }
        true
    } else {
        error
            .as_opaque()
            .expect("try_native cannot fail if e is not opaque")
            .as_object()
            .and_then(|o| o.get(js_str!("constructor"), context).ok())
            .as_ref()
            .and_then(JsValue::as_object)
            .and_then(|o| o.get(js_str!("name"), context).ok())
            .as_ref()
            .and_then(JsValue::as_string)
            .is_some_and(|s| s == target_type.as_str())
    }
}

/// Registers the print function in the context.
fn register_print_fn(context: &mut Context, async_result: AsyncResult) {
    // We use `FunctionBuilder` to define a closure with additional captures.
    let js_function = FunctionObjectBuilder::new(
        context.realm(),
        // SAFETY: `AsyncResult` has only non-traceable captures, making this safe.
        unsafe {
            NativeFunction::from_closure(move |_, args, context| {
                let message = args
                    .get_or_undefined(0)
                    .to_string(context)?
                    .to_std_string_escaped();
                let mut result = async_result.inner.borrow_mut();

                match *result {
                    UninitResult::Uninit | UninitResult::Ok(()) => {
                        if message == "Test262:AsyncTestComplete" {
                            *result = UninitResult::Ok(());
                        } else {
                            *result = UninitResult::Err(message);
                        }
                    }
                    UninitResult::Err(_) => {}
                }

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
        .expect("shouldn't fail with the default global");
}

/// Outcome line printed by an isolated child on stdout.
///
/// Protocol: the child prints exactly one JSON line and exits 0 for every
/// completed attempt (including failures and caught panics). Anything else —
/// nonzero exit, signal death, unparsable output — is classified by the
/// parent as a crash or harness error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SingleOutcome {
    pub(crate) outcome: TestOutcomeResult,
    pub(crate) text: String,
}

/// Maximum child output bytes captured per stream.
const MAX_CHILD_OUTPUT: u64 = 65_536;

/// Maximum result-text bytes kept per test.
///
/// The isolated child truncates to this bound *before* serializing its
/// outcome line, so the protocol payload always fits the OS pipe buffer
/// with room to spare; the parent truncates to the same bound, so stored
/// text is identical on both paths.
pub(crate) const MAX_RESULT_TEXT: usize = 4096;

/// Poll interval while waiting on an isolated child.
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Runs one strictness attempt in a child process with a wall-clock budget.
///
/// Returns the outcome, a bounded result text, and a crash record for
/// abnormal outcomes. A thread timeout would be unsound here — panics and
/// infinite loops must not take down (or stall) the runner — so isolation is
/// a real subprocess that the parent kills on expiry.
#[allow(clippy::too_many_arguments)]
fn run_child(
    test_path: &Path,
    test262_root: &Path,
    strict: bool,
    optimize: bool,
    console: bool,
    timeout_secs: u64,
    test_id: &str,
    gc_stress: Option<NonZeroU64>,
) -> (TestOutcomeResult, String, Option<CrashRecord>) {
    let harness_error = |message: String| {
        (
            TestOutcomeResult::HarnessError,
            message.clone(),
            Some(CrashRecord::new(
                test_id,
                TestOutcomeResult::HarnessError,
                message,
                Vec::new(),
            )),
        )
    };

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => return harness_error(format!("could not locate tester binary: {err}")),
    };
    let mut command = Command::new(exe);
    command
        .arg("run-single")
        .arg("--test262-path")
        .arg(test262_root)
        .arg("--test")
        .arg(test_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if strict {
        command.arg("--strict");
    }
    if optimize {
        command.arg("--optimize");
    }
    if console {
        command.arg("--console");
    }
    if let Some(n) = gc_stress {
        command.arg("--gc-stress").arg(n.to_string());
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => return harness_error(format!("could not spawn isolated child: {err}")),
    };

    // Drain both pipes on dedicated threads: the OS pipe buffer (~64 KiB)
    // is small, and the wait loop below only polls for exit. Without
    // concurrent draining, a child writing more than the buffer (a huge
    // completion-value dump, a deep backtrace on stderr) blocks forever
    // while the parent waits — a deadlock misreported as `Timeout`.
    let stdout_drain = spawn_drain(child.stdout.take());
    let stderr_drain = spawn_drain(child.stderr.take());

    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_drain(stdout_drain);
                let stderr = join_drain(stderr_drain);
                return finish_child(status, &stdout, &stderr, test_id);
            }
            Ok(None) if Instant::now() >= deadline => {
                drop(child.kill());
                drop(child.wait());
                // The pipes hit EOF once the child dies, so the drain
                // threads always terminate; join them to reclaim the
                // threads promptly.
                drop(join_drain(stdout_drain));
                drop(join_drain(stderr_drain));
                let message = format!("exceeded the {timeout_secs}s wall-clock budget");
                return (
                    TestOutcomeResult::Timeout,
                    message.clone(),
                    Some(CrashRecord::new(
                        test_id,
                        TestOutcomeResult::Timeout,
                        message,
                        Vec::new(),
                    )),
                );
            }
            Ok(None) => std::thread::sleep(CHILD_POLL_INTERVAL),
            Err(err) => {
                return harness_error(format!("could not wait on isolated child: {err}"));
            }
        }
    }
}

/// Spawns a thread draining one child pipe into a bounded buffer.
fn spawn_drain(
    pipe: Option<impl Read + Send + 'static>,
) -> Option<std::thread::JoinHandle<Vec<u8>>> {
    pipe.map(|pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            drop(pipe.take(MAX_CHILD_OUTPUT).read_to_end(&mut buf));
            buf
        })
    })
}

/// Joins a drain thread into a lossy string, tolerating any failure.
fn join_drain(handle: Option<std::thread::JoinHandle<Vec<u8>>>) -> String {
    let buf = handle.map_or_else(Vec::new, |handle| handle.join().unwrap_or_default());
    String::from_utf8_lossy(&buf).into_owned()
}

/// Finishes a naturally-exited child: parses its outcome line.
fn finish_child(
    status: std::process::ExitStatus,
    stdout: &str,
    stderr: &str,
    test_id: &str,
) -> (TestOutcomeResult, String, Option<CrashRecord>) {
    if status.success() {
        if let Ok(single) = serde_json::from_str::<SingleOutcome>(stdout.trim()) {
            let text = crate::crash::truncate(single.text, MAX_RESULT_TEXT);
            let crash = single.outcome.is_abnormal().then(|| {
                CrashRecord::new(
                    test_id,
                    single.outcome,
                    if stderr.trim().is_empty() {
                        text.clone()
                    } else {
                        format!("{text}\nchild stderr:\n{}", stderr.trim())
                    },
                    Vec::new(),
                )
            });
            (single.outcome, text, crash)
        } else {
            let message = format!("unparsable child output: {}", truncate_line(stdout, 300));
            (
                TestOutcomeResult::HarnessError,
                message.clone(),
                Some(CrashRecord::new(
                    test_id,
                    TestOutcomeResult::HarnessError,
                    message,
                    Vec::new(),
                )),
            )
        }
    } else {
        let (outcome, mut message) = classify_abnormal_exit(status);
        if !stderr.trim().is_empty() {
            message.push_str("\nchild stderr:\n");
            message.push_str(stderr.trim());
        }
        let message = crate::crash::truncate(message, MAX_RESULT_TEXT);
        (
            outcome,
            message.clone(),
            Some(CrashRecord::new(test_id, outcome, message, Vec::new())),
        )
    }
}

/// Classifies a child that did not exit successfully.
///
/// Signal deaths (abort, segfault, OOM kill) are crashes; plain nonzero
/// exits are harness errors. Note: Windows has no signals, so abnormal
/// child deaths there classify as harness errors.
fn classify_abnormal_exit(status: std::process::ExitStatus) -> (TestOutcomeResult, String) {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return (
            TestOutcomeResult::Crash,
            format!("child died on signal {signal}"),
        );
    }
    let code = status.code().map_or_else(
        || String::from("unknown status"),
        |code| format!("code {code}"),
    );
    (
        TestOutcomeResult::HarnessError,
        format!("child exited with {code}"),
    )
}

/// First line of `text`, truncated to `max` chars.
fn truncate_line(text: &str, max: usize) -> String {
    crate::crash::truncate(text.lines().next().unwrap_or("").trim().to_string(), max)
}

/// Parses a module and registers it into the `ModuleLoader` of the context.
fn parse_module_and_register(
    source: Source<'_, impl ReadChar>,
    path: &Path,
    context: &mut Context,
) -> JsResult<Module> {
    let module = Module::parse(source, None, context)?;

    let path = path
        .canonicalize()
        .expect("test path should be canonicalizable");

    let loader = context
        .downcast_module_loader::<SimpleModuleLoader>()
        .expect("context must use a SimpleModuleLoader");

    loader.insert(path, module.clone());

    Ok(module)
}

/// A `Result` value that is possibly uninitialized.
///
/// This is mainly used to check if an async test did call `print` to signal the termination of
/// a test. Otherwise, all async tests that result in `UninitResult::Uninit` are considered
/// failures.
///
/// The Test262 [interpreting guide][guide] contains more information about how to run async tests.
///
/// [guide]: https://github.com/tc39/test262/blob/main/INTERPRETING.md#flags
#[derive(Debug, Clone, Copy, Default)]
enum UninitResult<T, E> {
    #[default]
    Uninit,
    Ok(T),
    Err(E),
}

/// Object which includes the result of the async operation.
#[derive(Debug, Clone)]
struct AsyncResult {
    inner: Rc<RefCell<UninitResult<(), String>>>,
}

impl Default for AsyncResult {
    #[inline]
    fn default() -> Self {
        Self {
            inner: Rc::new(RefCell::new(UninitResult::default())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SingleOutcome, classify_abnormal_exit};
    use crate::TestOutcomeResult;

    #[test]
    fn single_outcome_round_trips() {
        let outcome = SingleOutcome {
            outcome: TestOutcomeResult::Passed,
            text: String::from("ok"),
        };
        let json = serde_json::to_string(&outcome).expect("serialize");
        assert_eq!(json, r#"{"outcome":"O","text":"ok"}"#);
        let back: SingleOutcome = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.outcome, TestOutcomeResult::Passed);
    }

    #[test]
    #[cfg(unix)]
    fn signal_death_classifies_as_crash() {
        use std::os::unix::process::ExitStatusExt;
        let killed = std::process::ExitStatus::from_raw(9);
        let (outcome, message) = classify_abnormal_exit(killed);
        assert_eq!(outcome, TestOutcomeResult::Crash);
        assert!(message.contains("signal 9"), "{message}");
    }

    #[test]
    fn nonzero_exit_classifies_as_harness_error() {
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(1 << 8)
        };
        #[cfg(not(unix))]
        let status = {
            // No portable exit-status constructor; spawn a failing child.
            std::process::Command::new(std::env::current_exe().expect("exe"))
                .arg("definitely-not-a-subcommand")
                .status()
                .expect("spawn")
        };
        let (outcome, message) = classify_abnormal_exit(status);
        assert_eq!(outcome, TestOutcomeResult::HarnessError);
        assert!(message.contains("code"), "{message}");
    }
}
