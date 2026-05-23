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
        io::stdin()
            .read_to_string(&mut buf)
            .unwrap_or_else(|e| panic!("cannot read stdin: {e}"));
        buf
    } else {
        std::fs::read_to_string(&cli.input)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", cli.input))
    };

    if cli.emit_asm {
        // Compile to assembly text only.
        let asm = compiler::compile_to_asm(&source)
            .unwrap_or_else(|e| {
                eprintln!("compile error: {e}");
                std::process::exit(1);
            });

        match &cli.output {
            Some(path) => std::fs::write(path, &asm)
                .unwrap_or_else(|e| panic!("cannot write {path}: {e}")),
            None => print!("{asm}"),
        }
    } else {
        // Compile all the way to a tape.
        let tape = compiler::compile(&source).unwrap_or_else(|e| {
            eprintln!("compile error: {e}");
            std::process::exit(1);
        });

        // Tape is Vec<u6>; convert to raw bytes for file I/O.
        let bytes: Vec<u8> = tape.iter().map(|t| u8::from(*t)).collect();

        match &cli.output {
            Some(path) => std::fs::write(path, &bytes)
                .unwrap_or_else(|e| panic!("cannot write {path}: {e}")),
            None => io::stdout()
                .write_all(&bytes)
                .unwrap_or_else(|e| panic!("cannot write stdout: {e}")),
        }
    }
}
