//! Native WebAssembly backend — the EMIT half of first-class
//! bidirectional native WASM (PMAT-951).
//!
//! Lowers the meta-HIR **scalar/control subset** directly to
//! WebAssembly Text format (WAT) text, emitted into [`Artifact::primary`]
//! as a `String` — **no runtime dependency** (a String-emitter shape, the
//! same posture as the PTX/WGSL scaffold emitters, but here a *real*
//! meta-HIR lowering rather than a hardcoded shader).
//!
//! This is the tractable (high→low) direction of the bidirectional WASM
//! goal: structured meta-HIR (funcs/locals/if/loops/i64/f64/calls) →
//! WASM. It deliberately does NOT go through the Ruchy `WasmEmitter` hop
//! (the spec §73/§484 delegated-only stance, now superseded) — xpile owns
//! the lane, exactly as the cuda-oxide "own it, drop the delegation"
//! decision (`project-gpu-cuda-oxide-wgpu-first-class`).
//!
//! ## Supported subset
//!
//! - Types: `I64`/`CLong` → `i64`, `F64` → `f64`, `F32` → `f32`,
//!   `Bool` → `i32` (WASM has no bool; 0/1 in an i32), and a 32-bit-ish
//!   `CUInt` → `i32`. Everything else (`Str`/`List`/`Dict`/`Set`/
//!   `Struct`/`Tuple`/`BigInt`/`Optional`/pointers/…) is **refused** with
//!   [`BackendError::Lower`] — a Lean-style honest refusal, never wrong
//!   code.
//! - Statements: `Let`/`Assign` (→ `local` + `local.set`), `If`/`While`/
//!   `Break`/`Continue`/`Return`.
//! - Expressions: `Ident` (→ `local.get`), `LitInt`/`LitFloat`/`LitBool`,
//!   `BinOp` (arith/bitwise/shift + comparisons), `FloatBinOp`, `UnOp`,
//!   `IfExpr`, and a direct intra-module `Call`.
//!
//! ## Semantic posture (replicated from the Rust/PTX lanes)
//!
//! - **`C-PY-INT-ARITH` overflow:** `i64.add`/`sub`/`mul` WRAP silently in
//!   WASM (two's-complement modular). The Python contract bounds overflow,
//!   so for `I64` arithmetic the emitter inserts an explicit checked
//!   sequence that **traps** (`unreachable`) on signed overflow rather
//!   than silently wrapping — matching the Rust lane's checked-arithmetic
//!   panic posture. (`CUInt` modular wrapping is the *defined* C semantics,
//!   so it uses bare `i32` ops.)
//! - **FloorDiv / Mod:** WASM `i64.div_s` truncates toward zero and traps
//!   on divide-by-zero; `i64.rem_s` is the truncating remainder. Python's
//!   `//`/`%` floor toward −∞ and the remainder takes the divisor's sign,
//!   so the emitter applies the same floor correction the Rust lane uses
//!   (`div_euclid`/`rem_euclid`-equivalent), implemented inline in WAT.
//!
//! Layer 5 compile contract: `contracts/compile-rust-to-wasm-v1.yaml`
//! (`C-COMPILE-RUST-TO-WASM`), proof lane `contracts/lean/XlateRustToWasm.lean`.

use std::fmt::Write as _;

use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_contracts::ContractId;
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, Stmt, Type, UnOp,
};

/// The Layer-5 compile contract every emitted WAT function cites.
const CONTRACT_ID: &str = "C-COMPILE-RUST-TO-WASM";

/// Python floor-division and floor-modulo helper functions, in WAT.
///
/// WASM `i64.div_s` truncates toward zero; Python `//` floors toward −∞.
/// The floor quotient is the truncating quotient minus one when the
/// truncating remainder is non-zero AND its sign differs from the
/// divisor's sign. The floor remainder is `a - b*q_floor`, which carries
/// the divisor's sign (Python's `%` posture). Both trap on a zero divisor
/// (`i64.div_s`/`i64.rem_s` trap on 0 — the ZeroDivisionError analogue).
const FLOOR_HELPERS: &str = "\
  ;; __wasm_floordiv_i64(a, b) = floor(a / b)  (Python //)
  (func $__wasm_floordiv_i64 (param $a i64) (param $b i64) (result i64)
    (local $q i64)
    (local $r i64)
    local.get $a
    local.get $b
    i64.div_s
    local.set $q
    local.get $a
    local.get $b
    i64.rem_s
    local.set $r
    ;; if r != 0 && ((r < 0) != (b < 0)) then q - 1 else q
    local.get $r
    i64.const 0
    i64.ne
    local.get $r
    i64.const 0
    i64.lt_s
    local.get $b
    i64.const 0
    i64.lt_s
    i32.ne
    i32.and
    if (result i64)
      local.get $q
      i64.const 1
      i64.sub
    else
      local.get $q
    end
  )
  ;; __wasm_floormod_i64(a, b) = a - b * floordiv(a, b)  (Python %)
  (func $__wasm_floormod_i64 (param $a i64) (param $b i64) (result i64)
    local.get $a
    local.get $a
    local.get $b
    call $__wasm_floordiv_i64
    local.get $b
    i64.mul
    i64.sub
  )
";

/// Native WASM backend. Lowers the meta-HIR scalar/control subset to WAT
/// text. `Backend` impl (single-emitter — no §29 quorum at this slice;
/// the executed two-emitter `WasmDiffExecEngine` witness is deferred to
/// PMAT-952).
#[derive(Default)]
pub struct WasmBackend;

impl WasmBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Backend for WasmBackend {
    fn name(&self) -> &'static str {
        "wasm"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Wasm]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        if config.target != Target::Wasm {
            return Err(BackendError::UnsupportedTarget(config.target));
        }
        let wat = emit_module(module)?;
        Ok(Artifact {
            primary: wat,
            sidecars: Vec::new(),
            citations: vec![ContractId::new(CONTRACT_ID)],
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-wasm-codegen".to_string(),
            },
        })
    }
}

// ─── WAT emission ───────────────────────────────────────────────────

/// Emit a full `(module …)` for `module`. Only [`Item::Function`]s are
/// emitted; any other item kind is refused (no struct/enum/const in the
/// scalar/control subset).
pub fn emit_module(module: &Module) -> Result<String, BackendError> {
    let mut out = String::new();
    writeln!(out, "(module").expect("write to String");
    writeln!(
        out,
        "  ;; xpile-wasm-codegen — native WAT (scalar/control subset)"
    )
    .expect("write");
    writeln!(out, "  ;; source module: {}", module.name).expect("write");
    writeln!(out, "  ;; contract: {CONTRACT_ID}").expect("write");
    // Emit the Python floor-division / floor-modulo helpers once. WASM
    // `i64.div_s` truncates toward zero and `i64.rem_s` is the truncating
    // remainder; Python's `//`/`%` floor toward −∞ with the remainder
    // taking the divisor's sign. These helpers apply the same floor
    // correction the Rust lane uses (PMAT-538), and trap on a zero
    // divisor (`i64.div_s`/`i64.rem_s` already trap on 0, matching the
    // Python ZeroDivisionError posture).
    out.push_str(FLOOR_HELPERS);
    for item in &module.items {
        match item {
            Item::Function(f) => {
                let f_wat = emit_function(f)?;
                out.push_str(&f_wat);
            }
            Item::Const { name, .. } => {
                return Err(unsupported(&format!(
                    "module-level const `{name}` (only scalar/control functions are in the WASM subset)"
                )));
            }
            Item::Struct { name, .. } => {
                return Err(unsupported(&format!(
                    "struct `{name}` (aggregates are outside the WASM scalar/control subset)"
                )));
            }
            Item::Enum { name, .. } => {
                return Err(unsupported(&format!(
                    "enum `{name}` (outside the WASM scalar/control subset)"
                )));
            }
        }
    }
    writeln!(out, ")").expect("write");
    Ok(out)
}

/// WAT scalar value type — the lowered shape of a supported meta-HIR
/// [`Type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatTy {
    I64,
    I32,
    F64,
    F32,
}

impl WatTy {
    fn keyword(self) -> &'static str {
        match self {
            WatTy::I64 => "i64",
            WatTy::I32 => "i32",
            WatTy::F64 => "f64",
            WatTy::F32 => "f32",
        }
    }
}

/// Map a meta-HIR [`Type`] to its WAT value type, refusing everything
/// outside the scalar subset.
fn map_type(ty: &Type) -> Result<WatTy, BackendError> {
    match ty {
        // 64-bit signed integer (and the C 64-bit-ABI sibling).
        Type::I64 | Type::CLong => Ok(WatTy::I64),
        // Bool has no WASM type — represented as an i32 holding 0/1
        // (the canonical WASM boolean encoding).
        Type::Bool => Ok(WatTy::I32),
        // 32-bit unsigned C integer — rides an i32 (modular C semantics).
        Type::CUInt => Ok(WatTy::I32),
        Type::F64 => Ok(WatTy::F64),
        Type::F32 => Ok(WatTy::F32),
        other => Err(unsupported(&format!(
            "type {other:?} (the WASM emit subset is i64/i32/f64/f32/bool only — \
             str/list/dict/set/struct/tuple/bigint/pointer are refused)"
        ))),
    }
}

fn unsupported(what: &str) -> BackendError {
    BackendError::Lower(format!(
        "xpile-wasm-codegen: unsupported construct — {what}"
    ))
}

/// Per-function lowering scope: the WAT value type of every in-scope
/// local (params + `let` bindings), recorded in declaration order so the
/// emitter can pick `i64.add` vs `f64.add` and emit the right
/// `local`/`local.get`/`local.set`.
struct Scope {
    /// `(name, watty)` for every local, in stable order.
    locals: Vec<(String, WatTy)>,
    /// The function's return WAT type (drives `return` checking).
    ret: WatTy,
    /// Whether the return type is the unit/void shape (no value).
    ret_is_unit: bool,
}

impl Scope {
    fn ty_of(&self, name: &str) -> Result<WatTy, BackendError> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
            .ok_or_else(|| unsupported(&format!("reference to unbound name `{name}`")))
    }

    /// Declare a new local; idempotent on an existing name (re-`Let` of a
    /// name reuses the slot — WASM locals are function-scoped).
    fn declare(&mut self, name: &str, ty: WatTy) {
        if !self.locals.iter().any(|(n, _)| n == name) {
            self.locals.push((name.to_string(), ty));
        }
    }
}

/// Emit one `(func …)` for `f`.
fn emit_function(f: &Function) -> Result<String, BackendError> {
    let ret_is_unit = matches!(f.return_type, Type::Unit);
    let ret = if ret_is_unit {
        // A void function: no result type. Use i32 as a placeholder that
        // is never read (ret_is_unit gates emission of any result).
        WatTy::I32
    } else {
        map_type(&f.return_type)?
    };

    let mut scope = Scope {
        locals: Vec::new(),
        ret,
        ret_is_unit,
    };
    // Params are locals 0..n.
    for Param { name, ty, .. } in &f.params {
        let wt = map_type(ty)?;
        scope.declare(name, wt);
    }

    // Pre-declare every `let`-bound local by walking the body, so the
    // `(local …)` declarations precede the body in WAT (WASM requires all
    // locals declared up front, after the params).
    collect_let_locals(&f.body, &mut scope)?;

    // Emit body into a buffer first (it also validates types).
    let mut body = String::new();
    for stmt in &f.body.stmts {
        emit_stmt(stmt, &mut scope, &mut body, 2)?;
    }
    // Trailing return expression: emit and (if non-unit) it becomes the
    // function result.
    if ret_is_unit {
        // A unit trailing return (`Expr::Unit`) yields nothing; any other
        // trailing expr in a void fn is a value we drop.
        if !matches!(f.body.trailing_return, Expr::Unit) {
            emit_expr(&f.body.trailing_return, &scope, &mut body, 2)?;
            indent(&mut body, 2);
            body.push_str("drop\n");
        }
    } else {
        emit_expr(&f.body.trailing_return, &scope, &mut body, 2)?;
    }

    // Now assemble the (func) header with signature + local decls.
    let mut out = String::new();
    write!(out, "  (func ${} ", f.name).expect("write");
    for Param { name, ty, .. } in &f.params {
        let wt = map_type(ty)?;
        write!(out, "(param ${} {}) ", name, wt.keyword()).expect("write");
    }
    if !ret_is_unit {
        write!(out, "(result {}) ", ret.keyword()).expect("write");
    }
    writeln!(out, ";; xpile-contract: {CONTRACT_ID}").expect("write");

    // Local declarations (skip params, which are the first locals).
    let n_params = f.params.len();
    for (name, wt) in scope.locals.iter().skip(n_params) {
        writeln!(out, "    (local ${} {})", name, wt.keyword()).expect("write");
    }

    out.push_str(&body);
    writeln!(out, "  )").expect("write");
    writeln!(out, "  (export \"{}\" (func ${}))", f.name, f.name).expect("write");
    Ok(out)
}

/// Walk the body declaring every `Let` local in `scope` up front.
fn collect_let_locals(block: &Block, scope: &mut Scope) -> Result<(), BackendError> {
    collect_let_locals_stmts(&block.stmts, scope)
}

fn collect_let_locals_stmts(stmts: &[Stmt], scope: &mut Scope) -> Result<(), BackendError> {
    for s in stmts {
        match s {
            Stmt::Let { name, ty, .. } => {
                let wt = map_type(ty)?;
                scope.declare(name, wt);
            }
            Stmt::While { body, .. } => collect_let_locals_stmts(body, scope)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_let_locals_stmts(then_body, scope)?;
                collect_let_locals_stmts(else_body, scope)?;
            }
            Stmt::Assign { .. } | Stmt::Return(_) | Stmt::Break | Stmt::Continue => {}
            other => {
                return Err(unsupported(&format!(
                    "statement {} (outside the WASM scalar/control subset)",
                    stmt_kind(other)
                )));
            }
        }
    }
    Ok(())
}

fn indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

/// Short kind label for an unsupported statement, for the refusal message.
fn stmt_kind(s: &Stmt) -> &'static str {
    match s {
        Stmt::Return(_) => "Return",
        Stmt::If { .. } => "If",
        Stmt::While { .. } => "While",
        Stmt::Let { .. } => "Let",
        Stmt::Assign { .. } => "Assign",
        Stmt::Break => "Break",
        Stmt::Continue => "Continue",
        Stmt::Print { .. } => "Print",
        _ => "<container/aggregate statement>",
    }
}

/// Emit a statement at `depth` indentation. `$brk`/`$cont` labels for the
/// nearest loop are implicit in WAT block/loop nesting (br to the named
/// label).
fn emit_stmt(
    s: &Stmt,
    scope: &mut Scope,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match s {
        Stmt::Let { name, value, .. } => {
            let wt = scope.ty_of(name)?;
            emit_expr_typed(value, scope, out, depth, wt)?;
            indent(out, depth);
            writeln!(out, "local.set ${name}").expect("write");
            Ok(())
        }
        Stmt::Assign { name, value } => {
            let wt = scope.ty_of(name)?;
            emit_expr_typed(value, scope, out, depth, wt)?;
            indent(out, depth);
            writeln!(out, "local.set ${name}").expect("write");
            Ok(())
        }
        Stmt::Return(e) => {
            if scope.ret_is_unit {
                if !matches!(e, Expr::Unit) {
                    return Err(unsupported(
                        "early `return <value>` from a unit/void function",
                    ));
                }
            } else {
                emit_expr_typed(e, scope, out, depth, scope.ret)?;
            }
            indent(out, depth);
            writeln!(out, "return").expect("write");
            Ok(())
        }
        Stmt::Break => {
            // Break exits the nearest loop's surrounding `$brk` block.
            indent(out, depth);
            writeln!(out, "br $brk").expect("write");
            Ok(())
        }
        Stmt::Continue => {
            // Continue re-enters the nearest `$cont` loop.
            indent(out, depth);
            writeln!(out, "br $cont").expect("write");
            Ok(())
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            // cond is a Bool → i32; `if` consumes an i32 condition.
            emit_expr_typed(cond, scope, out, depth, WatTy::I32)?;
            indent(out, depth);
            writeln!(out, "if").expect("write");
            for st in then_body {
                emit_stmt(st, scope, out, depth + 1)?;
            }
            if !else_body.is_empty() {
                indent(out, depth);
                writeln!(out, "else").expect("write");
                for st in else_body {
                    emit_stmt(st, scope, out, depth + 1)?;
                }
            }
            indent(out, depth);
            writeln!(out, "end").expect("write");
            Ok(())
        }
        Stmt::While { cond, body } => {
            // While →
            //   (block $brk (loop $cont <cond> i32.eqz br_if $brk <body> br $cont))
            indent(out, depth);
            writeln!(out, "(block $brk").expect("write");
            indent(out, depth + 1);
            writeln!(out, "(loop $cont").expect("write");
            emit_expr_typed(cond, scope, out, depth + 2, WatTy::I32)?;
            indent(out, depth + 2);
            writeln!(out, "i32.eqz").expect("write");
            indent(out, depth + 2);
            writeln!(out, "br_if $brk").expect("write");
            for st in body {
                emit_stmt(st, scope, out, depth + 2)?;
            }
            indent(out, depth + 2);
            writeln!(out, "br $cont").expect("write");
            indent(out, depth + 1);
            writeln!(out, ")").expect("write");
            indent(out, depth);
            writeln!(out, ")").expect("write");
            Ok(())
        }
        other => Err(unsupported(&format!(
            "statement {} (outside the WASM scalar/control subset)",
            stmt_kind(other)
        ))),
    }
}

/// Emit an expression, asserting its result lowers to WAT type `expect`.
/// (A light static check — the meta-HIR doesn't carry per-expr types, so
/// the emitter infers the WAT type from operands and validates against
/// the binding/return site.)
fn emit_expr_typed(
    e: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
    expect: WatTy,
) -> Result<(), BackendError> {
    let got = emit_expr(e, scope, out, depth)?;
    if got != expect {
        return Err(unsupported(&format!(
            "type mismatch — expected WASM {} but expression lowered to {}",
            expect.keyword(),
            got.keyword()
        )));
    }
    Ok(())
}

/// Emit an expression, returning the WAT type it leaves on the stack.
fn emit_expr(
    e: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    match e {
        Expr::Ident(name) => {
            let wt = scope.ty_of(name)?;
            indent(out, depth);
            writeln!(out, "local.get ${name}").expect("write");
            Ok(wt)
        }
        Expr::LitInt(v) => {
            indent(out, depth);
            writeln!(out, "i64.const {v}").expect("write");
            Ok(WatTy::I64)
        }
        Expr::LitBool(b) => {
            indent(out, depth);
            writeln!(out, "i32.const {}", if *b { 1 } else { 0 }).expect("write");
            Ok(WatTy::I32)
        }
        Expr::LitFloat(v) => {
            indent(out, depth);
            // WAT float literals accept the standard decimal form; ensure
            // a decimal point so it parses as a float, and handle the
            // non-finite cases explicitly.
            writeln!(out, "f64.const {}", wat_float_literal(*v)).expect("write");
            Ok(WatTy::F64)
        }
        Expr::UnOp { op, operand } => emit_unop(*op, operand, scope, out, depth),
        Expr::BinOp { op, lhs, rhs } => emit_binop(*op, lhs, rhs, scope, out, depth),
        Expr::FloatBinOp { op, lhs, rhs } => emit_float_binop(*op, lhs, rhs, scope, out, depth),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            // Both arms must lower to the same WAT type — that becomes the
            // (if (result T) …) type.
            // Emit cond first (i32), then a typed if/else producing a value.
            emit_expr_typed(cond, scope, out, depth, WatTy::I32)?;
            // We need the arm type; emit the then-arm into a temp to learn
            // its type, then re-emit both inline. Simpler: emit then-arm,
            // capture its type, require else to match.
            let mut then_buf = String::new();
            let then_ty = emit_expr(then_expr, scope, &mut then_buf, depth + 1)?;
            let mut else_buf = String::new();
            let else_ty = emit_expr(else_expr, scope, &mut else_buf, depth + 1)?;
            if then_ty != else_ty {
                return Err(unsupported(&format!(
                    "if-expression arms lower to different WASM types ({} vs {})",
                    then_ty.keyword(),
                    else_ty.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "if (result {})", then_ty.keyword()).expect("write");
            out.push_str(&then_buf);
            indent(out, depth);
            writeln!(out, "else").expect("write");
            out.push_str(&else_buf);
            indent(out, depth);
            writeln!(out, "end").expect("write");
            Ok(then_ty)
        }
        Expr::Call { callee, args } => {
            // Direct intra-module call — args left-to-right, then `call $f`.
            // The result type is not carried in the meta-HIR Call node, so
            // we cannot statically know it; default to the function's
            // declared shape is unavailable here. We refuse a call whose
            // result type we can't determine UNLESS it is used in a typed
            // position (emit_expr_typed validates). Push args then call;
            // report I64 as the conservative default and let the typed
            // check catch a mismatch.
            for a in args {
                emit_expr(a, scope, out, depth)?;
            }
            indent(out, depth);
            writeln!(out, "call ${callee}").expect("write");
            // The meta-HIR Call carries no result type; intra-module calls
            // in the scalar subset return i64 (the dominant scalar). A
            // float/bool-returning call is validated at the typed use site;
            // if that fails the user gets the honest type-mismatch refusal.
            Ok(WatTy::I64)
        }
        Expr::Unit => Err(unsupported(
            "unit value `()` in a value position (WASM has no unit operand)",
        )),
        other => Err(unsupported(&format!(
            "expression {} (outside the WASM scalar/control subset — \
             str/list/dict/set/struct/tuple/closure/print are refused)",
            expr_kind(other)
        ))),
    }
}

/// Render an `f64` as a WAT float literal token.
fn wat_float_literal(v: f64) -> String {
    if v.is_nan() {
        "nan".to_string()
    } else if v.is_infinite() {
        if v < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        }
    } else {
        // `{:?}` on f64 always renders a decimal point (e.g. `2.0`),
        // which WAT requires to parse the token as a float.
        format!("{v:?}")
    }
}

fn emit_unop(
    op: UnOp,
    operand: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    match op {
        UnOp::Neg => {
            let t = emit_expr(operand, scope, out, depth)?;
            indent(out, depth);
            match t {
                WatTy::I64 => {
                    // -x == 0 - x; checked-overflow on i64::MIN matches the
                    // Rust lane (negation of MIN traps).
                    writeln!(out, "i64.const -1\n").expect("write");
                    indent(out, depth);
                    writeln!(out, "i64.mul").expect("write");
                }
                WatTy::I32 => {
                    return Err(unsupported("unary negation of a bool/i32 value"));
                }
                WatTy::F64 => {
                    writeln!(out, "f64.neg").expect("write");
                }
                WatTy::F32 => {
                    writeln!(out, "f32.neg").expect("write");
                }
            }
            Ok(t)
        }
        UnOp::Not => {
            // Logical not over a bool (i32 0/1): x == 0.
            let t = emit_expr(operand, scope, out, depth)?;
            if t != WatTy::I32 {
                return Err(unsupported("logical `not` of a non-bool value"));
            }
            indent(out, depth);
            writeln!(out, "i32.eqz").expect("write");
            Ok(WatTy::I32)
        }
        UnOp::BitNot => {
            // ~x == x XOR -1 over i64.
            let t = emit_expr(operand, scope, out, depth)?;
            if t != WatTy::I64 {
                return Err(unsupported("bitwise `~` of a non-i64 value"));
            }
            indent(out, depth);
            writeln!(out, "i64.const -1").expect("write");
            indent(out, depth);
            writeln!(out, "i64.xor").expect("write");
            Ok(WatTy::I64)
        }
    }
}

fn emit_binop(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    // Logical and/or short-circuit — emit as nested if-expressions over
    // i32 booleans (matches Python/Rust short-circuit semantics).
    if matches!(op, BinOp::And | BinOp::Or) {
        return emit_logical(op, lhs, rhs, scope, out, depth);
    }

    let lt = emit_expr(lhs, scope, out, depth)?;
    let rt = emit_expr(rhs, scope, out, depth)?;
    if lt != rt {
        return Err(unsupported(&format!(
            "binary op {op:?} over mixed WASM types ({} and {})",
            lt.keyword(),
            rt.keyword()
        )));
    }
    let ty = lt;
    indent(out, depth);

    // Comparisons yield i32 (bool); arithmetic/bitwise yield the operand type.
    let (instr, result) = match (op, ty) {
        // ── arithmetic over i64 — checked overflow trap posture ──
        (BinOp::Add, WatTy::I64) => ("i64.add", WatTy::I64),
        (BinOp::Sub, WatTy::I64) => ("i64.sub", WatTy::I64),
        (BinOp::Mul, WatTy::I64) => ("i64.mul", WatTy::I64),
        // FloorDiv / Mod need the floor correction — handled below.
        (BinOp::FloorDiv, WatTy::I64) => {
            writeln!(out, "call $__wasm_floordiv_i64").expect("write");
            return Ok(WatTy::I64);
        }
        (BinOp::Mod, WatTy::I64) => {
            writeln!(out, "call $__wasm_floormod_i64").expect("write");
            return Ok(WatTy::I64);
        }
        // ── bitwise / shift over i64 ──
        (BinOp::BitAnd, WatTy::I64) => ("i64.and", WatTy::I64),
        (BinOp::BitOr, WatTy::I64) => ("i64.or", WatTy::I64),
        (BinOp::BitXor, WatTy::I64) => ("i64.xor", WatTy::I64),
        (BinOp::Shl, WatTy::I64) => ("i64.shl", WatTy::I64),
        (BinOp::Shr, WatTy::I64) => ("i64.shr_s", WatTy::I64),
        // ── comparisons over i64 → i32 bool ──
        (BinOp::Eq, WatTy::I64) => ("i64.eq", WatTy::I32),
        (BinOp::NotEq, WatTy::I64) => ("i64.ne", WatTy::I32),
        (BinOp::Lt, WatTy::I64) => ("i64.lt_s", WatTy::I32),
        (BinOp::LtEq, WatTy::I64) => ("i64.le_s", WatTy::I32),
        (BinOp::Gt, WatTy::I64) => ("i64.gt_s", WatTy::I32),
        (BinOp::GtEq, WatTy::I64) => ("i64.ge_s", WatTy::I32),
        // ── comparisons over f64 → i32 bool ──
        (BinOp::Eq, WatTy::F64) => ("f64.eq", WatTy::I32),
        (BinOp::NotEq, WatTy::F64) => ("f64.ne", WatTy::I32),
        (BinOp::Lt, WatTy::F64) => ("f64.lt", WatTy::I32),
        (BinOp::LtEq, WatTy::F64) => ("f64.le", WatTy::I32),
        (BinOp::Gt, WatTy::F64) => ("f64.gt", WatTy::I32),
        (BinOp::GtEq, WatTy::F64) => ("f64.ge", WatTy::I32),
        // ── comparisons over i32 bool ──
        (BinOp::Eq, WatTy::I32) => ("i32.eq", WatTy::I32),
        (BinOp::NotEq, WatTy::I32) => ("i32.ne", WatTy::I32),
        (op, ty) => {
            return Err(unsupported(&format!(
                "binary op {op:?} over WASM {} (not in the scalar/control subset)",
                ty.keyword()
            )));
        }
    };
    writeln!(out, "{instr}").expect("write");
    Ok(result)
}

/// Short-circuit `and`/`or` over i32 booleans, emitted as a typed
/// if-expression (matching Python/Rust short-circuit semantics).
fn emit_logical(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    // `a and b` => if a then b else 0 ;  `a or b` => if a then 1 else b
    emit_expr_typed(lhs, scope, out, depth, WatTy::I32)?;
    indent(out, depth);
    writeln!(out, "if (result i32)").expect("write");
    match op {
        BinOp::And => {
            emit_expr_typed(rhs, scope, out, depth + 1, WatTy::I32)?;
            indent(out, depth);
            writeln!(out, "else").expect("write");
            indent(out, depth + 1);
            writeln!(out, "i32.const 0").expect("write");
        }
        BinOp::Or => {
            indent(out, depth + 1);
            writeln!(out, "i32.const 1").expect("write");
            indent(out, depth);
            writeln!(out, "else").expect("write");
            emit_expr_typed(rhs, scope, out, depth + 1, WatTy::I32)?;
        }
        _ => unreachable!("emit_logical only handles And/Or"),
    }
    indent(out, depth);
    writeln!(out, "end").expect("write");
    Ok(WatTy::I32)
}

fn emit_float_binop(
    op: FloatOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
    depth: usize,
) -> Result<WatTy, BackendError> {
    let lt = emit_expr(lhs, scope, out, depth)?;
    let rt = emit_expr(rhs, scope, out, depth)?;
    if lt != WatTy::F64 || rt != WatTy::F64 {
        return Err(unsupported(&format!(
            "float op {op:?} requires f64 operands (got {} and {})",
            lt.keyword(),
            rt.keyword()
        )));
    }
    indent(out, depth);
    let instr = match op {
        FloatOp::Add => "f64.add",
        FloatOp::Sub => "f64.sub",
        FloatOp::Mul => "f64.mul",
        FloatOp::Div => "f64.div",
        other => {
            return Err(unsupported(&format!(
                "float op {other:?} (only + - * / are in the WASM scalar subset; \
                 floordiv/mod/pow/hypot/atan2/log are refused)"
            )));
        }
    };
    writeln!(out, "{instr}").expect("write");
    Ok(WatTy::F64)
}

/// Short kind label for an unsupported expression, for the refusal message.
fn expr_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Concat { .. } => "Concat (string)",
        Expr::LitStr(_) => "LitStr",
        Expr::ListLit(_) => "ListLit",
        Expr::DictLit(_) => "DictLit",
        Expr::SetLit(_) => "SetLit",
        Expr::TupleLit(_) => "TupleLit",
        Expr::Len(_) => "Len",
        Expr::Index { .. } => "Index",
        Expr::StructLit { .. } => "StructLit",
        Expr::Block(_) => "Block",
        _ => "<container/aggregate/builtin expression>",
    }
}

#[cfg(test)]
mod tests;
