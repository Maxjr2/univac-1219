pub mod abi;
pub mod codegen;
pub mod error;

use error::CompileError;
use codegen::Compiler;

use lang_c::driver::{Config, parse_preprocessed};

/// Compile a C source string to UNIVAC 1219 assembly text.
///
/// The returned string is suitable for passing directly to
/// `common::assembler::assemble()`.
pub fn compile_to_asm(source: &str) -> Result<String, CompileError> {
    let config = Config::default();
    let parse = parse_preprocessed(&config, source.to_string())
        .map_err(|e| CompileError::Parse(format!("{e:?}")))?;

    let mut compiler = Compiler::new();
    compiler.compile_unit(&parse.unit)?;
    Ok(compiler.finish())
}

/// Compile a C source string all the way to a `.76` tape.
pub fn compile(source: &str) -> Result<common::tape::Tape, CompileError> {
    let asm = compile_to_asm(source)?;
    let lines: Vec<&str> = asm.lines().collect();
    Ok(common::assembler::assemble(&lines, false))
}
