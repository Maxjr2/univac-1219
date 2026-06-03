/// Code generator: walks a lang-c AST and emits UNIVAC 1219 assembly text.
///
/// Calling convention (Phase 1 — non-recursive, static allocation):
///   * Each function gets a per-function return-address slot at its entry label.
///     `RJP FUNC` stores the return address there; `IJP FUNC` returns.
///   * Arguments are passed via static memory slots __p_FUNCNAME_N.
///   * Return value is left in AL.
///   * Local variables are static slots __l_FUNCNAME_VARNAME.
///   * Everything lives in one ORG block (page 1, 0o010000–0o017777).

use std::collections::HashMap;

use lang_c::ast::*;
use lang_c::span::Node;

use crate::abi::*;
use crate::error::CompileError;

// ── helpers ───────────────────────────────────────────────────────────────────

fn univac_dec(v: i64) -> u32 {
    to_univac_ones_complement(v)
}

// ── compiler state ────────────────────────────────────────────────────────────

pub struct Compiler {
    code: Vec<String>,
    data: Vec<String>,

    label_cnt: usize,
    tmp_cnt: usize,
    const_cnt: usize,

    current_fn: Option<String>,
    locals: HashMap<String, String>,
    globals: HashMap<String, String>,
    functions: HashMap<String, usize>,

    // Stack of (break_label, continue_label) for nested loops.
    loop_labels: Vec<(String, String)>,

    pending_label: Option<String>,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            code: Vec::new(),
            data: Vec::new(),
            label_cnt: 0,
            tmp_cnt: 0,
            const_cnt: 0,
            current_fn: None,
            locals: HashMap::new(),
            globals: HashMap::new(),
            functions: HashMap::new(),
            loop_labels: Vec::new(),
            pending_label: None,
        }
    }

    // ── label / emit helpers ──────────────────────────────────────────────

    fn fresh_label(&mut self, prefix: &str) -> String {
        let n = self.label_cnt;
        self.label_cnt += 1;
        format!("{prefix}{n}")
    }

    fn set_label(&mut self, label: String) {
        if let Some(prev) = self.pending_label.take() {
            // Two labels with no instruction between them: emit a harmless
            // DATA 0 to anchor the first one.
            self.code.push(format!(">{prev} DATA 0"));
        }
        self.pending_label = Some(label);
    }

    fn emit(&mut self, instr: &str) {
        let line = if let Some(lbl) = self.pending_label.take() {
            format!(">{lbl} {instr}")
        } else {
            instr.to_string()
        };
        self.code.push(line);
    }

    fn emit_data_decl(&mut self, label: &str, value: u32) {
        self.data.push(format!(">{label} DATA {value}"));
    }

    // ── allocation helpers ────────────────────────────────────────────────

    fn alloc_temp(&mut self) -> String {
        let label = format!("{TMP_PREFIX}{}", self.tmp_cnt);
        self.tmp_cnt += 1;
        self.emit_data_decl(&label, 0);
        label
    }

    fn alloc_const_word(&mut self, value: u32) -> String {
        let label = format!("{CONST_PREFIX}{}", self.const_cnt);
        self.const_cnt += 1;
        self.emit_data_decl(&label, value);
        label
    }

    // ── loading values into AL ────────────────────────────────────────────

    fn emit_load_const(&mut self, v: i64) -> Result<(), CompileError> {
        if v < INT_MIN || v > INT_MAX {
            return Err(CompileError::ConstantOutOfRange(v));
        }
        if fits_entalk(v) {
            if v >= 0 {
                self.emit(&format!("ENTALK {v}"));
            } else {
                // ENTALK takes a 12-bit unsigned immediate that is sign-extended.
                // Represent -N as its 12-bit ones-complement value (0o7777 ^ N).
                let oc12 = 0o7777u32 ^ ((-v) as u32 & 0o7777);
                self.emit(&format!("ENTALK &O{oc12:o}"));
            }
        } else {
            let lbl = self.alloc_const_word(univac_dec(v));
            self.emit(&format!("ENTAL {lbl}"));
        }
        Ok(())
    }

    fn resolve_var(&self, name: &str) -> Result<String, CompileError> {
        if let Some(l) = self.locals.get(name) {
            return Ok(l.clone());
        }
        if let Some(l) = self.globals.get(name) {
            return Ok(l.clone());
        }
        Err(CompileError::UndefinedVar(name.to_string()))
    }

    // ── sign-extension: AL → AU:AL (for DIVA) ────────────────────────────

    fn emit_sign_extend_al(&mut self) {
        let saved = self.alloc_temp();
        let pos_l = self.fresh_label("__sep");
        let end_l = self.fresh_label("__see");
        self.emit(&format!("STRAL {saved}"));
        self.emit(&format!("JPALP {pos_l}"));  // AL > 0
        self.emit(&format!("JPALZ {pos_l}"));  // AL == 0
        // Negative: AU = all-ones (= -0 in one's complement)
        let neg_zero = self.alloc_const_word(0o777777);
        self.emit(&format!("ENTAU {neg_zero}"));
        self.emit(&format!("ENTAL {saved}"));
        self.emit(&format!("JP {end_l}"));
        // Positive or zero:
        self.set_label(pos_l);
        self.emit(&format!("ENTAU {ZERO_LABEL}"));
        self.emit(&format!("ENTAL {saved}"));
        self.set_label(end_l);
    }

    // ── expressions ───────────────────────────────────────────────────────

    pub fn compile_expr(&mut self, expr: &Node<Expression>) -> Result<(), CompileError> {
        match &expr.node {
            Expression::Constant(c) => {
                let v = parse_constant(&c.node)?;
                self.emit_load_const(v)?;
            }

            Expression::Identifier(id) => {
                let lbl = self.resolve_var(&id.node.name)?;
                self.emit(&format!("ENTAL {lbl}"));
            }

            Expression::UnaryOperator(u) => {
                self.compile_unary(u)?;
            }

            Expression::BinaryOperator(b) => {
                self.compile_binop(b)?;
            }

            Expression::Call(call) => {
                self.compile_call(&call.node)?;
            }

            _ => {
                return Err(CompileError::Unsupported(format!(
                    "expression not yet supported"
                )));
            }
        }
        Ok(())
    }

    fn compile_unary(&mut self, u: &Node<UnaryOperatorExpression>) -> Result<(), CompileError> {
        match u.node.operator.node {
            UnaryOperator::Minus => {
                self.compile_expr(&u.node.operand)?;
                // One's-complement negation.  CPAL: AL = ~AL, but CPAL
                // leaves +0 unchanged (correct per hardware spec).
                self.emit("CPAL");
            }
            UnaryOperator::Complement => {
                self.compile_expr(&u.node.operand)?;
                self.emit("CPAL");
            }
            UnaryOperator::Negate => {
                // Logical NOT: 1 if AL==0, else 0
                self.compile_expr(&u.node.operand)?;
                let true_l = self.fresh_label("__lnot_t");
                let end_l  = self.fresh_label("__lnot_e");
                self.emit(&format!("JPALZ {true_l}"));
                self.emit("ENTALK 0");
                self.emit(&format!("JP {end_l}"));
                self.set_label(true_l);
                self.emit("ENTALK 1");
                self.set_label(end_l);
            }
            UnaryOperator::Plus => {
                self.compile_expr(&u.node.operand)?;
            }
            UnaryOperator::PreIncrement => {
                if let Expression::Identifier(id) = &u.node.operand.node {
                    let lbl = self.resolve_var(&id.node.name)?;
                    self.emit(&format!("ENTAL {lbl}"));
                    self.emit("ADDALK 1");
                    self.emit(&format!("STRAL {lbl}"));
                } else {
                    return Err(CompileError::Unsupported("++<non-ident>".into()));
                }
            }
            UnaryOperator::PostIncrement => {
                if let Expression::Identifier(id) = &u.node.operand.node {
                    let lbl = self.resolve_var(&id.node.name)?;
                    let saved = self.alloc_temp();
                    self.emit(&format!("ENTAL {lbl}"));
                    self.emit(&format!("STRAL {saved}"));  // save old value
                    self.emit("ADDALK 1");
                    self.emit(&format!("STRAL {lbl}"));    // store incremented
                    self.emit(&format!("ENTAL {saved}"));  // return old value
                } else {
                    return Err(CompileError::Unsupported("<non-ident>++".into()));
                }
            }
            UnaryOperator::PreDecrement => {
                if let Expression::Identifier(id) = &u.node.operand.node {
                    let lbl = self.resolve_var(&id.node.name)?;
                    let one = self.alloc_const_word(1);
                    self.emit(&format!("ENTAL {lbl}"));
                    self.emit(&format!("SUBAL {one}"));
                    self.emit(&format!("STRAL {lbl}"));
                } else {
                    return Err(CompileError::Unsupported("--<non-ident>".into()));
                }
            }
            UnaryOperator::PostDecrement => {
                if let Expression::Identifier(id) = &u.node.operand.node {
                    let lbl = self.resolve_var(&id.node.name)?;
                    let one = self.alloc_const_word(1);
                    let saved = self.alloc_temp();
                    self.emit(&format!("ENTAL {lbl}"));
                    self.emit(&format!("STRAL {saved}"));
                    self.emit(&format!("SUBAL {one}"));
                    self.emit(&format!("STRAL {lbl}"));
                    self.emit(&format!("ENTAL {saved}"));
                } else {
                    return Err(CompileError::Unsupported("<non-ident>--".into()));
                }
            }
            _ => return Err(CompileError::Unsupported(
                format!("unary op {:?}", u.node.operator.node)
            )),
        }
        Ok(())
    }

    fn compile_binop(
        &mut self,
        b: &Node<BinaryOperatorExpression>,
    ) -> Result<(), CompileError> {
        use BinaryOperator::*;
        let op = b.node.operator.node.clone();

        match op {
            // ── assignments ───────────────────────────────────────────────
            Assign => {
                self.compile_expr(&b.node.rhs)?;
                let lbl = self.lvalue_label(&b.node.lhs)?;
                self.emit(&format!("STRAL {lbl}"));
            }
            AssignPlus  => self.compile_compound_assign(&b.node, "ADDAL")?,
            AssignMinus => self.compile_compound_assign(&b.node, "SUBAL")?,
            AssignMultiply => {
                self.compile_expr(&b.node.rhs)?;
                let rhs_t = self.alloc_temp();
                self.emit(&format!("STRAL {rhs_t}"));
                let lbl = self.lvalue_label(&b.node.lhs)?;
                self.emit(&format!("ENTAL {lbl}"));
                self.emit(&format!("MULAL {rhs_t}"));
                self.emit(&format!("STRAL {lbl}"));
            }
            AssignDivide => {
                self.compile_expr(&b.node.rhs)?;
                let rhs_t = self.alloc_temp();
                self.emit(&format!("STRAL {rhs_t}"));
                let lbl = self.lvalue_label(&b.node.lhs)?;
                self.emit(&format!("ENTAL {lbl}"));
                self.emit_sign_extend_al();
                self.emit(&format!("DIVA {rhs_t}"));
                self.emit(&format!("STRAL {lbl}"));
            }

            // ── arithmetic ────────────────────────────────────────────────
            Plus => {
                self.compile_expr(&b.node.lhs)?;
                let t = self.alloc_temp();
                self.emit(&format!("STRAL {t}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("ADDAL {t}"));
            }
            Minus => {
                self.compile_expr(&b.node.lhs)?;
                let lhs_t = self.alloc_temp();
                self.emit(&format!("STRAL {lhs_t}"));
                self.compile_expr(&b.node.rhs)?;
                let rhs_t = self.alloc_temp();
                self.emit(&format!("STRAL {rhs_t}"));
                self.emit(&format!("ENTAL {lhs_t}"));
                self.emit(&format!("SUBAL {rhs_t}"));
            }
            Multiply => {
                self.compile_expr(&b.node.lhs)?;
                let t = self.alloc_temp();
                self.emit(&format!("STRAL {t}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("MULAL {t}"));  // result in AL
            }
            Divide => {
                self.compile_expr(&b.node.rhs)?;
                let rhs_t = self.alloc_temp();
                self.emit(&format!("STRAL {rhs_t}"));
                self.compile_expr(&b.node.lhs)?;
                self.emit_sign_extend_al();
                self.emit(&format!("DIVA {rhs_t}"));  // AL = quotient
            }
            Modulo => {
                self.compile_expr(&b.node.rhs)?;
                let rhs_t = self.alloc_temp();
                self.emit(&format!("STRAL {rhs_t}"));
                self.compile_expr(&b.node.lhs)?;
                self.emit_sign_extend_al();
                self.emit(&format!("DIVA {rhs_t}"));  // AU = remainder
                let rem_t = self.alloc_temp();
                self.emit(&format!("STRAU {rem_t}"));
                self.emit(&format!("ENTAL {rem_t}"));  // AL = remainder
            }

            // ── bitwise ───────────────────────────────────────────────────
            BitwiseAnd => {
                self.compile_expr(&b.node.lhs)?;
                let t = self.alloc_temp();
                self.emit(&format!("STRAL {t}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("SLCL {t}"));   // AL &= mem[t]
            }
            BitwiseOr => {
                self.compile_expr(&b.node.lhs)?;
                let t = self.alloc_temp();
                self.emit(&format!("STRAL {t}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("SLSET {t}"));  // AL |= mem[t]
            }
            BitwiseXor => {
                self.compile_expr(&b.node.lhs)?;
                let t = self.alloc_temp();
                self.emit(&format!("STRAL {t}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("SLCP {t}"));   // AL ^= mem[t]
            }
            ShiftLeft => {
                self.compile_shift(&b.node.lhs, &b.node.rhs, true)?;
            }
            ShiftRight => {
                self.compile_shift(&b.node.lhs, &b.node.rhs, false)?;
            }

            // ── comparisons ───────────────────────────────────────────────
            Equals | NotEquals | Less | LessOrEqual | Greater | GreaterOrEqual => {
                self.compile_comparison(&b.node.lhs, op, &b.node.rhs)?;
            }

            // ── logical ───────────────────────────────────────────────────
            LogicalAnd => {
                let false_l = self.fresh_label("__land_f");
                let end_l   = self.fresh_label("__land_e");
                self.compile_expr(&b.node.lhs)?;
                self.emit(&format!("JPALZ {false_l}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("JPALZ {false_l}"));
                self.emit("ENTALK 1");
                self.emit(&format!("JP {end_l}"));
                self.set_label(false_l);
                self.emit("ENTALK 0");
                self.set_label(end_l);
            }
            LogicalOr => {
                let true_l = self.fresh_label("__lor_t");
                let end_l  = self.fresh_label("__lor_e");
                self.compile_expr(&b.node.lhs)?;
                self.emit(&format!("JPALNZ {true_l}"));
                self.compile_expr(&b.node.rhs)?;
                self.emit(&format!("JPALNZ {true_l}"));
                self.emit("ENTALK 0");
                self.emit(&format!("JP {end_l}"));
                self.set_label(true_l);
                self.emit("ENTALK 1");
                self.set_label(end_l);
            }

            _ => return Err(CompileError::Unsupported(
                format!("binary op {:?}", op)
            )),
        }
        Ok(())
    }

    fn compile_compound_assign(
        &mut self,
        b: &BinaryOperatorExpression,
        op_instr: &str,
    ) -> Result<(), CompileError> {
        self.compile_expr(&b.rhs)?;
        let rhs_t = self.alloc_temp();
        self.emit(&format!("STRAL {rhs_t}"));
        let lbl = self.lvalue_label(&b.lhs)?;
        self.emit(&format!("ENTAL {lbl}"));
        self.emit(&format!("{op_instr} {rhs_t}"));
        self.emit(&format!("STRAL {lbl}"));
        Ok(())
    }

    fn compile_shift(
        &mut self,
        lhs: &Node<Expression>,
        rhs: &Node<Expression>,
        left: bool,
    ) -> Result<(), CompileError> {
        // Constant shift amounts only for Phase 1.
        if let Expression::Constant(c) = &rhs.node {
            let k = parse_constant(&c.node)? as u32;
            self.compile_expr(lhs)?;
            let instr = if left { "LSHAL" } else { "RSHAL" };
            self.emit(&format!("{instr} {k}"));
        } else {
            return Err(CompileError::Unsupported(
                "variable shift amounts not yet supported".into()
            ));
        }
        Ok(())
    }

    fn compile_comparison(
        &mut self,
        lhs: &Node<Expression>,
        op: BinaryOperator,
        rhs: &Node<Expression>,
    ) -> Result<(), CompileError> {
        use BinaryOperator::*;

        self.compile_expr(lhs)?;
        let lhs_t = self.alloc_temp();
        self.emit(&format!("STRAL {lhs_t}"));
        self.compile_expr(rhs)?;
        let rhs_t = self.alloc_temp();
        self.emit(&format!("STRAL {rhs_t}"));

        // CMAL flags are consumed by exactly ONE conditional jump instruction.
        // After that jump executes, the flags are cleared.  So every comparison
        // must use exactly one conditional branch after CMAL.
        //
        // Available single-jump semantics (after ENTAL a; CMAL b):
        //   JPALZ  → a == b   (equal flag)
        //   JPALNZ → a != b   (!equal flag)
        //   JPALP  → a > b    (greater flag)
        //   JPALNG → a <= b   (!greater flag)
        //
        // For  a < b  and  a >= b  we swap operands so we can still use
        // a single jump:  a < b  ↔  b > a;  a >= b  ↔  b <= a.

        let true_l = self.fresh_label("__ct");
        let end_l  = self.fresh_label("__ce");

        match op {
            Equals => {
                self.emit(&format!("ENTAL {lhs_t}"));
                self.emit(&format!("CMAL {rhs_t}"));
                self.emit(&format!("JPALZ {true_l}"));
            }
            NotEquals => {
                self.emit(&format!("ENTAL {lhs_t}"));
                self.emit(&format!("CMAL {rhs_t}"));
                self.emit(&format!("JPALNZ {true_l}"));
            }
            Less => {
                // a < b  ↔  b > a
                self.emit(&format!("ENTAL {rhs_t}"));
                self.emit(&format!("CMAL {lhs_t}"));
                self.emit(&format!("JPALP {true_l}"));
            }
            Greater => {
                self.emit(&format!("ENTAL {lhs_t}"));
                self.emit(&format!("CMAL {rhs_t}"));
                self.emit(&format!("JPALP {true_l}"));
            }
            LessOrEqual => {
                // JPALNG: !greater = (a <= b)
                self.emit(&format!("ENTAL {lhs_t}"));
                self.emit(&format!("CMAL {rhs_t}"));
                self.emit(&format!("JPALNG {true_l}"));
            }
            GreaterOrEqual => {
                // a >= b  ↔  b <= a
                self.emit(&format!("ENTAL {rhs_t}"));
                self.emit(&format!("CMAL {lhs_t}"));
                self.emit(&format!("JPALNG {true_l}"));
            }
            _ => unreachable!(),
        }

        // False: AL = 0
        self.emit("ENTALK 0");
        self.emit(&format!("JP {end_l}"));
        // True: AL = 1
        self.set_label(true_l);
        self.emit("ENTALK 1");
        self.set_label(end_l);
        Ok(())
    }

    fn compile_call(&mut self, call: &CallExpression) -> Result<(), CompileError> {
        let func_name = match &call.callee.node {
            Expression::Identifier(id) => id.node.name.clone(),
            _ => return Err(CompileError::Unsupported("indirect function call".into())),
        };

        // Built-in putchar(c)
        if func_name == "putchar" {
            if call.arguments.len() != 1 {
                return Err(CompileError::Unsupported("putchar needs 1 arg".into()));
            }
            self.compile_expr(&call.arguments[0])?;
            self.emit("STRAL __outwd");
            // OUT 0 with TACW=__outwd, IACW=__outwd (inline the two data words)
            self.emit("OUT 0");
            self.emit("DATA __outwd");
            self.emit("DATA __outwd");
            self.emit("SKPOIN 0");
            self.emit("JP LOK-1");
            return Ok(());
        }

        // Built-in getchar() — returns char in AL
        if func_name == "getchar" {
            self.emit("SIL 0");
            self.emit("IN 0");
            self.emit("DATA __inwd");
            self.emit("DATA __inwd");
            self.emit("SKPIIN 0");
            self.emit("JP LOK-1");
            self.emit("ENTAL __inwd");
            return Ok(());
        }

        let n_params = *self.functions.get(&func_name)
            .ok_or_else(|| CompileError::UndefinedFunc(func_name.clone()))?;

        if call.arguments.len() != n_params {
            return Err(CompileError::Unsupported(format!(
                "{func_name}: expected {n_params} args, got {}",
                call.arguments.len()
            )));
        }

        // Evaluate args and store to static param slots.
        for (i, arg) in call.arguments.iter().enumerate() {
            self.compile_expr(arg)?;
            self.emit(&format!("STRAL {PARAM_PREFIX}{func_name}_{i}"));
        }

        self.emit(&format!("RJP {FN_PREFIX}{func_name}"));
        Ok(())
    }

    fn lvalue_label(&self, expr: &Node<Expression>) -> Result<String, CompileError> {
        match &expr.node {
            Expression::Identifier(id) => self.resolve_var(&id.node.name),
            _ => Err(CompileError::Unsupported("non-identifier lvalue".into())),
        }
    }

    // ── statements ────────────────────────────────────────────────────────

    fn compile_stmt(&mut self, stmt: &Node<Statement>) -> Result<(), CompileError> {
        match &stmt.node {
            Statement::Compound(items) => {
                for item in items {
                    self.compile_block_item(item)?;
                }
            }

            Statement::Return(ret) => {
                if let Some(expr) = ret {
                    self.compile_expr(expr)?;
                } else {
                    self.emit("ENTALK 0");
                }
                let fn_name = self.current_fn.clone()
                    .ok_or_else(|| CompileError::Unsupported("return outside function".into()))?;
                let ret_lbl = format!("{FN_PREFIX}{fn_name}_ret");
                self.emit(&format!("JP {ret_lbl}"));
            }

            Statement::If(if_stmt) => {
                let else_l = self.fresh_label("__ife");
                let end_l  = self.fresh_label("__ifd");

                self.compile_expr(&if_stmt.node.condition)?;
                self.emit(&format!("JPALZ {else_l}"));
                self.compile_stmt(&if_stmt.node.then_statement)?;

                if if_stmt.node.else_statement.is_some() {
                    self.emit(&format!("JP {end_l}"));
                }

                self.set_label(else_l);

                if let Some(else_s) = &if_stmt.node.else_statement {
                    self.compile_stmt(else_s)?;
                    self.set_label(end_l);
                }
            }

            Statement::While(ws) => {
                let cond_l = self.fresh_label("__whc");
                let end_l  = self.fresh_label("__whe");
                self.set_label(cond_l.clone());
                self.compile_expr(&ws.node.expression)?;
                self.emit(&format!("JPALZ {end_l}"));
                self.loop_labels.push((end_l.clone(), cond_l.clone()));
                self.compile_stmt(&ws.node.statement)?;
                self.loop_labels.pop();
                self.emit(&format!("JP {cond_l}"));
                self.set_label(end_l);
            }

            Statement::For(fs) => {
                match &fs.node.initializer.node {
                    ForInitializer::Empty => {}
                    ForInitializer::Expression(e) => { self.compile_expr(e)?; }
                    ForInitializer::Declaration(d) => { self.compile_decl(&d.node)?; }
                    _ => return Err(CompileError::Unsupported("complex for-init".into())),
                }
                let cond_l = self.fresh_label("__forc");
                // `continue` in a for loop jumps to the step expression, not the
                // condition, so that `i++` still runs before the next iteration.
                let step_l = self.fresh_label("__fors");
                let end_l  = self.fresh_label("__fore");
                self.set_label(cond_l.clone());
                if let Some(cond) = &fs.node.condition {
                    self.compile_expr(cond)?;
                    self.emit(&format!("JPALZ {end_l}"));
                }
                self.loop_labels.push((end_l.clone(), step_l.clone()));
                self.compile_stmt(&fs.node.statement)?;
                self.loop_labels.pop();
                self.set_label(step_l);
                if let Some(step) = &fs.node.step {
                    self.compile_expr(step)?;
                }
                self.emit(&format!("JP {cond_l}"));
                self.set_label(end_l);
            }

            Statement::DoWhile(dw) => {
                let top_l  = self.fresh_label("__dot");
                let cond_l = self.fresh_label("__doc"); // continue → re-evaluate condition
                let end_l  = self.fresh_label("__doe"); // break → exit
                self.set_label(top_l.clone());
                self.loop_labels.push((end_l.clone(), cond_l.clone()));
                self.compile_stmt(&dw.node.statement)?;
                self.loop_labels.pop();
                self.set_label(cond_l);
                self.compile_expr(&dw.node.expression)?;
                self.emit(&format!("JPALNZ {top_l}"));
                self.set_label(end_l);
            }

            Statement::Break => {
                let (brk, _) = self.loop_labels.last()
                    .ok_or_else(|| CompileError::Unsupported("break outside loop".into()))?;
                let lbl = brk.clone();
                self.emit(&format!("JP {lbl}"));
            }

            Statement::Continue => {
                let (_, cont) = self.loop_labels.last()
                    .ok_or_else(|| CompileError::Unsupported("continue outside loop".into()))?;
                let lbl = cont.clone();
                self.emit(&format!("JP {lbl}"));
            }

            Statement::Expression(opt_expr) => {
                if let Some(expr) = opt_expr {
                    self.compile_expr(expr)?;
                }
            }

            _ => return Err(CompileError::Unsupported(
                format!("statement not yet supported")
            )),
        }
        Ok(())
    }

    fn compile_block_item(&mut self, item: &Node<BlockItem>) -> Result<(), CompileError> {
        match &item.node {
            BlockItem::Declaration(d) => self.compile_decl(&d.node),
            BlockItem::Statement(s)   => self.compile_stmt(s),
            _ => Err(CompileError::Unsupported("unknown block item".into())),
        }
    }

    fn compile_decl(&mut self, decl: &Declaration) -> Result<(), CompileError> {
        for init_decl in &decl.declarators {
            let name = extract_decl_name(&init_decl.node.declarator)?;
            let lbl = match &self.current_fn {
                Some(fn_name) => {
                    let l = format!("{LOCAL_PREFIX}{fn_name}_{name}");
                    self.locals.insert(name.clone(), l.clone());
                    self.emit_data_decl(&l, 0);
                    l
                }
                None => {
                    let l = format!("{GLOBAL_PREFIX}{name}");
                    self.globals.insert(name.clone(), l.clone());
                    self.emit_data_decl(&l, 0);
                    l
                }
            };

            if let Some(init) = &init_decl.node.initializer {
                match &init.node {
                    Initializer::Expression(expr) => {
                        self.compile_expr(expr)?;
                        self.emit(&format!("STRAL {lbl}"));
                    }
                    _ => return Err(CompileError::Unsupported("complex initializer".into())),
                }
            }
        }
        Ok(())
    }

    // ── functions ─────────────────────────────────────────────────────────

    fn compile_function(&mut self, fdef: &FunctionDefinition) -> Result<(), CompileError> {
        let name   = extract_decl_name(&fdef.declarator)?;
        let params = extract_params(&fdef.declarator);
        // statement is accessed via fdef.statement below

        self.functions.insert(name.clone(), params.len());

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_fn     = self.current_fn.replace(name.clone());

        let fn_lbl  = format!("{FN_PREFIX}{name}");
        let ret_lbl = format!("{FN_PREFIX}{name}_ret");

        // Function entry: RJP writes the return address into this DATA slot.
        self.emit(&format!(">{fn_lbl} DATA 0"));

        // Parameter slots.
        for (i, pname) in params.iter().enumerate() {
            let pslot = format!("{PARAM_PREFIX}{name}_{i}");
            self.locals.insert(pname.clone(), pslot.clone());
            self.emit_data_decl(&pslot, 0);
        }

        // Body.
        self.compile_stmt(&fdef.statement)?;

        // Canonical return point.
        self.set_label(ret_lbl);
        self.emit(&format!("IJP {fn_lbl}"));

        self.locals     = saved_locals;
        self.current_fn = saved_fn;
        Ok(())
    }

    // ── top-level ─────────────────────────────────────────────────────────

    pub fn compile_unit(&mut self, unit: &TranslationUnit) -> Result<(), CompileError> {
        // Pass 1: collect all function signatures (enables forward calls).
        for ext in &unit.0 {
            if let ExternalDeclaration::FunctionDefinition(fdef) = &ext.node {
                let name = extract_decl_name(&fdef.node.declarator)?;
                let params = extract_params(&fdef.node.declarator);
                self.functions.insert(name, params.len());
            }
        }

        // Pass 2: register global variable names (no code yet).
        for ext in &unit.0 {
            if let ExternalDeclaration::Declaration(decl) = &ext.node {
                for init_decl in &decl.node.declarators {
                    let name = extract_decl_name(&init_decl.node.declarator)?;
                    let l = format!("{GLOBAL_PREFIX}{name}");
                    self.globals.insert(name, l);
                }
            }
        }

        // Pass 3: emit global variable DATA words.
        for (_name, lbl) in self.globals.clone() {
            self.emit_data_decl(&lbl, 0);
        }

        // Pass 4: compile function bodies.
        for ext in &unit.0 {
            if let ExternalDeclaration::FunctionDefinition(fdef) = &ext.node {
                self.compile_function(&fdef.node)?;
            }
        }

        Ok(())
    }

    // ── final output ─────────────────────────────────────────────────────

    pub fn finish(mut self) -> String {
        if let Some(lbl) = self.pending_label.take() {
            self.code.push(format!(">{lbl} DATA 0"));
        }

        let mut out = String::new();
        out.push_str(&format!("ORG &O{:06o}\n", PROG_ORG));
        out.push_str(&format!("SADD {START_LABEL}\n\n"));

        // Runtime entry point.
        out.push_str(&format!(">{START_LABEL} ENTALK {STACK_INIT}\n"));
        out.push_str(&format!("STRAL {SP_LABEL}\n"));
        out.push_str(&format!("RJP {FN_PREFIX}main\n"));
        out.push_str("STOP 1\n\n");

        // I/O buffers (used by built-in putchar/getchar).
        out.push_str(">__outwd DATA 0\n");
        out.push_str(">__inwd DATA 0\n");

        // Runtime constants.
        out.push_str(&format!(">{ZERO_LABEL} DATA 0\n"));
        out.push_str(&format!(">{SP_LABEL} DATA 0\n\n"));

        // User code.
        for line in &self.code {
            out.push_str(line);
            out.push('\n');
        }

        // Static data (globals, locals, temps, constants).
        out.push('\n');
        for line in &self.data {
            out.push_str(line);
            out.push('\n');
        }

        out.push_str("\nEND\n");
        out
    }
}

// ── AST helpers ───────────────────────────────────────────────────────────────

fn parse_constant(c: &Constant) -> Result<i64, CompileError> {
    match c {
        Constant::Integer(i) => {
            // lang-c stores the digit string WITHOUT any prefix (0x, 0, 0b) in
            // i.number, and the base separately in i.base.
            let s: &str = &i.number;
            let radix = match i.base {
                IntegerBase::Decimal     => 10u32,
                IntegerBase::Octal       => 8,
                IntegerBase::Hexadecimal => 16,
                IntegerBase::Binary      => 2,
            };
            i64::from_str_radix(s, radix)
                .map_err(|e| CompileError::Parse(format!("bad integer literal: {e}")))
        }
        Constant::Character(ch) => {
            // Strip surrounding single quotes.
            let inner = ch.trim_matches('\'');
            if inner.starts_with('\\') {
                let v = match inner {
                    "\\n"  => 10i64,
                    "\\r"  => 13,
                    "\\t"  => 9,
                    "\\0"  => 0,
                    "\\\\" => 92,
                    "\\'"  => 39,
                    "\\\"" => 34,
                    _ => return Err(CompileError::Unsupported(
                        format!("escape sequence {inner}")
                    )),
                };
                Ok(v)
            } else {
                Ok(inner.chars().next().unwrap_or('\0') as i64)
            }
        }
        _ => Err(CompileError::Unsupported("floating-point constant".into())),
    }
}

fn extract_decl_name(decl: &Node<Declarator>) -> Result<String, CompileError> {
    match &decl.node.kind.node {
        DeclaratorKind::Identifier(id) => Ok(id.node.name.clone()),
        DeclaratorKind::Declarator(inner) => extract_decl_name(inner),
        _ => Err(CompileError::Unsupported("abstract declarator".into())),
    }
}

fn extract_params(decl: &Node<Declarator>) -> Vec<String> {
    for derived in &decl.node.derived {
        if let DerivedDeclarator::Function(fdecl) = &derived.node {
            let mut params = Vec::new();
            for param in &fdecl.node.parameters {
                if let Some(pd) = &param.node.declarator {
                    if let Ok(name) = extract_decl_name(pd) {
                        params.push(name);
                    }
                }
            }
            return params;
        }
    }
    Vec::new()
}
