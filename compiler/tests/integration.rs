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

    assert!(emu.running == false, "program did not halt within 5M steps");

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
