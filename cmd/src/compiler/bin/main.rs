use clap::Parser;
use std::io::{self, Read, Write};

#[derive(Parser)]
#[command(name = "cc1219", about = "Native C compiler for the UNIVAC 1219")]
struct Cli {
    /// Input C source file ("-" reads from stdin)
    #[arg(default_value = "-")]
    input: String,

    /// Output tape file (.76 format); omit to write to stdout
    #[arg(short, long)]
    output: Option<String>,

    /// Emit assembly text instead of a binary tape
    #[arg(long)]
    emit_asm: bool,
}

fn main() {
    let cli = Cli::parse();

    // Read source.
    let source = if cli.input == "-" {
        let mut buf = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut buf) {
            eprintln!("cannot read stdin: {e}");
            std::process::exit(1);
        }
        buf
    } else {
        match std::fs::read_to_string(&cli.input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot read {}: {e}", cli.input);
                std::process::exit(1);
            }
        }
    };

    if cli.emit_asm {
        let asm = match compiler::compile_to_asm(&source) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("compile error: {e}");
                std::process::exit(1);
            }
        };

        match &cli.output {
            Some(path) => {
                if let Err(e) = std::fs::write(path, &asm) {
                    eprintln!("cannot write {path}: {e}");
                    std::process::exit(1);
                }
            }
            None => print!("{asm}"),
        }
    } else {
        let tape = match compiler::compile(&source) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("compile error: {e}");
                std::process::exit(1);
            }
        };

        let bytes = common::tape::serialize_bin(&tape);

        match &cli.output {
            Some(path) => {
                if let Err(e) = std::fs::write(path, &bytes) {
                    eprintln!("cannot write {path}: {e}");
                    std::process::exit(1);
                }
            }
            None => {
                if let Err(e) = io::stdout().write_all(&bytes) {
                    eprintln!("cannot write stdout: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
