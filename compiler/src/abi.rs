/// ORG address — all compiler output goes in one block at this page.
/// Page 1 of UNIVAC address space: 0o010000 – 0o017777 (4096 words).
pub const PROG_ORG: u32 = 0o010000;

/// The compiler-generated start label (jumped to by the bootloader via SADD).
pub const START_LABEL: &str = "__start";

/// Reserved word: software stack pointer value (an address, stored at __sp).
/// Grows downward from just below the end of the page.
pub const SP_LABEL: &str = "__sp";

/// Initial SP value — top of the soft-stack region within this page.
/// (We leave 0o017000–0o017777 = 512 words for the stack.)
pub const STACK_INIT: u32 = 0o017777;

/// Constant zero word, used when a zero address operand is needed.
pub const ZERO_LABEL: &str = "__zero";

/// Prefix for global variable labels.
pub const GLOBAL_PREFIX: &str = "__g_";

/// Prefix for function entry labels.
pub const FN_PREFIX: &str = "__fn_";

/// Prefix for function local-variable labels.
pub const LOCAL_PREFIX: &str = "__l_";

/// Prefix for function parameter slot labels.
pub const PARAM_PREFIX: &str = "__p_";

/// Prefix for compiler-generated temporaries.
pub const TMP_PREFIX: &str = "__t";

/// Prefix for compiler-generated constant DATA words.
pub const CONST_PREFIX: &str = "__c";

/// Maximum signed integer representable in 18-bit one's complement.
pub const INT_MAX: i64 = 131_071;
/// Minimum signed integer representable in 18-bit one's complement.
pub const INT_MIN: i64 = -131_071;

/// Maximum value that fits in ENTALK's 12-bit sign-extended immediate field.
pub const ENTALK_MAX: i64 = 2047;
pub const ENTALK_MIN: i64 = -2047;

/// Convert a C signed integer to the 18-bit one's complement representation
/// used by UNIVAC 1219 (returned as a u32 for use in assembly DATA words).
pub fn to_univac_ones_complement(v: i64) -> u32 {
    assert!(v >= INT_MIN && v <= INT_MAX, "integer {v} out of 18-bit range");
    if v >= 0 {
        v as u32
    } else {
        // One's complement negation: flip all 18 bits.  NOT n = 0o777777 - n.
        0o777777u32 - ((-v) as u32)
    }
}

/// True if value fits in ENTALK's immediate field (-2047 .. +2047).
pub fn fits_entalk(v: i64) -> bool {
    v >= ENTALK_MIN && v <= ENTALK_MAX
}
