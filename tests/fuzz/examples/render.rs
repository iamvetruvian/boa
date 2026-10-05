//! Render a fuzzer artifact back to human-readable input (P4.6 triage).
//!
//! Fuzz artifacts are raw fuzzer bytes; for structured targets the bytes are
//! an `Arbitrary` encoding, not the program. This helper replays the target's
//! own input decoding and prints what the target saw, so crash triage and
//! regression-DB admission start from readable text instead of hex.
//!
//! Usage: `cargo run --example render -- <target> <artifact>`

use arbitrary::{Arbitrary, Unstructured};
use boa_fuzz::common::{FuzzData, FuzzSource};

fn load_source<T>(bytes: &[u8], render: impl FnOnce(T) -> String, what: &str) -> String
where
    T: for<'a> Arbitrary<'a>,
{
    let mut unstructured = Unstructured::new(bytes);
    match T::arbitrary(&mut unstructured) {
        Ok(input) => render(input),
        Err(err) => format!(
            "<could not decode {what} from {} bytes: {err}>",
            bytes.len()
        ),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: cargo run --example render -- <target> <artifact>");
        std::process::exit(2);
    }
    let bytes = std::fs::read(&args[2]).unwrap_or_else(|err| {
        eprintln!("could not read {}: {err}", args[2]);
        std::process::exit(2);
    });
    let text = match args[1].as_str() {
        "vm-implied" | "bytecompiler-implied" => {
            load_source::<FuzzSource>(&bytes, |input| input.source, "FuzzSource")
        }
        "parser-idempotency" => load_source::<FuzzData>(
            &bytes,
            |data| {
                use boa_interner::ToInternedString;
                data.ast.to_interned_string(&data.interner)
            },
            "FuzzData",
        ),
        "cli-exec" | "source-bytes" => String::from_utf8_lossy(&bytes).into_owned(),
        other => format!(
            "<{} takes a target-local structured input (see fuzz_targets/{}.rs); \
             artifact is {} raw bytes, shown below as lossy text>\n{}",
            other,
            other,
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        ),
    };
    const CAP: usize = 64 * 1024;
    if text.len() > CAP {
        println!(
            "{}...\n<output truncated at {CAP} of {} bytes>",
            &text[..CAP],
            text.len()
        );
    } else {
        println!("{text}");
    }
}
