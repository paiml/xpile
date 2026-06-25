//! PMAT-961 — the REAL meta-HIR → PTX text emitter (scalar element-wise
//! subset), the NVIDIA sibling of `xpile-wasm-codegen`'s hand-emitted WAT.
//!
//! Lowers a meta-HIR [`Function`] of the **element-wise kernel shape** — one
//! scalar input parameter, a scalar return, and a body of scalar arithmetic
//! over that parameter — into a complete, `ptxas`-assemblable PTX module:
//!
//! ```ptx
//! .version 8.0
//! .target  sm_<cc>          ; derived from HwProfile::Ptx, never hard-coded
//! .address_size 64
//! .visible .entry xpile_kernel(.param .u64 in, .param .u64 out, .param .u32 n)
//! { ... ld.global.f64 ; <arith> ; st.global.f64 ... ret; }
//! ```
//!
//! The emitted kernel computes `out[i] = f(in[i])` for the per-element scalar
//! function `f` the meta-HIR body expresses, with a real thread-index guard
//! (`mad.lo.s32` of `%ctaid.x * %ntid.x + %tid.x`, `setp.ge` against `n`,
//! `@p bra`). This is a genuine expression-tree → PTX register-allocated
//! lowering, NOT a hardcoded shader and NOT a comment placeholder — it is the
//! categorical PTX twin of the nvcc-compiled CUDA-C `xpile_kernel`, which is
//! exactly what the §29 anti-correlation quorum (PMAT-961's `PtxDiffExecEngine`)
//! needs.
//!
//! ## Supported subset (honest, Lean-style)
//!
//! - **Element type**: `F64` (`.f64`), `F32` (`.f32`), or `I64`/`CLong`
//!   (`.s64`). The kernel reads one element of that type, computes, writes
//!   one element of the *return* type (same width family).
//! - **Signature**: exactly ONE scalar parameter (the per-element input);
//!   the return type is scalar. (Multi-input kernels, host scalars, and
//!   reductions are out of subset.)
//! - **Body expression**: the single trailing return expression (an optional
//!   leading run of `let x = <expr>;` bindings is folded in). Operators:
//!   `+ - * /` over floats (`add/sub/mul/div.rn.f64|f32`), `+ - *` over i64
//!   (`add/sub/mul.s64`), unary negation, the parameter reference, `let`
//!   locals, and float/int literals.
//! - **Refused** (hard [`BackendError::Lower`], never wrong PTX): any
//!   aggregate type (str/list/dict/set/struct/tuple/bigint/optional/pointer),
//!   control flow in the body (`if`/`while`/`break`/early `return`),
//!   comparisons / booleans, calls, mixed-width arithmetic, integer division
//!   (Python floor semantics aren't a single PTX op), and any signature that
//!   isn't the one-scalar-in element-wise shape.

use std::fmt::Write as _;

use xpile_backend::BackendError;
use xpile_meta_hir::{Expr, FloatOp, Function, Stmt, Type, UnOp};

/// PTX ISA version emitted. 8.0 is supported by CUDA 11.8+ and assembles for
/// every `.target` in the contract's sm_80..sm_120 range (sm_89/sm_90 require
/// ISA ≥ 7.8; 8.0 is the safe common floor with headroom). Pure text — the
/// real `ptxas` on the box validates it (see [`crate::PtxDiffExecEngine`]).
pub const PTX_VERSION: &str = "8.0";

/// The kernel entry-point name. Bit-identical to the nvcc CUDA-C
/// `xpile_kernel` (the PMAT-949 path) so the anti-correlation quorum loads the
/// same symbol from both toolchains.
pub const KERNEL_NAME: &str = "xpile_kernel";

/// The PTX scalar register class an element lowers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PtxScalar {
    /// IEEE-754 double → `.f64` registers (`%fd`).
    F64,
    /// IEEE-754 single → `.f32` registers (`%f`).
    F32,
    /// 64-bit signed integer → `.s64` registers (`%rd` reused as `.b64`).
    S64,
}

impl PtxScalar {
    /// Element width in bytes (drives the `mul.wide.s32` index stride and the
    /// `.u32`/`.u64` load/store widths).
    fn bytes(self) -> u32 {
        match self {
            PtxScalar::F64 | PtxScalar::S64 => 8,
            PtxScalar::F32 => 4,
        }
    }

    /// `ld.global`/`st.global` type suffix.
    fn ldst_ty(self) -> &'static str {
        match self {
            PtxScalar::F64 => "f64",
            PtxScalar::F32 => "f32",
            PtxScalar::S64 => "s64",
        }
    }

    /// Register-declaration class keyword (`.f64`/`.f32`/`.s64`).
    fn reg_class(self) -> &'static str {
        match self {
            PtxScalar::F64 => ".f64",
            PtxScalar::F32 => ".f32",
            PtxScalar::S64 => ".s64",
        }
    }

    /// Register name prefix (`%fd`/`%f`/`%rv`).
    fn reg_prefix(self) -> &'static str {
        match self {
            PtxScalar::F64 => "%fd",
            PtxScalar::F32 => "%f",
            PtxScalar::S64 => "%rv",
        }
    }
}

fn map_scalar(ty: &Type) -> Result<PtxScalar, BackendError> {
    match ty {
        Type::F64 => Ok(PtxScalar::F64),
        Type::F32 => Ok(PtxScalar::F32),
        Type::I64 | Type::CLong => Ok(PtxScalar::S64),
        other => Err(refuse(&format!(
            "element type {other:?} (the PTX emit subset is f64/f32/i64 scalars only — \
             str/list/dict/set/struct/tuple/bigint/bool/pointer are refused)"
        ))),
    }
}

fn refuse(what: &str) -> BackendError {
    BackendError::Lower(format!("xpile-ptx-codegen: unsupported construct — {what}"))
}

/// IEEE-754 f64 → the PTX hex-double immediate form `0d<16 hex>` (PTX requires
/// floating immediates in their exact bit pattern; decimal isn't accepted in
/// instruction operands).
fn f64_imm(v: f64) -> String {
    format!("0d{:016X}", v.to_bits())
}

/// IEEE-754 f32 → the PTX hex-float immediate form `0f<8 hex>`.
fn f32_imm(v: f32) -> String {
    format!("0f{:08X}", v.to_bits())
}

/// Per-emission register allocator + body buffer for one scalar value class.
struct Emitter {
    scalar: PtxScalar,
    /// Next value-register index for the element scalar class.
    next_val: u32,
    /// `(name, register)` for in-scope `let` bindings + the input param.
    locals: Vec<(String, String)>,
    /// Emitted instruction lines for the compute region.
    body: String,
}

impl Emitter {
    fn new(scalar: PtxScalar) -> Self {
        Self {
            scalar,
            next_val: 1,
            locals: Vec::new(),
            body: String::new(),
        }
    }

    /// Allocate a fresh value register of the element class.
    fn fresh(&mut self) -> String {
        let r = format!("{}{}", self.scalar.reg_prefix(), self.next_val);
        self.next_val += 1;
        r
    }

    fn bind(&mut self, name: &str, reg: String) {
        self.locals.push((name.to_string(), reg));
    }

    fn lookup(&self, name: &str) -> Result<String, BackendError> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| refuse(&format!("reference to unbound name `{name}`")))
    }

    fn line(&mut self, instr: &str) {
        writeln!(self.body, "\t{instr}").expect("write to String");
    }

    /// Emit `e`, returning the register holding its value.
    fn emit_expr(&mut self, e: &Expr) -> Result<String, BackendError> {
        match e {
            Expr::Ident(name) => self.lookup(name),
            Expr::LitFloat(v) => {
                let dst = self.fresh();
                match self.scalar {
                    PtxScalar::F64 => self.line(&format!("mov.f64 \t{dst}, {};", f64_imm(*v))),
                    PtxScalar::F32 => {
                        self.line(&format!("mov.f32 \t{dst}, {};", f32_imm(*v as f32)))
                    }
                    PtxScalar::S64 => {
                        return Err(refuse(
                            "float literal in an integer-typed kernel (no implicit int↔float)",
                        ))
                    }
                }
                Ok(dst)
            }
            Expr::LitInt(v) => {
                let dst = self.fresh();
                match self.scalar {
                    PtxScalar::S64 => self.line(&format!("mov.s64 \t{dst}, {v};")),
                    PtxScalar::F64 => {
                        self.line(&format!("mov.f64 \t{dst}, {};", f64_imm(*v as f64)))
                    }
                    PtxScalar::F32 => {
                        self.line(&format!("mov.f32 \t{dst}, {};", f32_imm(*v as f32)))
                    }
                }
                Ok(dst)
            }
            Expr::UnOp {
                op: UnOp::Neg,
                operand,
            } => {
                let a = self.emit_expr(operand)?;
                let dst = self.fresh();
                let instr = match self.scalar {
                    PtxScalar::F64 => "neg.f64",
                    PtxScalar::F32 => "neg.f32",
                    PtxScalar::S64 => "neg.s64",
                };
                self.line(&format!("{instr} \t{dst}, {a};"));
                Ok(dst)
            }
            Expr::FloatBinOp { op, lhs, rhs } => {
                if self.scalar == PtxScalar::S64 {
                    return Err(refuse(
                        "float arithmetic in an integer-typed kernel (mixed width)",
                    ));
                }
                let a = self.emit_expr(lhs)?;
                let b = self.emit_expr(rhs)?;
                let dst = self.fresh();
                let ty = if self.scalar == PtxScalar::F64 {
                    "f64"
                } else {
                    "f32"
                };
                // `.rn` = round-to-nearest-even (IEEE default); matches what
                // nvcc emits for the corresponding CUDA-C operator.
                let instr = match op {
                    FloatOp::Add => format!("add.rn.{ty}"),
                    FloatOp::Sub => format!("sub.rn.{ty}"),
                    FloatOp::Mul => format!("mul.rn.{ty}"),
                    FloatOp::Div => format!("div.rn.{ty}"),
                    other => {
                        return Err(refuse(&format!(
                            "float op {other:?} (only + - * / are in the PTX scalar subset; \
                             floordiv/mod/pow/hypot/atan2/log are refused)"
                        )))
                    }
                };
                self.line(&format!("{instr} \t{dst}, {a}, {b};"));
                Ok(dst)
            }
            Expr::BinOp { op, lhs, rhs } => {
                use xpile_meta_hir::BinOp;
                if self.scalar != PtxScalar::S64 {
                    return Err(refuse(&format!(
                        "integer op {op:?} in a float-typed kernel (mixed width)"
                    )));
                }
                let a = self.emit_expr(lhs)?;
                let b = self.emit_expr(rhs)?;
                let dst = self.fresh();
                let instr = match op {
                    BinOp::Add => "add.s64",
                    BinOp::Sub => "sub.s64",
                    BinOp::Mul => "mul.lo.s64",
                    other => {
                        return Err(refuse(&format!(
                            "integer op {other:?} (only + - * over i64 are in the PTX scalar \
                             subset; division/mod/bitwise/shift/compare are refused — Python \
                             floor-div is not a single PTX op)"
                        )))
                    }
                };
                self.line(&format!("{instr} \t{dst}, {a}, {b};"));
                Ok(dst)
            }
            other => Err(refuse(&format!(
                "expression {other:?} (outside the PTX scalar element-wise subset — \
                 only param/let refs, literals, unary neg, and + - * / arithmetic are emitted)"
            ))),
        }
    }
}

/// Lower a meta-HIR [`Function`] of the element-wise kernel shape to a
/// complete PTX module string targeting `compute_capability` (e.g. `sm_89`).
///
/// `compute_capability` is threaded verbatim into the `.target` directive —
/// **derived from [`xpile_backend::HwProfile::Ptx`], never hard-coded**.
pub fn emit_kernel(f: &Function, compute_capability: &str) -> Result<String, BackendError> {
    // Signature must be the one-scalar-in element-wise shape.
    if f.params.len() != 1 {
        return Err(refuse(&format!(
            "kernel `{}` has {} parameters (the PTX element-wise subset emits exactly one \
             scalar input parameter)",
            f.name,
            f.params.len()
        )));
    }
    let in_scalar = map_scalar(&f.params[0].ty)?;
    let out_scalar = map_scalar(&f.return_type)?;
    if in_scalar != out_scalar {
        return Err(refuse(&format!(
            "kernel `{}` input/return widths differ ({in_scalar:?} vs {out_scalar:?}); the \
             element-wise subset keeps a single scalar class",
            f.name
        )));
    }

    // Body: an optional leading run of `let name = expr;` then the trailing
    // return expression. Any non-`let` statement (if/while/assign/early
    // return/print) is refused — control flow is out of subset.
    let mut em = Emitter::new(in_scalar);
    // The input parameter is the per-element value already loaded into the
    // first value register (filled by the prologue below).
    let in_reg = em.fresh();
    em.bind(&f.params[0].name, in_reg.clone());

    for stmt in &f.body.stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let r = em.emit_expr(value)?;
                em.bind(name, r);
            }
            other => {
                return Err(refuse(&format!(
                    "statement {other:?} in the kernel body (the PTX element-wise subset emits \
                     only `let` bindings + a trailing arithmetic return — no if/while/assign/\
                     early-return/print)"
                )))
            }
        }
    }
    let result_reg = em.emit_expr(&f.body.trailing_return)?;

    // ── assemble the full module ──────────────────────────────────────
    let scalar = in_scalar;
    let bytes = scalar.bytes();
    let ldst = scalar.ldst_ty();
    let reg_class = scalar.reg_class();
    let reg_prefix = scalar.reg_prefix();
    // How many value registers the compute region used (next_val − 1, +1 for
    // the `<N>` upper-bound convention PTX wants).
    let val_count = em.next_val;

    let mut out = String::new();
    writeln!(out, "//").expect("write");
    writeln!(
        out,
        "// Generated by xpile-ptx-codegen (hand-emitted, scalar element-wise subset)"
    )
    .expect("write");
    writeln!(out, "// source kernel: {}", f.name).expect("write");
    writeln!(out, "//").expect("write");
    writeln!(out, ".version {PTX_VERSION}").expect("write");
    writeln!(out, ".target {compute_capability}").expect("write");
    writeln!(out, ".address_size 64").expect("write");
    writeln!(out).expect("write");
    writeln!(out, ".visible .entry {KERNEL_NAME}(").expect("write");
    writeln!(out, "\t.param .u64 {KERNEL_NAME}_param_0,").expect("write");
    writeln!(out, "\t.param .u64 {KERNEL_NAME}_param_1,").expect("write");
    writeln!(out, "\t.param .u32 {KERNEL_NAME}_param_2").expect("write");
    writeln!(out, ")").expect("write");
    writeln!(out, "{{").expect("write");
    // Register declarations.
    writeln!(out, "\t.reg .pred \t%p<2>;").expect("write");
    writeln!(out, "\t.reg .b32 \t%r<6>;").expect("write");
    writeln!(out, "\t.reg {reg_class} \t{reg_prefix}<{val_count}>;").expect("write");
    writeln!(out, "\t.reg .b64 \t%rd<8>;").expect("write");
    writeln!(out).expect("write");
    // Prologue: params, thread index, bounds guard.
    writeln!(out, "\tld.param.u64 \t%rd1, [{KERNEL_NAME}_param_0];").expect("write");
    writeln!(out, "\tld.param.u64 \t%rd2, [{KERNEL_NAME}_param_1];").expect("write");
    writeln!(out, "\tld.param.u32 \t%r2, [{KERNEL_NAME}_param_2];").expect("write");
    writeln!(out, "\tmov.u32 \t%r3, %ctaid.x;").expect("write");
    writeln!(out, "\tmov.u32 \t%r4, %ntid.x;").expect("write");
    writeln!(out, "\tmov.u32 \t%r5, %tid.x;").expect("write");
    writeln!(out, "\tmad.lo.s32 \t%r1, %r3, %r4, %r5;").expect("write");
    writeln!(out, "\tsetp.ge.s32 \t%p1, %r1, %r2;").expect("write");
    writeln!(out, "\t@%p1 bra \t$L__BB0_2;").expect("write");
    writeln!(out).expect("write");
    // Load the input element into the first value register.
    writeln!(out, "\tcvta.to.global.u64 \t%rd3, %rd1;").expect("write");
    writeln!(out, "\tmul.wide.s32 \t%rd4, %r1, {bytes};").expect("write");
    writeln!(out, "\tadd.s64 \t%rd5, %rd3, %rd4;").expect("write");
    writeln!(out, "\tld.global.{ldst} \t{in_reg}, [%rd5];").expect("write");
    // Compute region (the lowered expression tree).
    out.push_str(&em.body);
    // Store the result element.
    writeln!(out, "\tcvta.to.global.u64 \t%rd6, %rd2;").expect("write");
    writeln!(out, "\tadd.s64 \t%rd7, %rd6, %rd4;").expect("write");
    writeln!(out, "\tst.global.{ldst} \t[%rd7], {result_reg};").expect("write");
    writeln!(out).expect("write");
    writeln!(out, "$L__BB0_2:").expect("write");
    writeln!(out, "\tret;").expect("write");
    writeln!(out, "}}").expect("write");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_meta_hir::{Block, Param};

    /// `def k(x: f64) -> f64: return x + x + 1.0` — the saxpy-like
    /// element-wise kernel (`2*x + 1` via reassociated doubling, mirroring the
    /// CUDA-C anti-correlation peer's general path semantics).
    fn saxpy_f64() -> Function {
        // (x + x) + 1.0
        let x_plus_x = Expr::FloatBinOp {
            op: FloatOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("x".into())),
        };
        let body_expr = Expr::FloatBinOp {
            op: FloatOp::Add,
            lhs: Box::new(x_plus_x),
            rhs: Box::new(Expr::LitFloat(1.0)),
        };
        Function {
            name: "saxpy".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::F64,
                mutable: false,
            }],
            return_type: Type::F64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: body_expr,
            },
        }
    }

    #[test]
    fn emits_real_ptx_directives_and_entry() {
        let ptx = emit_kernel(&saxpy_f64(), "sm_89").unwrap();
        assert!(ptx.contains(".version 8.0"));
        assert!(ptx.contains(".target sm_89"));
        assert!(ptx.contains(".address_size 64"));
        assert!(ptx.contains(".visible .entry xpile_kernel("));
        // Real load / compute / store, not a placeholder comment.
        assert!(ptx.contains("ld.global.f64"));
        assert!(ptx.contains("add.rn.f64"));
        assert!(ptx.contains("st.global.f64"));
        assert!(ptx.contains("ret;"));
    }

    #[test]
    fn target_is_derived_from_capability_not_hardcoded() {
        let a = emit_kernel(&saxpy_f64(), "sm_80").unwrap();
        let b = emit_kernel(&saxpy_f64(), "sm_90").unwrap();
        assert!(a.contains(".target sm_80"));
        assert!(b.contains(".target sm_90"));
    }

    #[test]
    fn f64_immediate_is_exact_bit_pattern() {
        // 1.0 → 0x3FF0000000000000.
        assert_eq!(f64_imm(1.0), "0d3FF0000000000000");
    }

    #[test]
    fn folds_let_bindings_into_compute() {
        // def k(x): t = x * 2.0; return t + 1.0
        let f = Function {
            name: "k".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::F64,
                mutable: false,
            }],
            return_type: Type::F64,
            body: Block {
                stmts: vec![Stmt::Let {
                    name: "t".into(),
                    ty: Type::F64,
                    value: Expr::FloatBinOp {
                        op: FloatOp::Mul,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::LitFloat(2.0)),
                    },
                    mutable: false,
                }],
                trailing_return: Expr::FloatBinOp {
                    op: FloatOp::Add,
                    lhs: Box::new(Expr::Ident("t".into())),
                    rhs: Box::new(Expr::LitFloat(1.0)),
                },
            },
        };
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("mul.rn.f64"));
        assert!(ptx.contains("add.rn.f64"));
    }

    #[test]
    fn refuses_aggregate_element_type() {
        let f = Function {
            name: "bad".into(),
            params: vec![Param {
                name: "s".into(),
                ty: Type::Str,
                mutable: false,
            }],
            return_type: Type::Str,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::Ident("s".into()),
            },
        };
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
        assert!(format!("{err}").contains("str/list/dict"));
    }

    #[test]
    fn refuses_control_flow_in_body() {
        let f = Function {
            name: "bad".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::F64,
                mutable: false,
            }],
            return_type: Type::F64,
            body: Block {
                stmts: vec![Stmt::Return(Expr::Ident("x".into()))],
                trailing_return: Expr::Ident("x".into()),
            },
        };
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("no if/while"));
    }

    #[test]
    fn refuses_multi_param_signature() {
        let f = Function {
            name: "two".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::F64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::F64,
                    mutable: false,
                },
            ],
            return_type: Type::F64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::Ident("a".into()),
            },
        };
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("exactly one scalar input"));
    }
}
