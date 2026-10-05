//! Fuzzilli REPRL adapter for Boa (P4.7).
//!
//! Speaks the REPRL child protocol (fixed fds 100-103, `HELO`/`exec`
//! framing) so `FuzzilliCli` can drive Boa: one fresh `Context` per script,
//! fuel-capped, with `SanitizerCoverage` edge feedback through the SHM
//! bitmap Fuzzilli provides (`SHM_ID`). Signal deaths and aborts surface
//! as crashes; uncaught JS exceptions (including fuel exhaustion) report
//! failure, never crash.

// A CLI adapter: printing outcomes/diagnostics is the job (same allow as
// `boa_cli`'s root).
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod cov;
mod eval;
mod reprl;

use clap::Parser;

/// Fuzzilli REPRL adapter for the Boa JavaScript engine.
#[derive(Debug, Parser)]
#[command(name = "boa-reprl", version)]
struct Cli {
    /// Speak the REPRL child protocol on fds 100-103 (what Fuzzilli spawns).
    #[arg(long)]
    reprl: bool,

    /// One-shot mode: evaluate the file and print the outcome (no Fuzzilli).
    #[arg(long, value_name = "PATH", conflicts_with = "reprl")]
    script: Option<String>,

    /// Instruction fuel per script (bounds execution deterministically).
    #[arg(long, value_name = "N", default_value_t = 65536)]
    fuel: usize,
}

fn main() {
    let cli = Cli::parse();
    if let Some(path) = cli.script {
        let source = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            eprintln!("could not read {path}: {err}");
            std::process::exit(2);
        });
        match eval::eval_once(&source, cli.fuel) {
            eval::Outcome::Completed => println!("completed"),
            eval::Outcome::Threw(class) => println!("threw {class}"),
        }
    } else if cli.reprl {
        reprl::run_loop(cli.fuel);
    } else {
        eprintln!("pass --reprl (Fuzzilli child) or --script PATH (one-shot)");
        std::process::exit(2);
    }
}
