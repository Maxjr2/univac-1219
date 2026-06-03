/// Integration tests: compile C → assemble → run in emulator → check AL.
///
/// Each test:
///   1. Reads a C source fixture from tests/programs/
///   2. Compiles it to assembly via compiler::compile_to_asm()
///   3. Assembles to a tape via common::assembler::assemble()
///   4. Decodes the tape into memory segments
///   5. Runs the emulator until STOP (or 5 000 000 step limit)
///   6. Asserts the AL register equals the expected value

use common::{assembler, emulator::State, memory::Layout, tape};
use ux::{u6, u18};

fn run_c_file(path: &str) -> i32 {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    run_c_source(&source)
}

fn run_c_source(source: &str) -> i32 {
    // Compile to assembly text.
    let asm = compiler::compile_to_asm(source)
        .unwrap_or_else(|e| panic!("compile error: {e}"));

    // Assemble to tape.
    let lines: Vec<&str> = asm.lines().collect();
    let tape_data = assembler::assemble(&lines, false);

    // Decode tape into memory layout.
    let layout = Layout::decode_76(&mut tape_data.iter().copied().peekable());

    // Build clean memory (all zeros).
    let mut mem: Vec<u18> = vec![u18::new(0); 40 * 1024];

    // Load program segments into memory.
    for seg in &layout.segments {
        for (addr, word) in seg.enumerate() {
            let a = u32::from(addr) as usize;
            mem[a] = word;
        }
    }

    // Create emulator state.
    let mut emu = State::new(mem, tape::new());
    emu.p = layout.start_address;
    emu.running = true;
    // Enable STOP 1 (bit 0).  Programs emit `STOP 1`; this makes the
    // emulator actually halt on that instruction.
    emu.stop = u6::new(1);

    // Run until halted or step budget exhausted.
    let mut steps = 0usize;
    while emu.running && steps < 5_000_000 {
        emu.step(false, |_| {});
        steps += 1;
    }

    assert!(!emu.running, "program did not halt within 5M steps");

    // Convert AL (UNIVAC one's complement) back to a signed i32.
    common::arith::ones_to_signed_18(emu.al)
}

// ── tests ──────────────────────────────────────────────────────────────────

#[test]
fn test_identity() {
    assert_eq!(run_c_file("tests/programs/identity.c"), 42);
}

#[test]
fn test_arithmetic_add() {
    assert_eq!(run_c_file("tests/programs/arithmetic.c"), 13);
}

#[test]
fn test_subtract() {
    assert_eq!(run_c_file("tests/programs/subtract.c"), 12);
}

#[test]
fn test_multiply() {
    assert_eq!(run_c_file("tests/programs/multiply.c"), 42);
}

#[test]
fn test_divide() {
    assert_eq!(run_c_file("tests/programs/divide.c"), 25);
}

#[test]
fn test_conditional() {
    assert_eq!(run_c_file("tests/programs/conditional.c"), 1);
}

#[test]
fn test_loop_sum() {
    // 1+2+...+10 = 55
    assert_eq!(run_c_file("tests/programs/loop.c"), 55);
}

#[test]
fn test_functions() {
    // add(15, 27) = 42
    assert_eq!(run_c_file("tests/programs/functions.c"), 42);
}

// ── inline source tests ───────────────────────────────────────────────────

#[test]
fn test_inline_negation() {
    let src = "int main() { int x; x = 10; return -x; }";
    // -10 in UNIVAC one's complement; ones_to_signed_18 gives -10
    assert_eq!(run_c_source(src), -10);
}

#[test]
fn test_inline_comparison_lt_false() {
    let src = "int main() { int a; int b; a = 5; b = 3; return (a < b); }";
    assert_eq!(run_c_source(src), 0);
}

#[test]
fn test_inline_comparison_lt_true() {
    let src = "int main() { int a; int b; a = 2; b = 7; return (a < b); }";
    assert_eq!(run_c_source(src), 1);
}

#[test]
fn test_if_else() {
    let src = r#"
    int main() {
        int x;
        x = 4;
        if (x == 4) {
            return 100;
        } else {
            return 200;
        }
    }"#;
    assert_eq!(run_c_source(src), 100);
}

#[test]
fn test_for_loop() {
    let src = r#"
    int main() {
        int i;
        int s;
        s = 0;
        for (i = 0; i < 5; i = i + 1) {
            s = s + i;
        }
        return s;
    }"#;
    // 0+1+2+3+4 = 10
    assert_eq!(run_c_source(src), 10);
}

#[test]
fn test_nested_calls() {
    let src = r#"
    int double_it(int n) {
        return n + n;
    }
    int main() {
        return double_it(21);
    }"#;
    assert_eq!(run_c_source(src), 42);
}

// ── operator coverage ─────────────────────────────────────────────────────

#[test]
fn test_modulo() {
    let src = "int main() { int x; x = 17; return x % 5; }";
    assert_eq!(run_c_source(src), 2); // 17 % 5 = 2
}

#[test]
fn test_bitwise_and() {
    let src = "int main() { int a; int b; a = 0xFF; b = 0x0F; return a & b; }";
    assert_eq!(run_c_source(src), 0x0F);
}

#[test]
fn test_bitwise_or() {
    let src = "int main() { int a; int b; a = 0xF0; b = 0x0F; return a | b; }";
    assert_eq!(run_c_source(src), 0xFF);
}

#[test]
fn test_bitwise_xor() {
    let src = "int main() { int a; int b; a = 0xFF; b = 0x0F; return a ^ b; }";
    assert_eq!(run_c_source(src), 0xF0);
}

#[test]
fn test_bitwise_not() {
    // ~0 in 18-bit one's complement is 0o777777 = -0, which rounds to 0
    // ~1 = 0o777776 = -1 in signed; ones_to_signed_18 gives -1
    let src = "int main() { int x; x = 1; return ~x; }";
    assert_eq!(run_c_source(src), -1);
}

#[test]
fn test_left_shift() {
    let src = "int main() { int x; x = 3; return x << 2; }"; // 3 << 2 = 12
    assert_eq!(run_c_source(src), 12);
}

#[test]
fn test_right_shift() {
    let src = "int main() { int x; x = 12; return x >> 2; }"; // 12 >> 2 = 3
    assert_eq!(run_c_source(src), 3);
}

#[test]
fn test_logical_and_true() {
    let src = "int main() { int a; int b; a = 5; b = 3; return (a > 0) && (b > 0); }";
    assert_eq!(run_c_source(src), 1);
}

#[test]
fn test_logical_and_false() {
    let src = "int main() { int a; int b; a = 5; b = 0; return (a > 0) && (b > 0); }";
    assert_eq!(run_c_source(src), 0);
}

#[test]
fn test_logical_or_true() {
    let src = "int main() { int a; int b; a = 0; b = 3; return (a > 0) || (b > 0); }";
    assert_eq!(run_c_source(src), 1);
}

#[test]
fn test_logical_or_false() {
    let src = "int main() { int a; int b; a = 0; b = 0; return (a > 0) || (b > 0); }";
    assert_eq!(run_c_source(src), 0);
}

#[test]
fn test_logical_not() {
    let src = "int main() { int x; x = 0; return !x; }";
    assert_eq!(run_c_source(src), 1);
}

#[test]
fn test_negative_constant() {
    // Negative literal in ENTALK range (uses octal ones-complement encoding)
    let src = "int main() { int x; x = -7; return x; }";
    assert_eq!(run_c_source(src), -7);
}

// ── global variables ──────────────────────────────────────────────────────

#[test]
fn test_global_var() {
    let src = r#"
    int g;
    int main() {
        g = 99;
        return g;
    }"#;
    assert_eq!(run_c_source(src), 99);
}

#[test]
fn test_global_shared() {
    let src = r#"
    int counter;
    int bump() {
        counter = counter + 1;
        return counter;
    }
    int main() {
        counter = 0;
        bump();
        bump();
        return bump();
    }"#;
    assert_eq!(run_c_source(src), 3);
}

// ── break / continue ──────────────────────────────────────────────────────

#[test]
fn test_break_while() {
    let src = r#"
    int main() {
        int i;
        i = 0;
        while (i < 10) {
            if (i == 5) {
                break;
            }
            i = i + 1;
        }
        return i;
    }"#;
    assert_eq!(run_c_source(src), 5);
}

#[test]
fn test_continue_for() {
    // Sum only even numbers from 0..9: 0+2+4+6+8 = 20
    let src = r#"
    int main() {
        int i;
        int s;
        s = 0;
        for (i = 0; i < 10; i = i + 1) {
            if (i % 2 != 0) {
                continue;
            }
            s = s + i;
        }
        return s;
    }"#;
    assert_eq!(run_c_source(src), 20);
}

#[test]
fn test_do_while() {
    // do { i++; } while (i < 5);  starts at 0, ends at 5
    let src = r#"
    int main() {
        int i;
        i = 0;
        do {
            i = i + 1;
        } while (i < 5);
        return i;
    }"#;
    assert_eq!(run_c_source(src), 5);
}

// ── existing assembly smoke-test ──────────────────────────────────────────
// Assemble and run a few steps of existing UNIVAC assembly programs to
// ensure the assembler/emulator haven't been broken.

#[test]
fn existing_pi_assembles() {
    let src = std::fs::read_to_string("../examples/PI.TXT").unwrap();
    let lines: Vec<&str> = src.lines().collect();
    // If this panics, the assembler is broken.
    let _tape = assembler::assemble(&lines, false);
}

#[test]
fn existing_ctest3_assembles() {
    let src = std::fs::read_to_string("../examples/CTEST3.TXT").unwrap();
    let lines: Vec<&str> = src.lines().collect();
    let _tape = assembler::assemble(&lines, false);
}
