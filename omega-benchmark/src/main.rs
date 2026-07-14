use std::env;
use std::io::{self, Write};

fn main() {
    let output = omega_benchmark::cli::run(env::args().skip(1));
    if !output.stdout.is_empty() {
        let _ = io::stdout().write_all(output.stdout.as_bytes());
    }
    if !output.stderr.is_empty() {
        let _ = io::stderr().write_all(output.stderr.as_bytes());
    }
    std::process::exit(output.exit_code);
}
