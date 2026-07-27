//! PMAT-961/962 — the REAL meta-HIR → PTX text emitter (scalar + control-flow
//! subset), the NVIDIA sibling of `xpile-wasm-codegen`'s hand-emitted WAT.
//!
//! Lowers a meta-HIR [`Function`] of the **element-wise kernel shape** — one
//! *or more* scalar input parameters, a scalar return, and a body of scalar
//! arithmetic, comparisons and control flow over those parameters — into a
//! complete, `ptxas`-assemblable PTX module:
//!
//! ```ptx
//! .version 8.0
//! .target  sm_<cc>          ; derived from HwProfile::Ptx, never hard-coded
//! .address_size 64
//! .visible .entry xpile_kernel(.param .u64 in0, …, .param .u64 out, .param .u32 n)
//! { ... ld.global.f64 ; <arith/branches> ; st.global.f64 ... ret; }
//! ```
//!
//! The emitted kernel computes `out[i] = f(in0[i], in1[i], …)` for the
//! per-element scalar function `f` the meta-HIR body expresses, with a real
//! thread-index guard (`mad.lo.s32` of `%ctaid.x * %ntid.x + %tid.x`,
//! `setp.ge` against `n`, `@p bra`). This is a genuine expression-tree → PTX
//! register-allocated lowering, NOT a hardcoded shader and NOT a comment
//! placeholder — it is the categorical PTX twin of the nvcc-compiled CUDA-C
//! `xpile_kernel`, which is exactly what the §29 anti-correlation quorum
//! (`PtxDiffExecEngine`) needs.
//!
//! ## Supported subset (honest, Lean-style)
//!
//! - **Element type**: `F64` (`.f64`), `F32` (`.f32`), or `I64`/`CLong`
//!   (`.s64`). All parameters AND the return share one scalar class (no
//!   implicit int↔float, no mixed width).
//! - **Signature** (PMAT-962): ONE OR MORE scalar parameters (each a
//!   per-element input array `in_k[i]`); the return type is the same scalar
//!   class. (Host scalars and reductions are still out of subset.)
//! - **Body** (PMAT-962): a sequence of statements — `let`/`let mut` bindings,
//!   `name = expr` reassignment, `if`/`else`, and `while` loops — followed by
//!   the trailing return expression.
//!   - **Expressions**: `+ - * /` over floats (`add/sub/mul/div.rn.f64|f32`),
//!     `+ - *` over i64 (`add/sub/mul.s64`), unary negation, parameter/`let`
//!     refs, and float/int literals.
//!   - **Conditions** (`if`/`while`): comparisons `< <= > >= == !=` over the
//!     element class (`setp.<cmp>.f64|f32|s64` → a `.pred` register), composed
//!     with `and`/`or` (predicate `and.pred`/`or.pred`). Control flow lowers to
//!     `@%p bra LABEL` / labels / a back-edge `bra` loop — the PTX analog of
//!     the WAT `(block (loop … br_if … br))`.
//!   - **Locals across branches**: a `let`/param register is *persistent
//!     storage* (the phi-via-shared-register idiom); `if`/`else`/`while` bodies
//!     `mov` into it, so its post-join value is well-defined for ptxas's SSA.
//! - **Refused** (hard [`BackendError::Lower`], never wrong PTX): any
//!   aggregate type (str/list/dict/set/struct/tuple/bigint/optional/pointer),
//!   `break`/`continue`/early `return`/`print`, a boolean as a *value* (only as
//!   an `if`/`while` condition), calls, mixed-width arithmetic, integer
//!   division (Python floor semantics aren't a single PTX op), and any
//!   signature that isn't the scalar element-wise shape.

use std::fmt::Write as _;

use xpile_backend::BackendError;
use xpile_meta_hir::{BinOp, Expr, FloatOp, Function, NumBuiltinOp, Stmt, Type, UnOp};

/// PTX ISA version FLOOR. 8.0 is supported by CUDA 11.8+ and assembles for
/// every `.target` in the contract's sm_80..sm_90 range (sm_89/sm_90 require
/// ISA ≥ 7.8; 8.0 is the safe common floor with headroom). Pure text — the
/// real `ptxas` on the box validates it (see [`crate::PtxDiffExecEngine`]).
///
/// NEWER architectures need a NEWER ISA: ptxas 13.0 rejects `.version 8.0`
/// for `.target sm_121` (Blackwell GB10) — the minimum ISA there is 8.8.
/// [`ptx_version_for`] derives the right `.version` from the compute
/// capability so the emitted module always assembles for its target.
pub const PTX_VERSION: &str = "8.0";

/// PTX ISA version that assembles for `compute_capability`'s `.target` —
/// **derived, never hard-coded** (the same honesty discipline as the
/// `.target` directive itself, PMAT-963).
///
/// The Blackwell family (`sm_100`+, e.g. the GB10's `sm_121`) requires PTX
/// ISA ≥ 8.8 — ptxas 13.0 hard-rejects the [`PTX_VERSION`] 8.0 floor for
/// those targets (verified on the gx10 fleet host). Every prior `.target`
/// in the contract's sm_80..sm_90 range assembles for the 8.0 floor, so the
/// floor is kept for them (no churn to the existing RTX 4090 / sm_89 witness).
///
/// The ARCHITECTURE-SPECIFIC spellings count too (PMAT-1406). `sm_120a` is a
/// real Blackwell target, not a typo: ptxas accepts the `sm_MNa` / `sm_MNf`
/// variant forms alongside the plain `sm_MN`. The original parse was
/// `strip_prefix("sm_").parse::<u32>()`, which fails on the trailing letter,
/// so `sm_100a` / `sm_120a` / `sm_121a` silently fell back to the 8.0 floor
/// and emitted a module ptxas hard-rejects — measured against ptxas 13.0:
/// `PTX .version 8.0 does not support .target sm_120a`. The suffix is
/// stripped before the family test so those land on 8.8 with their
/// non-suffixed twins. `sm_90a` (Hopper) stays on 8.0 and assembles, so the
/// `>= 100` boundary is unchanged.
///
/// A capability that is not a well-formed `.target` token never reaches here:
/// [`validate_compute_capability`] refuses it at the top of [`emit_kernel`].
pub fn ptx_version_for(compute_capability: &str) -> &'static str {
    let major_minor = compute_capability
        .strip_prefix("sm_")
        .or_else(|| compute_capability.strip_prefix("compute_"))
        // `sm_120a` / `sm_120f` — the arch-variant suffix is not part of the
        // family number.
        .map(|n| n.trim_end_matches(['a', 'f']))
        .and_then(|n| n.parse::<u32>().ok());
    match major_minor {
        // Blackwell (sm_100 / sm_120 / sm_121, …) needs ISA ≥ 8.8.
        Some(cc) if cc >= 100 => "8.8",
        _ => PTX_VERSION,
    }
}

/// PMAT-1406 — a compute capability that cannot be a well-formed PTX
/// `.target` operand.
///
/// [`emit_kernel`] threads the capability VERBATIM into the `.target`
/// directive, so an unchecked string is copied straight into the emitted
/// assembly. Before this gate, `--hardware ptx:bogus` exited 0 emitting
/// `.target bogus`, and `--hardware 'ptx:sm_80 ; rm'` emitted
/// `.target sm_80 ; rm` — a PTX *syntax error*. No assembler accepts either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidComputeCapability {
    /// The rejected capability, verbatim.
    pub got: String,
}

impl std::fmt::Display for InvalidComputeCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "compute capability `{}` is not a well-formed PTX `.target` \
             (expected `sm_<digits>`, `compute_<digits>`, optionally with an \
             architecture-variant suffix `a`/`f` — e.g. sm_80, sm_89, sm_121, \
             sm_120a, compute_90). It is threaded verbatim into the `.target` \
             directive, so an arbitrary string emits PTX no assembler accepts",
            self.got
        )
    }
}

impl std::error::Error for InvalidComputeCapability {}

/// PMAT-1406 — is `cap` a syntactically well-formed PTX `.target` operand?
///
/// Accepts `sm_<digits>` and `compute_<digits>`, each optionally carrying a
/// single architecture-variant suffix `a` or `f` (`sm_90a`, `sm_120a`,
/// `sm_100f`). Every one of those spellings was assembled by ptxas 13.0
/// during PMAT-1406 — this grammar is MEASURED, not guessed, which matters
/// because a naive digits-only check would refuse `sm_90a`, a REAL Hopper
/// architecture.
///
/// ** WHAT THIS DELIBERATELY DOES NOT DO — state it rather than imply full
/// validation. This checks SHAPE, not EXISTENCE. `sm_999` is shape-valid and
/// still passes; it emits syntactically valid PTX that ptxas rejects cleanly
/// with `Unsupported .target 'sm_999'`. Refusing it would require xpile to
/// carry an allow-list of every NVIDIA architecture, which goes stale on the
/// next generation — the exact rot PMAT-963 avoided by DERIVING `.version`
/// instead of hard-coding it. The division of labour is: xpile refuses what
/// can never be valid PTX syntax, `ptxas` is the architecture-existence
/// oracle. Nothing in this crate claims otherwise.
pub fn validate_compute_capability(cap: &str) -> Result<(), InvalidComputeCapability> {
    let invalid = || InvalidComputeCapability {
        got: cap.to_string(),
    };
    let digits = cap
        .strip_prefix("sm_")
        .or_else(|| cap.strip_prefix("compute_"))
        .ok_or_else(invalid)?;
    // At most one trailing architecture-variant letter.
    let digits = match digits.strip_suffix(['a', 'f']) {
        Some(rest) => rest,
        None => digits,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    Ok(())
}

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
    /// 64-bit signed integer → `.s64` registers (`%rv`).
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

    /// The `mov`/`neg`/`setp` etc. type suffix (`f64`/`f32`/`s64`).
    fn op_ty(self) -> &'static str {
        match self {
            PtxScalar::F64 => "f64",
            PtxScalar::F32 => "f32",
            PtxScalar::S64 => "s64",
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

/// The `setp.<cmp>` comparison mnemonic for a meta-HIR comparison [`BinOp`]
/// over the element scalar class. `None` for a non-comparison op.
fn setp_cmp(op: &BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Eq => "eq",
        BinOp::NotEq => "ne",
        BinOp::Lt => "lt",
        BinOp::LtEq => "le",
        BinOp::Gt => "gt",
        BinOp::GtEq => "ge",
        _ => return None,
    })
}

/// Per-emission register allocator + body buffer for one scalar value class.
///
/// PMAT-962: a *name* (param or `let`) maps to a single PERSISTENT storage
/// register so `Assign` / `if`-else / `while` can re-write it and ptxas sees a
/// well-defined post-join value (the phi-via-shared-register idiom). Fresh
/// temporaries (sub-expression results) get their own one-shot registers.
struct Emitter {
    scalar: PtxScalar,
    /// Next value-register index for the element scalar class.
    next_val: u32,
    /// Next predicate-register index (`%p<…>`; `%p1` is the bounds guard).
    next_pred: u32,
    /// Next branch-label index (`$L__BB0_<n>`; `$L__BB0_2` is the bounds-exit).
    next_label: u32,
    /// `(name, persistent-register)` for in-scope `let` bindings + params.
    locals: Vec<(String, String)>,
    /// Emitted instruction lines for the compute region.
    body: String,
    /// PMAT-980: `(list-param-name, cvta-to-global base register)` for each
    /// `list[scalar]` parameter the *array* kernel takes. An `Expr::Index`
    /// over such a name lowers to `ld.global` at `base + idx*elem_size`. Empty
    /// for the (default) scalar element-wise kernel.
    list_params: Vec<(String, String)>,
    /// PMAT-980: the next `%rd<n>` addressing register the array kernel may
    /// allocate for in-body `Index` offset arithmetic. The prologue reserves
    /// the low `%rd` range; the body draws from here upward so the two never
    /// collide.
    next_rd: u32,
    /// PMAT-980: the name bound to the per-thread index (`%r1`) in an array
    /// kernel — the only legal `Index` subscript. `None` for the scalar kernel.
    thread_index_name: Option<String>,
}

impl Emitter {
    fn new(scalar: PtxScalar) -> Self {
        Self {
            scalar,
            next_val: 1,
            // %p1 reserved for the prologue bounds guard.
            next_pred: 2,
            // $L__BB0_2 reserved for the prologue bounds-exit label.
            next_label: 3,
            locals: Vec::new(),
            body: String::new(),
            list_params: Vec::new(),
            next_rd: 0,
            thread_index_name: None,
        }
    }

    /// PMAT-980: allocate a fresh `%rd<n>` addressing register for in-body
    /// `Index` offset arithmetic (the array-kernel path seeds `next_rd` past
    /// the prologue's reserved low range).
    fn fresh_rd(&mut self) -> String {
        let r = format!("%rd{}", self.next_rd);
        self.next_rd += 1;
        r
    }

    /// Allocate a fresh value register of the element class.
    fn fresh(&mut self) -> String {
        let r = format!("{}{}", self.scalar.reg_prefix(), self.next_val);
        self.next_val += 1;
        r
    }

    /// Allocate a fresh predicate register (`%p<n>`).
    fn fresh_pred(&mut self) -> String {
        let r = format!("%p{}", self.next_pred);
        self.next_pred += 1;
        r
    }

    /// Allocate a fresh branch label (`$L__BB0_<n>`).
    fn fresh_label(&mut self) -> String {
        let l = format!("$L__BB0_{}", self.next_label);
        self.next_label += 1;
        l
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

    fn label_line(&mut self, label: &str) {
        writeln!(self.body, "{label}:").expect("write to String");
    }

    /// Emit `e`, returning a register holding its value (element class).
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
                // A comparison/logical in *value* position is refused (booleans
                // are only `if`/`while` conditions, never a kernel result). The
                // arithmetic i64 ops lower; everything else is refused.
                if setp_cmp(op).is_some() || matches!(op, BinOp::And | BinOp::Or) {
                    return Err(refuse(&format!(
                        "comparison/logical `{op:?}` in value position (booleans are only \
                         `if`/`while` conditions in the PTX subset, never a kernel result)"
                    )));
                }
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
                             subset; division/mod/bitwise/shift are refused — Python \
                             floor-div is not a single PTX op)"
                        )))
                    }
                };
                self.line(&format!("{instr} \t{dst}, {a}, {b};"));
                Ok(dst)
            }
            // PMAT-972: scalar numeric builtins that map to a SINGLE native PTX
            // op — `abs`, `min`, `max` (all three element classes) and
            // `math.sqrt` (float only). These are the staple element-wise GPU
            // primitives (relu = `max(x, 0)`, clamping via `min`/`max`, norms
            // via `sqrt`); ptxas has `abs.{f64,f32,s64}`, `min`/`max.{f64,f32,
            // s64}`, and `sqrt.rn.{f64,f32}` for them. The remaining
            // `NumBuiltinOp`s (sin/cos/exp/ln/floor/ceil/trunc/…) stay refused:
            // they need transcendental approximations or a float→int width
            // change (mixed width), neither of which is a single PTX op.
            Expr::NumBuiltin { op, args, .. } => self.emit_num_builtin(*op, args),
            // PMAT-980: a read-only indexed load `xs[i]` from a `list[scalar]`
            // parameter — the canonical GPU element-wise array shape (the PTX
            // analog of PMAT-966's WASM `*.load` and PMAT-970's WGSL array read).
            // `xs` must be a `list[scalar]` param (cvta'd to global in the
            // prologue) and the subscript must be the per-thread index name.
            Expr::Index { collection, index } => self.emit_index_load(collection, index),
            other => Err(refuse(&format!(
                "expression {other:?} (outside the PTX scalar element-wise subset — \
                 only param/let refs, literals, unary neg, + - * / arithmetic, the \
                 abs/min/max/sqrt scalar builtins, and a read-only `xs[i]` over a \
                 `list[scalar]` param are emitted)"
            ))),
        }
    }

    /// PMAT-980: lower `collection[index]` to an `ld.global.<ty>` of one
    /// element. The collection must name a `list[scalar]` parameter and the
    /// index must be the per-thread index identifier — the only subscript the
    /// array element-wise kernel defines (a literal / computed subscript has
    /// no thread mapping and is refused, never wrong PTX). The loaded element
    /// lands in a fresh element-class register, so an indexed load composes
    /// with all the existing scalar arithmetic / control flow.
    fn emit_index_load(&mut self, collection: &Expr, index: &Expr) -> Result<String, BackendError> {
        let list_name = match collection {
            Expr::Ident(n) => n,
            other => {
                return Err(refuse(&format!(
                    "indexed read over {other:?} (the PTX array subset indexes a `list[scalar]` \
                     *parameter* by name — `xs[i]`, not an arbitrary expression)"
                )))
            }
        };
        let base = self
            .list_params
            .iter()
            .find(|(n, _)| n == list_name)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| {
                refuse(&format!(
                    "indexed read `{list_name}[..]` over a non-list name (only a `list[scalar]` \
                     parameter is indexable in the PTX array element-wise subset)"
                ))
            })?;
        // The only legal subscript is the per-thread index name (`xs[i]` where
        // `i` is the thread index). A literal / arithmetic / other-name index
        // has no thread mapping in the element-wise kernel and is refused.
        match (index, &self.thread_index_name) {
            (Expr::Ident(idx), Some(ti)) if idx == ti => {}
            (other, _) => {
                return Err(refuse(&format!(
                    "indexed read `{list_name}[{other:?}]` with a non-thread-index subscript \
                     (the element-wise array kernel only defines `xs[i]` at the per-thread \
                     index; a literal / computed subscript is out of subset)"
                )))
            }
        }
        // `off = thread_idx (i32, %r1) * elem_bytes` (sign-extended to 64-bit),
        // `addr = base + off`, `ld.global.<ty>` into a fresh element register.
        let bytes = self.scalar.bytes();
        let off = self.fresh_rd();
        let addr = self.fresh_rd();
        self.line(&format!("mul.wide.s32 \t{off}, %r1, {bytes};"));
        self.line(&format!("add.s64 \t{addr}, {base}, {off};"));
        let dst = self.fresh();
        self.line(&format!(
            "ld.global.{} \t{dst}, [{addr}];",
            self.scalar.ldst_ty()
        ));
        Ok(dst)
    }

    /// PMAT-972: lower a [`NumBuiltinOp`] that has a single-instruction PTX
    /// form. `abs`/`sqrt` are unary; `min`/`max` are variadic (`>= 2` args) and
    /// fold pairwise over the tail (the PTX analog of the chained `.min`/`.max`
    /// Rust emit). Everything else is an honest refusal.
    fn emit_num_builtin(
        &mut self,
        op: NumBuiltinOp,
        args: &[Expr],
    ) -> Result<String, BackendError> {
        match op {
            NumBuiltinOp::Abs => {
                let [a] = args else {
                    return Err(refuse(&format!(
                        "abs() takes exactly one argument in the PTX subset (got {})",
                        args.len()
                    )));
                };
                let src = self.emit_expr(a)?;
                let dst = self.fresh();
                self.line(&format!("abs.{} \t{dst}, {src};", self.scalar.op_ty()));
                Ok(dst)
            }
            NumBuiltinOp::Sqrt => {
                // math.sqrt is always float in Python; PTX integer sqrt is not a
                // single op, so refuse on an int-typed kernel (no implicit
                // int↔float in the subset).
                if self.scalar == PtxScalar::S64 {
                    return Err(refuse(
                        "math.sqrt in an integer-typed kernel (sqrt is float-only — \
                         no single integer-sqrt PTX op, and no implicit int↔float)",
                    ));
                }
                let [a] = args else {
                    return Err(refuse(&format!(
                        "sqrt() takes exactly one argument in the PTX subset (got {})",
                        args.len()
                    )));
                };
                let src = self.emit_expr(a)?;
                let dst = self.fresh();
                // `.rn` round-to-nearest-even is REQUIRED for `sqrt.f64`
                // (ptxas rejects a roundless f64 sqrt) and is the IEEE default
                // nvcc emits for f32 too.
                self.line(&format!("sqrt.rn.{} \t{dst}, {src};", self.scalar.op_ty()));
                Ok(dst)
            }
            NumBuiltinOp::Min | NumBuiltinOp::Max => {
                if args.len() < 2 {
                    return Err(refuse(&format!(
                        "{}() needs at least two scalar arguments in the PTX subset \
                         (1-arg min/max over a list is out of subset; got {})",
                        if matches!(op, NumBuiltinOp::Min) {
                            "min"
                        } else {
                            "max"
                        },
                        args.len()
                    )));
                }
                let mnem = if matches!(op, NumBuiltinOp::Min) {
                    "min"
                } else {
                    "max"
                };
                let ty = self.scalar.op_ty();
                // Fold pairwise over the tail: acc = op(acc, arg_k).
                let mut acc = self.emit_expr(&args[0])?;
                for arg in &args[1..] {
                    let b = self.emit_expr(arg)?;
                    let dst = self.fresh();
                    self.line(&format!("{mnem}.{ty} \t{dst}, {acc}, {b};"));
                    acc = dst;
                }
                Ok(acc)
            }
            other => Err(refuse(&format!(
                "numeric builtin {other:?} (the PTX subset emits abs/min/max/sqrt — \
                 single native PTX ops; floor/ceil/trunc change float→int width and \
                 sin/cos/tan/exp/ln/log10/log2 need transcendental approximations, \
                 neither a single PTX op)"
            ))),
        }
    }

    /// PMAT-962: emit a boolean *condition* `cond`, returning the `.pred`
    /// register that holds its truth value. Only comparisons over the element
    /// class and `and`/`or` of conditions are accepted (a bare name / literal
    /// boolean is refused — the subset has no `Bool` element class).
    fn emit_cond(&mut self, cond: &Expr) -> Result<String, BackendError> {
        match cond {
            Expr::BinOp { op, lhs, rhs } if setp_cmp(op).is_some() => {
                let cmp = setp_cmp(op).expect("checked");
                let a = self.emit_expr(lhs)?;
                let b = self.emit_expr(rhs)?;
                let p = self.fresh_pred();
                // setp.<cmp>.<ty> %p, a, b — for floats, the unordered-aware
                // `setp.<cmp>` already matches IEEE compares nvcc emits.
                self.line(&format!(
                    "setp.{cmp}.{} \t{p}, {a}, {b};",
                    self.scalar.op_ty()
                ));
                Ok(p)
            }
            Expr::BinOp {
                op: op @ (BinOp::And | BinOp::Or),
                lhs,
                rhs,
            } => {
                // Non-short-circuit predicate composition (both sides are pure
                // scalar comparisons — no side effects, so eager `and.pred` /
                // `or.pred` is observationally identical to Python's `and`/`or`).
                let pa = self.emit_cond(lhs)?;
                let pb = self.emit_cond(rhs)?;
                let p = self.fresh_pred();
                let instr = if matches!(op, BinOp::And) {
                    "and.pred"
                } else {
                    "or.pred"
                };
                self.line(&format!("{instr} \t{p}, {pa}, {pb};"));
                Ok(p)
            }
            other => Err(refuse(&format!(
                "condition {other:?} (an `if`/`while` condition must be a comparison \
                 `< <= > >= == !=` over the element class, optionally composed with \
                 `and`/`or` — bare booleans / truthiness are refused)"
            ))),
        }
    }

    /// PMAT-962: emit `value` into the *existing persistent register* `dst`
    /// (the phi-via-shared-register idiom for `Assign` and reassigning `let`s).
    /// A `mov` from the freshly-computed temp keeps `dst` the single stable
    /// home of the name across branches.
    fn emit_into(&mut self, dst: &str, value: &Expr) -> Result<(), BackendError> {
        let src = self.emit_expr(value)?;
        if src != dst {
            self.line(&format!("mov.{} \t{dst}, {src};", self.scalar.op_ty()));
        }
        Ok(())
    }

    /// PMAT-962: emit one body statement.
    fn emit_stmt(&mut self, s: &Stmt) -> Result<(), BackendError> {
        match s {
            Stmt::Let { name, value, .. } => {
                // First binding of `name`: give it a persistent home register so
                // any later `Assign` / branch can rewrite the same register.
                let dst = self.fresh();
                self.emit_into(&dst, value)?;
                self.bind(name, dst);
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let dst = self.lookup(name)?;
                self.emit_into(&dst, value)
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                // if cond { then } else { else }:
                //   <cond → %p> ; @!%p bra ELSE ; <then> ; bra END ; ELSE: <else> ; END:
                let p = self.emit_cond(cond)?;
                let else_label = self.fresh_label();
                let end_label = self.fresh_label();
                // Skip the then-body when the predicate is false.
                self.line(&format!("@!{p} bra \t{else_label};"));
                for st in then_body {
                    self.emit_stmt(st)?;
                }
                self.line(&format!("bra \t{end_label};"));
                self.label_line(&else_label);
                for st in else_body {
                    self.emit_stmt(st)?;
                }
                self.label_line(&end_label);
                Ok(())
            }
            Stmt::While { cond, body } => {
                // while cond { body }:
                //   HEAD: <cond → %p> ; @!%p bra END ; <body> ; bra HEAD ; END:
                // The PTX analog of the WAT (block (loop <cond> i32.eqz br_if
                // $brk <body> br $cont)).
                let head_label = self.fresh_label();
                let end_label = self.fresh_label();
                self.label_line(&head_label);
                let p = self.emit_cond(cond)?;
                self.line(&format!("@!{p} bra \t{end_label};"));
                for st in body {
                    self.emit_stmt(st)?;
                }
                self.line(&format!("bra \t{head_label};"));
                self.label_line(&end_label);
                Ok(())
            }
            other => Err(refuse(&format!(
                "statement {other:?} in the kernel body (the PTX subset emits `let`/`mut` \
                 bindings, `name = expr` reassignment, `if`/`else`, and `while` — no \
                 break/continue/early-return/print)"
            ))),
        }
    }
}

/// PMAT-980: the element scalar class of a `list[scalar]` type, if `ty` is a
/// list of a supported scalar (the array kernel's input shape). `None` for a
/// non-list (the scalar element-wise kernel's input shape) — so a mixed
/// list/scalar parameter list routes to neither path and is refused.
fn list_elem_scalar(ty: &Type) -> Option<Result<PtxScalar, BackendError>> {
    match ty {
        Type::List(elem) => Some(map_scalar(elem)),
        _ => None,
    }
}

/// PMAT-980: find the single per-thread index identifier the body uses as a
/// subscript. Every `Expr::Index { index, .. }` must use the SAME bare
/// identifier (the thread index `i` in `xs[i]`, `ys[i]`, …) — a literal /
/// computed / inconsistent subscript is refused (no thread mapping). Returns
/// `Ok(None)` when the body indexes nothing.
fn discover_thread_index_name(f: &Function) -> Result<Option<String>, BackendError> {
    let mut found: Option<String> = None;
    let visit = |e: &Expr, found: &mut Option<String>| -> Result<(), BackendError> {
        if let Expr::Index { index, .. } = e {
            match index.as_ref() {
                Expr::Ident(name) => match found {
                    Some(prev) if prev != name => {
                        return Err(refuse(&format!(
                            "the array element-wise kernel indexes by two different subscripts \
                             (`{prev}` and `{name}`); all `xs[i]` reads must share ONE per-thread \
                             index"
                        )))
                    }
                    Some(_) => {}
                    None => *found = Some(name.clone()),
                },
                other => {
                    return Err(refuse(&format!(
                        "indexed read with a non-identifier subscript {other:?} (the array \
                         element-wise kernel only defines `xs[i]` at the per-thread index)"
                    )))
                }
            }
        }
        Ok(())
    };
    // Walk the trailing return + every statement's expressions.
    walk_exprs(&f.body.trailing_return, &mut |e| visit(e, &mut found))?;
    for s in &f.body.stmts {
        walk_stmt_exprs(s, &mut |e| visit(e, &mut found))?;
    }
    Ok(found)
}

/// PMAT-980: pre-order walk over every sub-expression of `e`, calling `f`.
fn walk_exprs(
    e: &Expr,
    f: &mut dyn FnMut(&Expr) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    f(e)?;
    match e {
        Expr::UnOp { operand, .. } => walk_exprs(operand, f)?,
        Expr::FloatBinOp { lhs, rhs, .. } | Expr::BinOp { lhs, rhs, .. } => {
            walk_exprs(lhs, f)?;
            walk_exprs(rhs, f)?;
        }
        Expr::Index { collection, index } => {
            walk_exprs(collection, f)?;
            walk_exprs(index, f)?;
        }
        Expr::NumBuiltin { args, .. } => {
            for a in args {
                walk_exprs(a, f)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// PMAT-980: walk every expression reachable from a statement (recursing into
/// `if`/`while` bodies).
fn walk_stmt_exprs(
    s: &Stmt,
    f: &mut dyn FnMut(&Expr) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    match s {
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => walk_exprs(value, f)?,
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            walk_exprs(cond, f)?;
            for st in then_body {
                walk_stmt_exprs(st, f)?;
            }
            for st in else_body {
                walk_stmt_exprs(st, f)?;
            }
        }
        Stmt::While { cond, body } => {
            walk_exprs(cond, f)?;
            for st in body {
                walk_stmt_exprs(st, f)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Lower a meta-HIR [`Function`] of the element-wise kernel shape to a
/// complete PTX module string targeting `compute_capability` (e.g. `sm_89`).
///
/// `compute_capability` is threaded verbatim into the `.target` directive —
/// **derived from [`xpile_backend::HwProfile::Ptx`], never hard-coded**.
///
/// PMAT-980: a kernel whose parameters are `list[scalar]` arrays routes to the
/// array element-wise path ([`emit_array_kernel`]), where each list rides a
/// `.u64` global base pointer and `xs[i]` (at the per-thread index) lowers to
/// an `ld.global`. A kernel of bare scalar params keeps the original implicit
/// element-wise lowering below.
pub fn emit_kernel(f: &Function, compute_capability: &str) -> Result<String, BackendError> {
    // PMAT-1406 — THE choke point. `compute_capability` is written verbatim
    // into `.target` by both this function and `emit_array_kernel` (which is
    // private and reached only via the delegation below), so refusing here
    // covers every real PTX emission in the crate. Validate BEFORE any
    // signature analysis: a malformed capability is malformed regardless of
    // what the kernel looks like.
    validate_compute_capability(compute_capability).map_err(|e| refuse(&e.to_string()))?;
    // Route by parameter shape. ALL-list → the explicit array kernel;
    // ALL-scalar → the implicit scalar kernel. A mix is refused (the two
    // calling conventions don't compose into one element-wise kernel).
    let any_list = f.params.iter().any(|p| matches!(p.ty, Type::List(_)));
    if any_list {
        return emit_array_kernel(f, compute_capability);
    }
    // Signature must be the scalar element-wise shape: one or more scalar
    // params, all the same scalar class as the (scalar) return.
    if f.params.is_empty() {
        return Err(refuse(&format!(
            "kernel `{}` has no parameters (the PTX element-wise subset emits at least one \
             scalar input parameter)",
            f.name
        )));
    }
    let out_scalar = map_scalar(&f.return_type)?;
    let mut in_scalars = Vec::with_capacity(f.params.len());
    for p in &f.params {
        let s = map_scalar(&p.ty)?;
        if s != out_scalar {
            return Err(refuse(&format!(
                "kernel `{}` parameter `{}` ({s:?}) differs from the return scalar class \
                 ({out_scalar:?}); the element-wise subset keeps a single scalar class",
                f.name, p.name
            )));
        }
        in_scalars.push(s);
    }
    let scalar = out_scalar;
    let n_params = f.params.len();

    let mut em = Emitter::new(scalar);
    // Each parameter's per-element value is loaded into its own persistent
    // register by the prologue below (filled in order: param 0 → first
    // register, param 1 → second, …).
    let mut in_regs = Vec::with_capacity(n_params);
    for p in &f.params {
        let r = em.fresh();
        em.bind(&p.name, r.clone());
        in_regs.push(r);
    }

    // Body: statements then the trailing return expression.
    for stmt in &f.body.stmts {
        em.emit_stmt(stmt)?;
    }
    let result_reg = em.emit_expr(&f.body.trailing_return)?;

    // ── assemble the full module ──────────────────────────────────────
    let bytes = scalar.bytes();
    let ldst = scalar.ldst_ty();
    let reg_class = scalar.reg_class();
    let reg_prefix = scalar.reg_prefix();
    // How many registers each class used (next_* − 1, +1 for the `<N>`
    // upper-bound convention PTX wants).
    let val_count = em.next_val;
    let pred_count = em.next_pred;

    let mut out = String::new();
    writeln!(out, "//").expect("write");
    writeln!(
        out,
        "// Generated by xpile-ptx-codegen (hand-emitted, scalar element-wise + control subset)"
    )
    .expect("write");
    writeln!(out, "// source kernel: {}", f.name).expect("write");
    writeln!(out, "//").expect("write");
    writeln!(out, ".version {}", ptx_version_for(compute_capability)).expect("write");
    writeln!(out, ".target {compute_capability}").expect("write");
    writeln!(out, ".address_size 64").expect("write");
    writeln!(out).expect("write");
    writeln!(out, ".visible .entry {KERNEL_NAME}(").expect("write");
    // n_params input pointers, then the output pointer, then the count.
    for k in 0..n_params {
        writeln!(out, "\t.param .u64 {KERNEL_NAME}_param_{k},").expect("write");
    }
    writeln!(out, "\t.param .u64 {KERNEL_NAME}_param_{n_params},").expect("write");
    writeln!(out, "\t.param .u32 {KERNEL_NAME}_param_{}", n_params + 1).expect("write");
    writeln!(out, ")").expect("write");
    writeln!(out, "{{").expect("write");
    // Register declarations. `%rd` addressing registers: 3 base (in/out/count
    // converted) + the shared index `%rd_idx`; size generously by param count.
    let rd_count = 4 + (n_params + 1) as u32 * 3;
    writeln!(out, "\t.reg .pred \t%p<{pred_count}>;").expect("write");
    writeln!(out, "\t.reg .b32 \t%r<6>;").expect("write");
    writeln!(out, "\t.reg {reg_class} \t{reg_prefix}<{val_count}>;").expect("write");
    writeln!(out, "\t.reg .b64 \t%rd<{rd_count}>;").expect("write");
    writeln!(out).expect("write");
    // Prologue: thread index + bounds guard. The count param is the LAST param.
    let count_param = n_params + 1;
    writeln!(
        out,
        "\tld.param.u32 \t%r2, [{KERNEL_NAME}_param_{count_param}];"
    )
    .expect("write");
    writeln!(out, "\tmov.u32 \t%r3, %ctaid.x;").expect("write");
    writeln!(out, "\tmov.u32 \t%r4, %ntid.x;").expect("write");
    writeln!(out, "\tmov.u32 \t%r5, %tid.x;").expect("write");
    writeln!(out, "\tmad.lo.s32 \t%r1, %r3, %r4, %r5;").expect("write");
    writeln!(out, "\tsetp.ge.s32 \t%p1, %r1, %r2;").expect("write");
    writeln!(out, "\t@%p1 bra \t$L__BB0_2;").expect("write");
    writeln!(out).expect("write");
    // Shared element-offset (`i * bytes`) in %rd1.
    writeln!(out, "\tmul.wide.s32 \t%rd1, %r1, {bytes};").expect("write");
    // Load each input element into its persistent register.
    // %rd addressing temporaries start at %rd2.
    let mut rd = 2u32;
    for (k, in_reg) in in_regs.iter().enumerate() {
        let base = rd;
        let glob = rd + 1;
        let addr = rd + 2;
        rd += 3;
        writeln!(
            out,
            "\tld.param.u64 \t%rd{base}, [{KERNEL_NAME}_param_{k}];"
        )
        .expect("write");
        writeln!(out, "\tcvta.to.global.u64 \t%rd{glob}, %rd{base};").expect("write");
        writeln!(out, "\tadd.s64 \t%rd{addr}, %rd{glob}, %rd1;").expect("write");
        writeln!(out, "\tld.global.{ldst} \t{in_reg}, [%rd{addr}];").expect("write");
    }
    writeln!(out).expect("write");
    // Compute region (the lowered statements + return).
    out.push_str(&em.body);
    // Store the result element to the output array (param index n_params).
    {
        let base = rd;
        let glob = rd + 1;
        let addr = rd + 2;
        writeln!(
            out,
            "\tld.param.u64 \t%rd{base}, [{KERNEL_NAME}_param_{n_params}];"
        )
        .expect("write");
        writeln!(out, "\tcvta.to.global.u64 \t%rd{glob}, %rd{base};").expect("write");
        writeln!(out, "\tadd.s64 \t%rd{addr}, %rd{glob}, %rd1;").expect("write");
        writeln!(out, "\tst.global.{ldst} \t[%rd{addr}], {result_reg};").expect("write");
    }
    writeln!(out).expect("write");
    writeln!(out, "$L__BB0_2:").expect("write");
    writeln!(out, "\tret;").expect("write");
    writeln!(out, "}}").expect("write");
    Ok(out)
}

/// PMAT-980 — the ARRAY element-wise kernel: every parameter is a
/// `list[scalar]` (an input array), the body reads `xs[i]` at the per-thread
/// index, and the result is stored to `out[i]`. The PTX analog of PMAT-966's
/// WASM `list[scalar]`-param-indexed-by-`xs[i]` and PMAT-970's WGSL array read.
///
/// Where [`emit_kernel`]'s scalar path loads each scalar param's element
/// IMPLICITLY in the prologue (the param *is* `in_k[i]`), this path keeps the
/// list as a `.u64` global base pointer and emits an `ld.global` per EXPLICIT
/// `xs[i]` in the body — so a kernel can read the same array more than once, or
/// not at all, and the indexed read composes with all the scalar arithmetic /
/// control flow / abs-min-max-sqrt builtins the scalar path already lowers.
///
/// Refused (hard [`BackendError`], never wrong PTX): a `list` of a non-scalar
/// element (`list[bool]`/`list[str]`/nested list — no natural `ld.global`
/// width), a list-element class differing from the return class, a subscript
/// other than the single per-thread index, and (still) a list literal / list
/// return / append / index-assignment.
fn emit_array_kernel(f: &Function, compute_capability: &str) -> Result<String, BackendError> {
    if f.params.is_empty() {
        return Err(refuse(&format!(
            "array kernel `{}` has no parameters (it emits at least one `list[scalar]` input)",
            f.name
        )));
    }
    // The return is a bare scalar (the per-element result); every param is a
    // `list` of THAT scalar class (a uniform-width element-wise kernel).
    let out_scalar = map_scalar(&f.return_type)?;
    for p in &f.params {
        match list_elem_scalar(&p.ty) {
            Some(res) => {
                let s = res?;
                if s != out_scalar {
                    return Err(refuse(&format!(
                        "array kernel `{}` parameter `{}` is `list[{s:?}]` but the return scalar \
                         class is {out_scalar:?}; the element-wise array subset keeps ONE scalar \
                         class across all inputs and the output",
                        f.name, p.name
                    )));
                }
            }
            None => {
                return Err(refuse(&format!(
                    "array kernel `{}` mixes a non-list parameter `{}` ({:?}) with list \
                     parameters; the array element-wise subset takes ALL `list[scalar]` inputs",
                    f.name, p.name, p.ty
                )))
            }
        }
    }
    let scalar = out_scalar;
    let n_params = f.params.len();

    // The single per-thread index name `xs[i]` reads through (`%r1`). A kernel
    // that indexes nothing still emits a valid (constant-per-thread) store.
    let thread_index_name = discover_thread_index_name(f)?;

    let mut em = Emitter::new(scalar);
    em.thread_index_name = thread_index_name;
    // Each list param's GLOBAL base pointer lives in a persistent `%rd` base
    // register, filled by the prologue below. The prologue uses `%rd0..` for
    // those bases; the body's `Index` offset arithmetic draws from `next_rd`
    // (seeded past them) so the two never collide.
    let mut base_regs = Vec::with_capacity(n_params);
    for (k, p) in f.params.iter().enumerate() {
        let base = format!("%rd{}", k);
        em.list_params.push((p.name.clone(), base.clone()));
        base_regs.push(base);
        let _ = k;
    }
    // Output base pointer + the shared store-offset register sit right after the
    // input bases; the body's transient `%rd`s start after THOSE.
    let out_base = format!("%rd{}", n_params);
    let store_off = format!("%rd{}", n_params + 1);
    let store_addr = format!("%rd{}", n_params + 2);
    em.next_rd = (n_params + 3) as u32;

    // Body: statements then the trailing return expression (the per-element f).
    for stmt in &f.body.stmts {
        em.emit_stmt(stmt)?;
    }
    let result_reg = em.emit_expr(&f.body.trailing_return)?;

    // ── assemble the full module ──────────────────────────────────────
    let bytes = scalar.bytes();
    let ldst = scalar.ldst_ty();
    let reg_class = scalar.reg_class();
    let reg_prefix = scalar.reg_prefix();
    let val_count = em.next_val.max(2);
    let pred_count = em.next_pred;
    // The body may have allocated transient `%rd`s up to `next_rd`; size the
    // `%rd` file to cover them (PTX wants the `<N>` upper bound).
    let rd_count = em.next_rd.max((n_params + 3) as u32) + 1;

    let mut out = String::new();
    writeln!(out, "//").expect("write");
    writeln!(
        out,
        "// Generated by xpile-ptx-codegen (hand-emitted, ARRAY element-wise subset, PMAT-980)"
    )
    .expect("write");
    writeln!(out, "// source kernel: {}", f.name).expect("write");
    writeln!(out, "//").expect("write");
    writeln!(out, ".version {}", ptx_version_for(compute_capability)).expect("write");
    writeln!(out, ".target {compute_capability}").expect("write");
    writeln!(out, ".address_size 64").expect("write");
    writeln!(out).expect("write");
    writeln!(out, ".visible .entry {KERNEL_NAME}(").expect("write");
    // n_params input array pointers, then the output pointer, then the count.
    for k in 0..n_params {
        writeln!(out, "\t.param .u64 {KERNEL_NAME}_param_{k},").expect("write");
    }
    writeln!(out, "\t.param .u64 {KERNEL_NAME}_param_{n_params},").expect("write");
    writeln!(out, "\t.param .u32 {KERNEL_NAME}_param_{}", n_params + 1).expect("write");
    writeln!(out, ")").expect("write");
    writeln!(out, "{{").expect("write");
    writeln!(out, "\t.reg .pred \t%p<{pred_count}>;").expect("write");
    writeln!(out, "\t.reg .b32 \t%r<6>;").expect("write");
    writeln!(out, "\t.reg {reg_class} \t{reg_prefix}<{val_count}>;").expect("write");
    writeln!(out, "\t.reg .b64 \t%rd<{rd_count}>;").expect("write");
    writeln!(out).expect("write");
    // Prologue: thread index + bounds guard (count is the LAST param).
    let count_param = n_params + 1;
    writeln!(
        out,
        "\tld.param.u32 \t%r2, [{KERNEL_NAME}_param_{count_param}];"
    )
    .expect("write");
    writeln!(out, "\tmov.u32 \t%r3, %ctaid.x;").expect("write");
    writeln!(out, "\tmov.u32 \t%r4, %ntid.x;").expect("write");
    writeln!(out, "\tmov.u32 \t%r5, %tid.x;").expect("write");
    writeln!(out, "\tmad.lo.s32 \t%r1, %r3, %r4, %r5;").expect("write");
    writeln!(out, "\tsetp.ge.s32 \t%p1, %r1, %r2;").expect("write");
    writeln!(out, "\t@%p1 bra \t$L__BB0_2;").expect("write");
    writeln!(out).expect("write");
    // Load each input array's GLOBAL base pointer into its persistent register
    // (NO element load here — the body's `xs[i]` does the per-element loads).
    for (k, base) in base_regs.iter().enumerate() {
        writeln!(out, "\tld.param.u64 \t{base}, [{KERNEL_NAME}_param_{k}];").expect("write");
        writeln!(out, "\tcvta.to.global.u64 \t{base}, {base};").expect("write");
    }
    writeln!(out).expect("write");
    // Compute region (the lowered statements + return, incl. `xs[i]` loads).
    out.push_str(&em.body);
    // Store the result element to the output array (param index n_params) at the
    // per-thread index: `out[i] = result`.
    writeln!(
        out,
        "\tld.param.u64 \t{out_base}, [{KERNEL_NAME}_param_{n_params}];"
    )
    .expect("write");
    writeln!(out, "\tcvta.to.global.u64 \t{out_base}, {out_base};").expect("write");
    writeln!(out, "\tmul.wide.s32 \t{store_off}, %r1, {bytes};").expect("write");
    writeln!(out, "\tadd.s64 \t{store_addr}, {out_base}, {store_off};").expect("write");
    writeln!(out, "\tst.global.{ldst} \t[{store_addr}], {result_reg};").expect("write");
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

    fn p(name: &str, ty: Type) -> Param {
        Param {
            name: name.into(),
            ty,
            mutable: false,
        }
    }

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
            params: vec![p("x", Type::F64)],
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

    /// PMAT-963 — the PTX `.version` is DERIVED from the compute capability,
    /// not pinned to the 8.0 floor: the contract's sm_80..sm_90 range keeps the
    /// 8.0 floor (no churn to the RTX 4090 / sm_89 witness), but Blackwell
    /// (`sm_100`+, e.g. the GB10's `sm_121`) bumps to 8.8 — ptxas 13.0
    /// hard-rejects 8.0 there (verified on the gx10 fleet host).
    #[test]
    fn ptx_version_is_derived_for_blackwell() {
        assert_eq!(ptx_version_for("sm_80"), "8.0");
        assert_eq!(ptx_version_for("sm_89"), "8.0");
        assert_eq!(ptx_version_for("sm_90"), "8.0");
        assert_eq!(ptx_version_for("sm_100"), "8.8");
        assert_eq!(ptx_version_for("sm_120"), "8.8");
        assert_eq!(ptx_version_for("sm_121"), "8.8");
        // a non-sm capability falls back to the floor (validate_ptx / ptxas
        // are the downstream oracles).
        assert_eq!(ptx_version_for("compute_90"), "8.0");
        // the emitted module reflects the derived version per target.
        assert!(emit_kernel(&saxpy_f64(), "sm_89")
            .unwrap()
            .contains(".version 8.0"));
        assert!(emit_kernel(&saxpy_f64(), "sm_121")
            .unwrap()
            .contains(".version 8.8"));
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
            params: vec![p("x", Type::F64)],
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

    /// PMAT-962 — multiple scalar parameters (`def k(a, b) -> f64: return
    /// a + b`) emit one input pointer per param and a load per param.
    #[test]
    fn emits_multi_param_kernel() {
        let f = Function {
            name: "addab".into(),
            params: vec![p("a", Type::F64), p("b", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::FloatBinOp {
                    op: FloatOp::Add,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        // Three input/output pointers + count: param_0, param_1 (inputs),
        // param_2 (out), param_3 (n).
        assert!(ptx.contains("xpile_kernel_param_0"));
        assert!(ptx.contains("xpile_kernel_param_1"));
        assert!(ptx.contains("xpile_kernel_param_2")); // output
        assert!(ptx.contains("xpile_kernel_param_3")); // count
                                                       // Two element loads (one per input).
        assert_eq!(ptx.matches("ld.global.f64").count(), 2);
        assert!(ptx.contains("add.rn.f64"));
        assert!(ptx.contains("st.global.f64"));
    }

    /// PMAT-962 — `if`/`else` with a comparison condition emits `setp` + a
    /// predicated branch + labels, and both branches write the shared local.
    #[test]
    fn emits_if_else_with_comparison() {
        // def relu(x: f64) -> f64:
        //     r = 0.0
        //     if x > 0.0: r = x
        //     else: r = 0.0
        //     return r
        let f = Function {
            name: "relu".into(),
            params: vec![p("x", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "r".into(),
                        ty: Type::F64,
                        value: Expr::LitFloat(0.0),
                        mutable: true,
                    },
                    Stmt::If {
                        cond: Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: Box::new(Expr::Ident("x".into())),
                            rhs: Box::new(Expr::LitFloat(0.0)),
                        },
                        then_body: vec![Stmt::Assign {
                            name: "r".into(),
                            value: Expr::Ident("x".into()),
                        }],
                        else_body: vec![Stmt::Assign {
                            name: "r".into(),
                            value: Expr::LitFloat(0.0),
                        }],
                    },
                ],
                trailing_return: Expr::Ident("r".into()),
            },
        };
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("setp.gt.f64"));
        assert!(ptx.contains("bra"));
        assert!(ptx.contains("@!%p")); // predicated skip
    }

    /// PMAT-962 — a `while` loop emits a head label, a guard `setp` + `@!%p
    /// bra`, and a back-edge `bra` to the head.
    #[test]
    fn emits_while_loop() {
        // def countdown(x: f64) -> f64:
        //     acc = x
        //     while acc > 1.0: acc = acc - 1.0
        //     return acc
        let f = Function {
            name: "cd".into(),
            params: vec![p("x", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "acc".into(),
                        ty: Type::F64,
                        value: Expr::Ident("x".into()),
                        mutable: true,
                    },
                    Stmt::While {
                        cond: Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: Box::new(Expr::Ident("acc".into())),
                            rhs: Box::new(Expr::LitFloat(1.0)),
                        },
                        body: vec![Stmt::Assign {
                            name: "acc".into(),
                            value: Expr::FloatBinOp {
                                op: FloatOp::Sub,
                                lhs: Box::new(Expr::Ident("acc".into())),
                                rhs: Box::new(Expr::LitFloat(1.0)),
                            },
                        }],
                    },
                ],
                trailing_return: Expr::Ident("acc".into()),
            },
        };
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("setp.gt.f64"));
        // Both forward (guard exit) and back-edge branches present.
        assert!(ptx.matches("bra").count() >= 2);
    }

    /// PMAT-962 — composed conditions (`and`/`or`) lower to `and.pred`/`or.pred`.
    #[test]
    fn emits_and_or_condition() {
        // if (x > 0.0) and (x < 10.0): r = x else: r = 0.0
        let f = Function {
            name: "clamp".into(),
            params: vec![p("x", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "r".into(),
                        ty: Type::F64,
                        value: Expr::LitFloat(0.0),
                        mutable: true,
                    },
                    Stmt::If {
                        cond: Expr::BinOp {
                            op: BinOp::And,
                            lhs: Box::new(Expr::BinOp {
                                op: BinOp::Gt,
                                lhs: Box::new(Expr::Ident("x".into())),
                                rhs: Box::new(Expr::LitFloat(0.0)),
                            }),
                            rhs: Box::new(Expr::BinOp {
                                op: BinOp::Lt,
                                lhs: Box::new(Expr::Ident("x".into())),
                                rhs: Box::new(Expr::LitFloat(10.0)),
                            }),
                        },
                        then_body: vec![Stmt::Assign {
                            name: "r".into(),
                            value: Expr::Ident("x".into()),
                        }],
                        else_body: vec![Stmt::Assign {
                            name: "r".into(),
                            value: Expr::LitFloat(0.0),
                        }],
                    },
                ],
                trailing_return: Expr::Ident("r".into()),
            },
        };
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("and.pred"));
        assert!(ptx.contains("setp.gt.f64"));
        assert!(ptx.contains("setp.lt.f64"));
    }

    #[test]
    fn refuses_aggregate_element_type() {
        let f = Function {
            name: "bad".into(),
            params: vec![p("s", Type::Str)],
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

    /// PMAT-962 — a comparison in *value* position (a boolean kernel result) is
    /// still refused — booleans are only conditions, never a returned value.
    #[test]
    fn refuses_comparison_in_value_position() {
        let f = Function {
            name: "bad".into(),
            params: vec![p("x", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::LitFloat(0.0)),
                },
            },
        };
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("value position"));
    }

    /// PMAT-962 — `break`/`continue`/early-return stay refused.
    #[test]
    fn refuses_break_in_body() {
        let f = Function {
            name: "bad".into(),
            params: vec![p("x", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: vec![Stmt::Break],
                trailing_return: Expr::Ident("x".into()),
            },
        };
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("break/continue"));
    }

    /// Build a single-statement-free kernel whose return is `body_expr`.
    fn kernel_returning(name: &str, params: Vec<Param>, ret: Type, body_expr: Expr) -> Function {
        Function {
            name: name.into(),
            params,
            return_type: ret,
            body: Block {
                stmts: Vec::new(),
                trailing_return: body_expr,
            },
        }
    }

    /// PMAT-972 — `abs(x)` over an f64 kernel lowers to the single `abs.f64`
    /// PTX op (was previously a hard refusal).
    #[test]
    fn emits_abs_f64() {
        let f = kernel_returning(
            "ab",
            vec![p("x", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Abs,
                args: vec![Expr::Ident("x".into())],
                of_float: true,
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("abs.f64"), "expected abs.f64 in:\n{ptx}");
    }

    /// PMAT-972 — `abs(x)` over an i64 kernel lowers to `abs.s64`.
    #[test]
    fn emits_abs_s64() {
        let f = kernel_returning(
            "ab",
            vec![p("x", Type::I64)],
            Type::I64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Abs,
                args: vec![Expr::Ident("x".into())],
                of_float: false,
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("abs.s64"), "expected abs.s64 in:\n{ptx}");
    }

    /// PMAT-972 — `math.sqrt(x)` lowers to `sqrt.rn.f64` (the `.rn` rounding
    /// modifier is required for an f64 sqrt).
    #[test]
    fn emits_sqrt_rn_f64() {
        let f = kernel_returning(
            "sq",
            vec![p("x", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Sqrt,
                args: vec![Expr::Ident("x".into())],
                of_float: true,
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(
            ptx.contains("sqrt.rn.f64"),
            "expected sqrt.rn.f64 in:\n{ptx}"
        );
    }

    /// PMAT-972 — `min(a, b)` lowers to `min.f64`; `max(a, b)` to `max.f64`
    /// (the relu/clamp staples).
    #[test]
    fn emits_min_max_f64() {
        let min_f = kernel_returning(
            "mn",
            vec![p("a", Type::F64), p("b", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Min,
                args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
                of_float: true,
            },
        );
        let max_f = kernel_returning(
            "mx",
            vec![p("a", Type::F64), p("b", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Max,
                args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
                of_float: true,
            },
        );
        assert!(emit_kernel(&min_f, "sm_89").unwrap().contains("min.f64"));
        assert!(emit_kernel(&max_f, "sm_89").unwrap().contains("max.f64"));
    }

    /// PMAT-972 — variadic `max(a, b, c)` folds pairwise into two `max.f64`s
    /// (the chained-`.max` analog).
    #[test]
    fn emits_variadic_max_folds_pairwise() {
        let f = kernel_returning(
            "mx3",
            vec![p("a", Type::F64), p("b", Type::F64), p("c", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Max,
                args: vec![
                    Expr::Ident("a".into()),
                    Expr::Ident("b".into()),
                    Expr::Ident("c".into()),
                ],
                of_float: true,
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        // Two inputs after the seed → two fold steps.
        assert_eq!(
            ptx.matches("max.f64").count(),
            2,
            "expected two max.f64 fold steps in:\n{ptx}"
        );
    }

    /// PMAT-972 — `min(s64, s64)` lowers to the integer `min.s64`.
    #[test]
    fn emits_min_s64() {
        let f = kernel_returning(
            "mns",
            vec![p("a", Type::I64), p("b", Type::I64)],
            Type::I64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Min,
                args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
                of_float: false,
            },
        );
        assert!(emit_kernel(&f, "sm_89").unwrap().contains("min.s64"));
    }

    /// PMAT-972 — `math.sqrt` over an integer kernel stays an honest refusal
    /// (no single integer-sqrt PTX op; no implicit int↔float).
    #[test]
    fn refuses_sqrt_on_int_kernel() {
        let f = kernel_returning(
            "bad",
            vec![p("x", Type::I64)],
            Type::I64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Sqrt,
                args: vec![Expr::Ident("x".into())],
                of_float: false,
            },
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("sqrt"));
    }

    /// PMAT-972 — a transcendental builtin (`math.sin`) stays refused (needs an
    /// approximation, not a single PTX op).
    #[test]
    fn refuses_transcendental_builtin() {
        let f = kernel_returning(
            "bad",
            vec![p("x", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Sin,
                args: vec![Expr::Ident("x".into())],
                of_float: true,
            },
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("transcendental"));
    }

    /// PMAT-972 — `min` with a single argument is refused (1-arg min/max over a
    /// list is out of subset).
    #[test]
    fn refuses_single_arg_min() {
        let f = kernel_returning(
            "bad",
            vec![p("x", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Min,
                args: vec![Expr::Ident("x".into())],
                of_float: true,
            },
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("at least two"));
    }

    #[test]
    fn refuses_empty_param_signature() {
        let f = Function {
            name: "noargs".into(),
            params: Vec::new(),
            return_type: Type::F64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::LitFloat(1.0),
            },
        };
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("at least one scalar input"));
    }

    // ─── PMAT-980: the ARRAY element-wise kernel (`list[scalar]` param + `xs[i]`) ───

    fn lp(name: &str, elem: Type) -> Param {
        p(name, Type::List(Box::new(elem)))
    }

    fn index(coll: &str, idx: &str) -> Expr {
        Expr::Index {
            collection: Box::new(Expr::Ident(coll.into())),
            index: Box::new(Expr::Ident(idx.into())),
        }
    }

    /// `def k(xs: list[f64]) -> f64: return xs[i] + 1.0` — the canonical GPU
    /// element-wise array kernel: a `list[scalar]` param read by `xs[i]` at the
    /// per-thread index, +1.
    fn array_add_one_f64() -> Function {
        kernel_returning(
            "addone",
            vec![lp("xs", Type::F64)],
            Type::F64,
            Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(index("xs", "i")),
                rhs: Box::new(Expr::LitFloat(1.0)),
            },
        )
    }

    #[test]
    fn emits_array_kernel_index_load_and_store() {
        let ptx = emit_kernel(&array_add_one_f64(), "sm_89").unwrap();
        // Routed to the array path (its banner), NOT the scalar one.
        assert!(ptx.contains("ARRAY element-wise subset"));
        // One input array pointer (param_0), the output (param_1), the count
        // (param_2).
        assert!(ptx.contains("xpile_kernel_param_0"));
        assert!(ptx.contains("xpile_kernel_param_1"));
        assert!(ptx.contains("xpile_kernel_param_2"));
        // The `xs[i]` read is a real indexed global load: thread-index stride +
        // add onto the global base + ld.global.
        assert!(ptx.contains("mul.wide.s32"));
        assert!(ptx.contains("ld.global.f64"));
        // The `+ 1.0`.
        assert!(ptx.contains("add.rn.f64"));
        // The `out[i] = ...` store.
        assert!(ptx.contains("st.global.f64"));
        // The element class is NOT loaded in the prologue: the only ld.global
        // for the input happens in the body (one input read).
        assert_eq!(ptx.matches("ld.global.f64").count(), 1);
    }

    /// `def k(xs: list[f64], ys: list[f64]) -> f64: return xs[i] + ys[i]` — a
    /// two-array element-wise add (the array twin of the multi-param scalar
    /// kernel), two indexed loads.
    #[test]
    fn emits_two_array_kernel() {
        let f = kernel_returning(
            "addarr",
            vec![lp("xs", Type::F64), lp("ys", Type::F64)],
            Type::F64,
            Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(index("xs", "i")),
                rhs: Box::new(index("ys", "i")),
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("xpile_kernel_param_0")); // xs
        assert!(ptx.contains("xpile_kernel_param_1")); // ys
        assert!(ptx.contains("xpile_kernel_param_2")); // out
        assert!(ptx.contains("xpile_kernel_param_3")); // count
                                                       // Two indexed input loads, one output store.
        assert_eq!(ptx.matches("ld.global.f64").count(), 2);
        assert_eq!(ptx.matches("st.global.f64").count(), 1);
    }

    /// An i64 array kernel uses `.s64` loads/stores and the i64 register class.
    #[test]
    fn emits_i64_array_kernel() {
        let f = kernel_returning(
            "addi",
            vec![lp("xs", Type::I64)],
            Type::I64,
            Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(index("xs", "i")),
                rhs: Box::new(Expr::LitInt(7)),
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("ld.global.s64"));
        assert!(ptx.contains("add.s64"));
        assert!(ptx.contains("st.global.s64"));
    }

    /// The array path composes with the existing PMAT-972 builtins:
    /// `relu(xs[i]) = max(xs[i], 0.0)`.
    #[test]
    fn array_kernel_composes_with_builtins() {
        let f = kernel_returning(
            "relu",
            vec![lp("xs", Type::F64)],
            Type::F64,
            Expr::NumBuiltin {
                op: NumBuiltinOp::Max,
                args: vec![index("xs", "i"), Expr::LitFloat(0.0)],
                of_float: true,
            },
        );
        let ptx = emit_kernel(&f, "sm_89").unwrap();
        assert!(ptx.contains("ld.global.f64"));
        assert!(ptx.contains("max.f64"));
        assert!(ptx.contains("st.global.f64"));
    }

    #[test]
    fn refuses_list_of_non_scalar_element() {
        let f = kernel_returning(
            "bad",
            vec![lp("xs", Type::Str)],
            Type::F64,
            index("xs", "i"),
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("str/list/dict"));
    }

    #[test]
    fn refuses_list_element_class_mismatch() {
        // list[i64] but f64 return — mixed width.
        let f = kernel_returning(
            "bad",
            vec![lp("xs", Type::I64)],
            Type::F64,
            index("xs", "i"),
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("scalar class"));
    }

    #[test]
    fn refuses_mixed_list_and_scalar_params() {
        let f = kernel_returning(
            "bad",
            vec![lp("xs", Type::F64), p("k", Type::F64)],
            Type::F64,
            index("xs", "i"),
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("non-list parameter"));
    }

    #[test]
    fn refuses_non_thread_index_subscript() {
        // xs[0] — a literal subscript has no per-thread mapping.
        let f = kernel_returning(
            "bad",
            vec![lp("xs", Type::F64)],
            Type::F64,
            Expr::Index {
                collection: Box::new(Expr::Ident("xs".into())),
                index: Box::new(Expr::LitInt(0)),
            },
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("non-identifier subscript"));
    }

    #[test]
    fn refuses_two_distinct_index_names() {
        // xs[i] + xs[j] — two subscripts, no single thread index.
        let f = kernel_returning(
            "bad",
            vec![lp("xs", Type::F64)],
            Type::F64,
            Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(index("xs", "i")),
                rhs: Box::new(index("xs", "j")),
            },
        );
        let err = emit_kernel(&f, "sm_89").unwrap_err();
        assert!(format!("{err}").contains("two different subscripts"));
    }
}
