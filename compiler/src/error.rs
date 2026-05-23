use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Unsupported C feature: {0}")]
    Unsupported(String),

    #[error("Undefined variable: {0}")]
    UndefinedVar(String),

    #[error("Undefined function: {0}")]
    UndefinedFunc(String),

    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Integer constant out of 18-bit UNIVAC range: {0}")]
    ConstantOutOfRange(i64),
}
