//! PMAT-970 — real meta-HIR → WGSL lowering for the scalar/control
//! subset, bringing the WGSL compute lane toward the construct-set
//! parity the native-WASM emitter (`xpile-wasm-codegen`) already has.
//!
//! Before this slice the WGSL backend emitted ONLY a scaffold comment
//! (`ScaffoldWgslEmitter`) or one of two hardcoded SAXPY shaders — it
//! refused every meta-HIR node. This module adds a genuine lowering of
//! the **scalar/control subset** directly to WGSL text, the GPU sibling
//! of the WASM lane's WAT lowering: funcs / params / `let` / `if`/`else`
//! statement-and-expression / `while` (→ WGSL `loop` + `break`/`continue`)
//! / `return` / comparisons / arithmetic / bitwise / `array<T>` storage-
//! buffer indexing. Everything outside the subset is an honest
//! [`BackendError::Lower`] refusal — never wrong code.
//!
//! ## Type mapping — WGSL is a 32-bit-native GPU language
//!
//! WGSL core (the floor every wgpu adapter accepts, no extensions) has
//! `i32` / `u32` / `f32` / `bool` — and **no 64-bit scalar**. So:
//!
//! - `I64` → `i32`. A Python `int` (and the C `int` that also lowers to
//!   `I64`) has no width the SOURCE declared, so mapping it to the
//!   GPU-native 32-bit integer is a lane CHOICE, not a contradiction of
//!   anything the user wrote. This is the documented WGSL-subset posture
//!   (the analogue of the WASM lane documenting its overflow-trap
//!   posture): the lane is for GPU compute kernels, where 32-bit
//!   integers are the native width. An out-of-`i32` LITERAL is refused
//!   (PMAT-1401, `wgsl_int_boundary_witness.rs`); an out-of-range value
//!   arriving at RUNTIME through a parameter is the caller's contract to
//!   avoid, exactly as variable-index bounds-checking is out of scope in
//!   the WASM lane.
//! - `CLong` is **refused** (PMAT-1404). This is the case `I64` is not:
//!   `Type::CLong` exists precisely to carry a width the C source
//!   DECLARED — `long` / `long long` / `int64_t`, kept apart from `I64`
//!   by `decy-frontend` so the 64 bits survive lowering — and folding it
//!   into the `I64` arm collapsed the one distinction the type was
//!   introduced to preserve. Through v0.1.617 `long f(long a)` emitted
//!   `fn f(a: i32) -> i32` at exit 0, silently halving a declared range,
//!   while the SAME lane refused to write the literal `3000000000` in
//!   that same function ("the concrete type `i32` cannot represent the
//!   abstract value `3000000000` accurately") — it declined to write
//!   down what it was silently accepting. Every other backend honours
//!   the declaration: rust `i64`, wasm `i64`, ptx `.s64`.
//! - `F32` → `f32`, `CUInt` → `u32`, `Bool` → `bool`.
//! - `F64` is **refused**: WGSL core has no `f64`, and silently
//!   substituting `f32` would change numeric results — a precision lie
//!   the lane refuses to tell. (An `enable f64;`-gated path is a future
//!   increment, not this one.)
//! - `Str` / `Dict` / `Set` / `Struct` / `Tuple` / `BigInt` / `Optional`
//!   / pointers are refused.
//!
//! ## Buffer indexing (`xs[i]`)
//!
//! A `list[scalar]` **parameter** lowers to a `@group(0) @binding(N)
//! var<storage, ...> xs: array<T>` storage buffer (the GPU analogue of
//! the WASM lane's linear-memory base-pointer). A read-only `xs[i]`
//! (`Expr::Index`) lowers to a direct `xs[u32(i)]` subscript — WGSL
//! array indexing, the natural GPU form. The element type must be a
//! supported scalar (`i32`/`u32`/`f32`); `list[bool]`, nested lists, and
//! `list[str]` are refused.
//!
//! ## Buffer WRITE (`xs[i] = v`) — PMAT-979
//!
//! A single-index `xs[i] = v` (`Stmt::IndexAssign`) over a list param is
//! a real storage-buffer **store** — the companion of the read path that
//! turns the WGSL lane into a genuine compute kernel (read inputs, write
//! results) rather than read-only sampling. A list param the body ever
//! writes through binds `var<storage, read_write>` (a read-only param
//! stays `read`); the index narrows to `u32` and the value's WGSL type
//! must equal the buffer element type. Only a 1-D store is in the subset:
//! a nested `grid[i][j] = v` (multi-index `IndexAssign`) is refused, as
//! is a write to anything that is not a `list[scalar]` parameter.
//!
//! ## Validation
//!
//! Emitted WGSL is checked by [`naga_validate_wgsl`] — a CPU-only
//! `naga::front::wgsl::parse_str` + `naga::valid::Validator` round-trip
//! (no GPU), the same naga pin `xpile-spirv-codegen` uses. This is a
//! STRONGER gate than the text-structural [`crate::validate_wgsl`]: naga
//! actually parses and type-checks the WGSL.
//!
//! **PMAT-1391 — this became true of the PRODUCTION path on 2026-07-27.**
//! Until then the sentence above described the unit tests only: nothing
//! on the `xpile transpile --target wgsl` path ever called the validator,
//! so `--target wgsl` exited 0 emitting WGSL this crate's own exported
//! gate rejected (a `for i in range(n)` loop desugars to `__forc0` /
//! `__forstop1` locals, and WGSL reserves the `__` identifier prefix;
//! `def f(n): return f(n)` emitted a recursive `fn`, which WGSL forbids).
//! The check is now the last step of [`emit_wgsl_module`], so every
//! caller of the real lowering inherits it, and the reserved-prefix
//! family is fixed at the source by [`wgsl_ident`] rather than merely
//! detected.

use std::fmt::Write as _;

use xpile_backend::BackendError;
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, Stmt, Type, UnOp,
};

/// The Layer-5 compile contract every emitted WGSL function cites.
pub(crate) const CONTRACT_ID: &str = "C-COMPILE-RUST-TO-WGSL";

/// WGSL scalar value type — the lowered shape of a supported meta-HIR
/// [`Type`]. WGSL core is 32-bit-native (no i64/f64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WgslTy {
    I32,
    U32,
    F32,
    Bool,
}

impl WgslTy {
    fn keyword(self) -> &'static str {
        match self {
            WgslTy::I32 => "i32",
            WgslTy::U32 => "u32",
            WgslTy::F32 => "f32",
            WgslTy::Bool => "bool",
        }
    }

    /// `true` for the integer types that admit bitwise / shift ops.
    fn is_int(self) -> bool {
        matches!(self, WgslTy::I32 | WgslTy::U32)
    }
}

fn unsupported(what: &str) -> BackendError {
    BackendError::Lower(format!(
        "xpile-wgsl-codegen: unsupported construct — {what}"
    ))
}

/// PMAT-1391 — sanitize a meta-HIR name into a legal WGSL identifier.
///
/// WGSL reserves two identifier shapes (both confirmed against the naga
/// pin, not assumed): an identifier may not START with `__`, and may not
/// be exactly `_`. A single leading underscore (`_foo`) is legal.
///
/// This matters because the depyler frontend desugars loops into
/// synthetic locals named `__forc0` / `__forstop1` / `__broke0` (see
/// `crates/depyler-frontend/src/lib.rs`). Those names are CORRECT for
/// the Rust and WASM lanes and must not change there — so the fix is a
/// mangle at the WGSL emission boundary, applied at every site that
/// writes a name into the output. A user identifier literally spelled
/// `__x` is equally illegal, so this sanitizes by SHAPE, not by a
/// hardcoded list of the frontend's synthetics.
///
/// ## The mapping is INJECTIVE — a mangled name can never alias a user name
///
/// Sanitizing only the offending names would be an incomplete fix: a
/// naive `__forc0` → `forc0` collides with a user variable actually
/// named `forc0`, trading an honest naga rejection for a silent
/// wrong-variable read. So the mapping is total and injective:
///
/// | input starts with | output              | output starts with |
/// |-------------------|---------------------|--------------------|
/// | `xpm`             | `xpm_e_` + input    | `xpm_e_`           |
/// | `_`               | `xpm_u` + input     | `xpm_u_`           |
/// | anything else     | input (unchanged)   | neither            |
///
/// The three output classes are pairwise disjoint (rows 1 and 2 differ
/// at byte 4, `e` vs `u`; row 3 can start with neither `xpm` nor `_`),
/// and each row is individually reversible by stripping its prefix.
/// Therefore distinct inputs always produce distinct outputs, and the
/// common case — every identifier a user would plausibly write — is the
/// identity, so emitted WGSL stays readable.
///
/// Reserved WGSL KEYWORDS (`var`, `let`, `loop`, …) are deliberately NOT
/// mangled: a function or variable named `let` is vanishingly rare, and
/// the `naga_validate_wgsl` gate now wired into [`emit_wgsl_module`]
/// turns it into an honest exit-1 refusal that names the keyword rather
/// than a silent wrong emit. Widening the mangle to keywords is a
/// future increment, not a correctness hole.
fn wgsl_ident(name: &str) -> String {
    if name.starts_with("xpm") {
        format!("xpm_e_{name}")
    } else if name.starts_with('_') {
        format!("xpm_u{name}")
    } else {
        name.to_string()
    }
}

/// The refusal text for a C `long` reaching the WGSL subset (PMAT-1404).
///
/// Shared by [`map_type`] and [`map_list_elem_type`] so the scalar and the
/// `list[…]` element positions cannot drift into stating different reasons
/// for the same refusal.
///
/// It names the OTHER backends' dispositions on purpose: without them a user
/// reads this as "xpile cannot handle `long`", when the actual claim is
/// narrower — the GPU lane has no 64-bit integer, and four other lanes do.
const CLONG_REFUSAL: &str = "C `long` / `long long` / `int64_t` (meta-HIR CLong, 64-bit) — WGSL \
     core has no 64-bit integer, and narrowing to i32 would silently change \
     results for any value outside the i32 range. The C source DECLARED the \
     width, so the WGSL subset refuses it rather than shrink it behind the \
     user's back — the same posture as `f64` and `unsigned long`. Every other \
     backend honours the declaration (rust `i64`, wasm `i64`, ptx `.s64`); use \
     `int` in the C source for the GPU lane, or target one of those instead";

/// Map a meta-HIR [`Type`] to its WGSL scalar type, refusing everything
/// outside the 32-bit GPU subset.
fn map_type(ty: &Type) -> Result<WgslTy, BackendError> {
    match ty {
        // Python `int` (undeclared width) rides an i32 — the GPU-native
        // integer width. Documented 32-bit-subset posture; see the module
        // docs for why an UNDECLARED width may be chosen and a DECLARED
        // one may not.
        Type::I64 => Ok(WgslTy::I32),
        Type::CUInt => Ok(WgslTy::U32),
        Type::F32 => Ok(WgslTy::F32),
        Type::Bool => Ok(WgslTy::Bool),
        Type::CLong => Err(unsupported(CLONG_REFUSAL)),
        Type::F64 => Err(unsupported(
            "f64 — WGSL core has no 64-bit float; substituting f32 would \
             change numeric results, so the WGSL subset refuses f64 rather \
             than narrow it silently (use f32 for the GPU lane)",
        )),
        other => Err(unsupported(&format!(
            "type {other:?} (the WGSL emit subset is i32/u32/f32/bool only — \
             f64/str/list/dict/set/struct/tuple/bigint/pointer are refused)"
        ))),
    }
}

/// Map a `list[T]` element type to its WGSL `array<T>` element scalar.
/// The list itself becomes a `var<storage>` buffer; the element must be a
/// supported scalar (`i32`/`u32`/`f32`). `list[bool]`, `list[f64]`,
/// nested lists, and `list[str]` are refused.
fn map_list_elem_type(inner: &Type) -> Result<WgslTy, BackendError> {
    match inner {
        Type::I64 => Ok(WgslTy::I32),
        Type::CUInt => Ok(WgslTy::U32),
        Type::F32 => Ok(WgslTy::F32),
        // `list[long]` narrows every ELEMENT, so it is refused for the same
        // reason a scalar `long` is — and stated separately so the refusal
        // names the position it came from.
        Type::CLong => Err(unsupported(&format!("list element type — {CLONG_REFUSAL}"))),
        other => Err(unsupported(&format!(
            "list element type {other:?} — the WGSL list subset supports \
             list[int]/list[uint]/list[float32] only (i32/u32/f32 array \
             elements); list[bool], list[f64], list[str], and nested lists \
             are refused"
        ))),
    }
}

/// Per-function lowering scope: the WGSL type of every in-scope local
/// (params + `let` bindings) and, for list-param buffers, the element
/// type so `Index` knows the result type.
struct Scope {
    /// `(name, ty)` for every scalar local, in declaration order.
    locals: Vec<(String, WgslTy)>,
    /// For each `list[scalar]` param, the WGSL element type of its
    /// `array<T>` storage buffer (`i32`/`u32`/`f32`).
    list_elem: Vec<(String, WgslTy)>,
    /// For each `list[scalar]` param, the module-scope storage-buffer var
    /// name (`<fn>_<param>`) the param ident resolves to when indexed.
    buffer_var: Vec<(String, String)>,
    /// The function's return WGSL type, or `None` for a unit/void fn.
    ret: Option<WgslTy>,
}

impl Scope {
    fn ty_of(&self, name: &str) -> Result<WgslTy, BackendError> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
            .ok_or_else(|| unsupported(&format!("reference to unbound name `{name}`")))
    }

    fn list_elem_of(&self, name: &str) -> Option<WgslTy> {
        self.list_elem
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
    }

    fn buffer_var_of(&self, name: &str) -> Option<String> {
        self.buffer_var
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.clone())
    }

    fn declare(&mut self, name: &str, ty: WgslTy) {
        if !self.locals.iter().any(|(n, _)| n == name) {
            self.locals.push((name.to_string(), ty));
        }
    }
}

/// Emit a full WGSL module for `module`. Each [`Item::Function`] in the
/// scalar/control subset becomes a WGSL `fn`; a `list[scalar]` param
/// becomes a module-level `var<storage>` buffer binding. Any non-function
/// item, or any function using an unsupported construct, is an honest
/// [`BackendError::Lower`] refusal.
///
/// This is the real meta-HIR lowering that replaces the WGSL backend's
/// scaffold-comment placeholder (PMAT-970). The output parses and
/// type-checks under [`naga_validate_wgsl`].
pub fn emit_wgsl_module(module: &Module) -> Result<String, BackendError> {
    // Collect every list-param buffer FIRST so all bindings are declared
    // at module scope (WGSL requires resource vars at module scope), then
    // the function bodies reference them.
    let mut out = String::new();
    writeln!(
        out,
        "// xpile-wgsl-codegen — meta-HIR → WGSL (scalar/control subset)"
    )
    .expect("write");
    writeln!(out, "// source module: {}", module.name).expect("write");
    writeln!(out, "// contract: {CONTRACT_ID}").expect("write");

    // First pass: emit storage-buffer bindings for every list param, with
    // a stable, globally-unique binding index. The buffer var name is
    // `<fnname>_<paramname>` so two functions can each take a list param
    // without a name clash at module scope.
    //
    // PMAT-979: a list param that the body ever WRITES through (a single-
    // index `xs[i] = v`, `Stmt::IndexAssign`) needs WGSL access mode
    // `read_write`; a read-only param stays `read`. We pre-scan each
    // function for its written-list-param set so the binding's access
    // mode matches its use — this is what unlocks a real compute kernel
    // (read inputs, store results) rather than read-only sampling.
    let mut binding: u32 = 0;
    for item in &module.items {
        if let Item::Function(f) = item {
            let written = written_list_params(&f.body);
            for Param { name, ty, .. } in &f.params {
                if let Type::List(inner) = ty {
                    let elem = map_list_elem_type(inner)?;
                    let access = if written.contains(name) {
                        "read_write"
                    } else {
                        "read"
                    };
                    writeln!(
                        out,
                        "@group(0) @binding({binding}) var<storage, {access}> {buf}: array<{ty}>;",
                        buf = buffer_var(&f.name, name),
                        ty = elem.keyword()
                    )
                    .expect("write");
                    binding += 1;
                }
            }
        }
    }
    if binding > 0 {
        out.push('\n');
    }

    let mut emitted_any = false;
    for item in &module.items {
        match item {
            Item::Function(f) => {
                let f_wgsl = emit_function(f)?;
                out.push_str(&f_wgsl);
                emitted_any = true;
            }
            Item::Const { name, .. } => {
                return Err(unsupported(&format!(
                    "module-level const `{name}` (only scalar/control functions are in the WGSL subset)"
                )));
            }
            Item::Struct { name, .. } => {
                return Err(unsupported(&format!(
                    "struct `{name}` (aggregates are outside the WGSL scalar/control subset)"
                )));
            }
            Item::Enum { name, .. } => {
                return Err(unsupported(&format!(
                    "enum `{name}` (outside the WGSL scalar/control subset)"
                )));
            }
        }
    }
    if !emitted_any {
        return Err(unsupported(
            "module has no functions to lower (WGSL subset emits scalar/control \
             functions; an empty module would produce no entry point)",
        ));
    }

    // PMAT-1391 — the emission-boundary gate. Until this slice the module
    // doc's "Emitted WGSL is checked by `naga_validate_wgsl`" was true only
    // of the unit tests: the PRODUCTION path (`RealWgslEmitter::try_emit` →
    // `WgslBackend::lower` → `xpile transpile --target wgsl`) never called
    // the validator, so `--target wgsl` exited 0 handing the user WGSL this
    // repo's own exported gate rejects. Validating HERE rather than in
    // `try_emit` makes the guarantee structural: every caller of the real
    // lowering — the production emitter, the wgpu diff-exec witness's
    // general slot, and the tests — inherits it from one choke point.
    //
    // A rejection is an honest `BackendError::Lower` naming naga's reason,
    // which is what turns the recursion case (`def f(n): return f(n)` →
    // `declaration of `f` is recursive`) from a silent exit-0 wrong emit
    // into an exit-1 refusal. It is also the BACKSTOP for `wgsl_ident`: if
    // some future name-emission site is added without sanitizing, the
    // failure mode is a loud refusal, never wrong code.
    naga_validate_wgsl(&out).map_err(|e| {
        BackendError::Lower(format!(
            "xpile-wgsl-codegen: emitted WGSL failed naga validation — {e}"
        ))
    })?;
    Ok(out)
}

/// The module-scope storage-buffer var name for a `list` parameter:
/// `<fnname>_<paramname>`, unique across functions.
///
/// PMAT-1391: sanitized through [`wgsl_ident`], so a function or param
/// whose name starts with `_` still yields a legal WGSL module-scope
/// identifier. The composite is built from the RAW names and mangled as
/// a whole, so the single call site here is the only place the buffer
/// name is spelled — every consumer (`emit_wgsl_module`'s binding decl,
/// `emit_index`'s read, `Stmt::IndexAssign`'s store) reads it back from
/// the `Scope` already sanitized.
fn buffer_var(fn_name: &str, param: &str) -> String {
    wgsl_ident(&format!("{fn_name}_{param}"))
}

/// Emit one WGSL `fn` for `f`.
fn emit_function(f: &Function) -> Result<String, BackendError> {
    let ret = match &f.return_type {
        Type::Unit => None,
        ty => Some(map_type(ty)?),
    };

    let mut scope = Scope {
        locals: Vec::new(),
        list_elem: Vec::new(),
        buffer_var: Vec::new(),
        ret,
    };

    // Params. A scalar param is a WGSL fn parameter; a `list[scalar]`
    // param is NOT a fn parameter — it is the module-scope storage buffer
    // emitted by `emit_wgsl_module`, so the fn signature omits it and the
    // body references the buffer var directly.
    let mut sig_params: Vec<(String, WgslTy)> = Vec::new();
    for Param { name, ty, .. } in &f.params {
        if let Type::List(inner) = ty {
            let elem = map_list_elem_type(inner)?;
            // Bind the param name to the buffer var so `Index` over it
            // resolves; record the element type and the module-scope
            // buffer var name (`<fn>_<param>`).
            scope.list_elem.push((name.clone(), elem));
            scope
                .buffer_var
                .push((name.clone(), buffer_var(&f.name, name)));
            // The list param is NOT a scalar local — indexing it yields a
            // scalar but the buffer itself is not a value.
        } else {
            let wt = map_type(ty)?;
            scope.declare(name, wt);
            sig_params.push((name.clone(), wt));
        }
    }

    // Pre-walk the body so every `let`-bound local's type is known (WGSL
    // `let`/`var` are declared at use site, but we still validate types).
    collect_let_locals(&f.body, &mut scope)?;

    // Emit the body.
    let mut body = String::new();
    let needs_var = mutated_locals(&f.body);
    // Declare every local that is reassigned as a `var` up front (WGSL
    // `let` is immutable; a reassigned binding must be a `var`). We hoist
    // these so a later `Assign` (including inside a loop) is legal.
    for (name, wt) in &scope.locals {
        if needs_var.contains(name) {
            writeln!(
                body,
                "  var {name}: {ty};",
                name = wgsl_ident(name),
                ty = wt.keyword()
            )
            .expect("write");
        }
    }
    for stmt in &f.body.stmts {
        emit_stmt(stmt, &mut scope, &needs_var, &mut body, 1)?;
    }
    // Trailing return expression.
    match (&ret, &f.body.trailing_return) {
        (None, Expr::Unit) => {}
        (None, other) => {
            // A void fn with a non-unit trailing expr: evaluate and discard.
            // WGSL has no statement-expression discard for a bare value, so
            // refuse rather than emit something naga rejects.
            return Err(unsupported(&format!(
                "void function with a non-unit trailing expression {} \
                 (the WGSL subset wants `return;` or a unit tail for a void fn)",
                expr_kind(other)
            )));
        }
        (Some(rt), e) => {
            let mut expr_buf = String::new();
            let got = emit_expr(e, &scope, &mut expr_buf)?;
            if got != *rt {
                return Err(unsupported(&format!(
                    "trailing return lowers to WGSL {} but the function returns {}",
                    got.keyword(),
                    rt.keyword()
                )));
            }
            writeln!(body, "  return {expr_buf};").expect("write");
        }
    }

    // Assemble the signature.
    let mut out = String::new();
    writeln!(out, "// xpile-contract: {CONTRACT_ID}").expect("write");
    write!(out, "fn {}(", wgsl_ident(&f.name)).expect("write");
    let mut first = true;
    for (name, wt) in &sig_params {
        if !first {
            out.push_str(", ");
        }
        write!(
            out,
            "{name}: {ty}",
            name = wgsl_ident(name),
            ty = wt.keyword()
        )
        .expect("write");
        first = false;
    }
    out.push(')');
    if let Some(rt) = ret {
        write!(out, " -> {}", rt.keyword()).expect("write");
    }
    writeln!(out, " {{").expect("write");
    out.push_str(&body);
    writeln!(out, "}}").expect("write");
    Ok(out)
}

/// Walk the body collecting every `Let`-bound local's WGSL type.
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
            // PMAT-979: a single-index `xs[i] = v` storage write binds no
            // new local; the actual subset checks happen at emit time.
            Stmt::IndexAssign { indices, .. } if indices.len() == 1 => {}
            other => {
                return Err(unsupported(&format!(
                    "statement {} (outside the WGSL scalar/control subset)",
                    stmt_kind(other)
                )));
            }
        }
    }
    Ok(())
}

/// Collect the set of local names that are ever `Assign`-ed (reassigned)
/// anywhere in the body — these must be WGSL `var` (mutable) bindings
/// rather than `let` (immutable). A name only ever `Let`-bound is a `let`.
fn mutated_locals(block: &Block) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    collect_mutated(&block.stmts, &mut set);
    set
}

fn collect_mutated(stmts: &[Stmt], set: &mut std::collections::BTreeSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign { name, .. } => {
                set.insert(name.clone());
            }
            Stmt::While { body, .. } => collect_mutated(body, set),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_mutated(then_body, set);
                collect_mutated(else_body, set);
            }
            _ => {}
        }
    }
}

/// PMAT-979: collect the set of list-param names that are the target of a
/// single-index `xs[i] = v` (`Stmt::IndexAssign`) anywhere in the body.
/// These buffers bind `var<storage, read_write>` (a read-only param stays
/// `read`). A multi-index `IndexAssign` (nested list) is NOT a WGSL-subset
/// store — those names are not collected here and the statement is refused
/// at emit time, so a 1-D storage write is the only thing that flips the
/// access mode.
fn written_list_params(block: &Block) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    collect_written_list_params(&block.stmts, &mut set);
    set
}

fn collect_written_list_params(stmts: &[Stmt], set: &mut std::collections::BTreeSet<String>) {
    for s in stmts {
        match s {
            Stmt::IndexAssign {
                list_name, indices, ..
            } if indices.len() == 1 => {
                set.insert(list_name.clone());
            }
            Stmt::While { body, .. } => collect_written_list_params(body, set),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_written_list_params(then_body, set);
                collect_written_list_params(else_body, set);
            }
            _ => {}
        }
    }
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
        Stmt::IndexAssign { .. } => "IndexAssign",
        _ => "<container/aggregate statement>",
    }
}

/// Emit a statement at `depth` indentation into `out`.
fn emit_stmt(
    s: &Stmt,
    scope: &mut Scope,
    needs_var: &std::collections::BTreeSet<String>,
    out: &mut String,
    depth: usize,
) -> Result<(), BackendError> {
    match s {
        Stmt::Let { name, value, .. } => {
            let wt = scope.ty_of(name)?;
            let mut buf = String::new();
            let got = emit_expr(value, scope, &mut buf)?;
            if got != wt {
                return Err(unsupported(&format!(
                    "let `{name}` annotated {} but its initializer lowers to {}",
                    wt.keyword(),
                    got.keyword()
                )));
            }
            indent(out, depth);
            if needs_var.contains(name) {
                // Reassigned later → hoisted as a `var` up front; here just
                // assign the initial value (the `var` decl already happened).
                writeln!(out, "{name} = {buf};", name = wgsl_ident(name)).expect("write");
            } else {
                writeln!(
                    out,
                    "let {name}: {ty} = {buf};",
                    name = wgsl_ident(name),
                    ty = wt.keyword()
                )
                .expect("write");
            }
            Ok(())
        }
        Stmt::Assign { name, value } => {
            let wt = scope.ty_of(name)?;
            let mut buf = String::new();
            let got = emit_expr(value, scope, &mut buf)?;
            if got != wt {
                return Err(unsupported(&format!(
                    "assignment to `{name}` ({}) from a {} value",
                    wt.keyword(),
                    got.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "{name} = {buf};", name = wgsl_ident(name)).expect("write");
            Ok(())
        }
        // PMAT-979: `xs[i] = v` over a `list[scalar]` PARAMETER — a real
        // storage-buffer store. The companion of the read path
        // (`emit_index`): the param's buffer was bound `var<storage,
        // read_write>` (see `written_list_params`), the index narrows to
        // `u32`, and the value's WGSL type must equal the buffer's element
        // type. This is what turns the WGSL lane into a real compute kernel
        // (read inputs, write results) rather than read-only sampling.
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => {
            // Only a SINGLE index (a 1-D storage buffer) is in the subset;
            // a nested `grid[i][j] = v` has no flat-buffer lowering here.
            let [index] = indices.as_slice() else {
                return Err(unsupported(&format!(
                    "multi-index `{list_name}[…][…] = v` ({} indices) — the WGSL \
                     list subset stores into a 1-D `array<T>` storage buffer only \
                     (nested list writes are refused)",
                    indices.len()
                )));
            };
            // The target must be a `list[scalar]` parameter (a storage
            // buffer); a local list / temporary has no buffer binding.
            let Some(elem) = scope.list_elem_of(list_name) else {
                return Err(unsupported(&format!(
                    "indexed write to `{list_name}` which is not a `list[scalar]` \
                     parameter — only a list param (a storage buffer) is writable \
                     in the WGSL subset"
                )));
            };
            // Index → u32 subscript (same narrowing as the read path).
            let mut ibuf = String::new();
            let it = emit_expr(index, scope, &mut ibuf)?;
            let idx = match it {
                WgslTy::I32 => format!("u32({ibuf})"),
                WgslTy::U32 => ibuf,
                other => {
                    return Err(unsupported(&format!(
                        "list index lowers to WGSL {} (an integer index is required)",
                        other.keyword()
                    )));
                }
            };
            // The stored value must match the buffer's element type.
            let mut vbuf = String::new();
            let vt = emit_expr(value, scope, &mut vbuf)?;
            if vt != elem {
                return Err(unsupported(&format!(
                    "store into `{list_name}: array<{}>` from a {} value",
                    elem.keyword(),
                    vt.keyword()
                )));
            }
            let buf_name = scope
                .buffer_var_of(list_name)
                .unwrap_or_else(|| list_name.to_string());
            indent(out, depth);
            writeln!(out, "{buf_name}[{idx}] = {vbuf};").expect("write");
            Ok(())
        }
        Stmt::Return(e) => {
            match &scope.ret {
                None => {
                    if !matches!(e, Expr::Unit) {
                        return Err(unsupported(
                            "early `return <value>` from a unit/void function",
                        ));
                    }
                    indent(out, depth);
                    writeln!(out, "return;").expect("write");
                }
                Some(rt) => {
                    let mut buf = String::new();
                    let got = emit_expr(e, scope, &mut buf)?;
                    if got != *rt {
                        return Err(unsupported(&format!(
                            "early return lowers to WGSL {} but the function returns {}",
                            got.keyword(),
                            rt.keyword()
                        )));
                    }
                    indent(out, depth);
                    writeln!(out, "return {buf};").expect("write");
                }
            }
            Ok(())
        }
        Stmt::Break => {
            indent(out, depth);
            writeln!(out, "break;").expect("write");
            Ok(())
        }
        Stmt::Continue => {
            indent(out, depth);
            writeln!(out, "continue;").expect("write");
            Ok(())
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let mut cbuf = String::new();
            let ct = emit_expr(cond, scope, &mut cbuf)?;
            if ct != WgslTy::Bool {
                return Err(unsupported(&format!(
                    "`if` condition lowers to WGSL {} (a bool is required)",
                    ct.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "if ({cbuf}) {{").expect("write");
            for st in then_body {
                emit_stmt(st, scope, needs_var, out, depth + 1)?;
            }
            if else_body.is_empty() {
                indent(out, depth);
                writeln!(out, "}}").expect("write");
            } else {
                indent(out, depth);
                writeln!(out, "}} else {{").expect("write");
                for st in else_body {
                    emit_stmt(st, scope, needs_var, out, depth + 1)?;
                }
                indent(out, depth);
                writeln!(out, "}}").expect("write");
            }
            Ok(())
        }
        Stmt::While { cond, body } => {
            // Python/Rust `while cond { body }` → WGSL:
            //   loop { if (!(cond)) { break; } <body> }
            // WGSL has no `while`; `loop` + a guard `break` is the form.
            // `continue` inside `body` re-enters the loop (WGSL `continue`).
            let mut cbuf = String::new();
            let ct = emit_expr(cond, scope, &mut cbuf)?;
            if ct != WgslTy::Bool {
                return Err(unsupported(&format!(
                    "`while` condition lowers to WGSL {} (a bool is required)",
                    ct.keyword()
                )));
            }
            indent(out, depth);
            writeln!(out, "loop {{").expect("write");
            indent(out, depth + 1);
            writeln!(out, "if (!({cbuf})) {{ break; }}").expect("write");
            for st in body {
                emit_stmt(st, scope, needs_var, out, depth + 1)?;
            }
            indent(out, depth);
            writeln!(out, "}}").expect("write");
            Ok(())
        }
        other => Err(unsupported(&format!(
            "statement {} (outside the WGSL scalar/control subset)",
            stmt_kind(other)
        ))),
    }
}

/// Emit an expression into `out` as a WGSL value-string, returning the
/// WGSL type it produces. (Expressions are emitted as parenthesized
/// infix WGSL text — the natural high-level form, unlike the WASM lane's
/// stack instructions.)
fn emit_expr(e: &Expr, scope: &Scope, out: &mut String) -> Result<WgslTy, BackendError> {
    match e {
        Expr::Ident(name) => {
            let wt = scope.ty_of(name)?;
            // The `Scope` stays keyed by the RAW meta-HIR name (so lookup
            // is unaffected); only the emitted spelling is sanitized.
            out.push_str(&wgsl_ident(name));
            Ok(wt)
        }
        Expr::LitInt(v) => {
            // Python int → i32 (the 32-bit GPU-native posture). Annotate
            // the literal so naga types it as i32, not abstract-int.
            write!(out, "i32({v})").expect("write");
            Ok(WgslTy::I32)
        }
        Expr::LitBool(b) => {
            out.push_str(if *b { "true" } else { "false" });
            Ok(WgslTy::Bool)
        }
        Expr::LitFloat(v) => {
            write!(out, "f32({})", wgsl_float_literal(*v)).expect("write");
            Ok(WgslTy::F32)
        }
        Expr::UnOp { op, operand } => emit_unop(*op, operand, scope, out),
        Expr::BinOp { op, lhs, rhs } => emit_binop(*op, lhs, rhs, scope, out),
        Expr::FloatBinOp { op, lhs, rhs } => emit_float_binop(*op, lhs, rhs, scope, out),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            // WGSL has no `?:`; the `select(false_val, true_val, cond)`
            // builtin is the value-level conditional. Both arms must share
            // a type.
            let mut cbuf = String::new();
            let ct = emit_expr(cond, scope, &mut cbuf)?;
            if ct != WgslTy::Bool {
                return Err(unsupported(&format!(
                    "if-expression condition lowers to WGSL {} (a bool is required)",
                    ct.keyword()
                )));
            }
            let mut tbuf = String::new();
            let tt = emit_expr(then_expr, scope, &mut tbuf)?;
            let mut ebuf = String::new();
            let et = emit_expr(else_expr, scope, &mut ebuf)?;
            if tt != et {
                return Err(unsupported(&format!(
                    "if-expression arms lower to different WGSL types ({} vs {})",
                    tt.keyword(),
                    et.keyword()
                )));
            }
            // select(false, true, cond): the false value first.
            write!(out, "select({ebuf}, {tbuf}, {cbuf})").expect("write");
            Ok(tt)
        }
        Expr::Index { collection, index } => emit_index(collection, index, scope, out),
        Expr::Call { callee, args } => {
            // Direct intra-module call. WGSL calls are infix `f(a, b)`.
            // The result type is not carried in the meta-HIR Call node;
            // since WGSL is strongly typed and naga will reject a real
            // mismatch, we report the dominant scalar (i32) and let naga's
            // validation be the backstop. (A float/bool-returning call used
            // in a typed position that disagrees is caught by naga.)
            write!(out, "{callee}(", callee = wgsl_ident(callee)).expect("write");
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(a, scope, out)?;
            }
            out.push(')');
            Ok(WgslTy::I32)
        }
        Expr::Unit => Err(unsupported(
            "unit value `()` in a value position (WGSL has no unit operand)",
        )),
        other => Err(unsupported(&format!(
            "expression {} (outside the WGSL scalar/control subset — \
             str/list/dict/set/struct/tuple/closure/print are refused)",
            expr_kind(other)
        ))),
    }
}

/// Emit a read-only `xs[i]` over a `list[scalar]` parameter buffer.
///
/// `collection` must be an [`Expr::Ident`] naming a list-param buffer
/// (the only list shape in the WGSL subset). The index lowers to a `u32`
/// subscript (`xs[u32(i)]`). The result is the buffer's element type.
/// Posture matches the WASM lane: a negative / out-of-range index is NOT
/// bounds-checked here (WGSL out-of-bounds access is implementation-
/// clamped/undefined; variable-index bounds checking is out of scope for
/// this first increment).
fn emit_index(
    collection: &Expr,
    index: &Expr,
    scope: &Scope,
    out: &mut String,
) -> Result<WgslTy, BackendError> {
    let Expr::Ident(name) = collection else {
        return Err(unsupported(
            "indexing a non-name collection — the WGSL list subset only \
             indexes a `list[scalar]` PARAMETER (a storage buffer); list \
             literals / temporaries / nested indexing are refused",
        ));
    };
    let Some(elem) = scope.list_elem_of(name) else {
        return Err(unsupported(&format!(
            "index over `{name}` which is not a `list[scalar]` parameter — \
             only a list param (a storage buffer) can be indexed in the WGSL \
             subset (no str/dict/tuple indexing)"
        )));
    };
    let mut ibuf = String::new();
    let it = emit_expr(index, scope, &mut ibuf)?;
    // Index must be an integer; narrow/cast to u32 for the subscript.
    let idx = match it {
        WgslTy::I32 => format!("u32({ibuf})"),
        WgslTy::U32 => ibuf,
        other => {
            return Err(unsupported(&format!(
                "list index lowers to WGSL {} (an integer index is required)",
                other.keyword()
            )));
        }
    };
    // `name` is the param ident; resolve it to the module-scope buffer var
    // name (`<fn>_<param>`) recorded by `emit_function`.
    let buf_name = scope
        .buffer_var_of(name)
        .unwrap_or_else(|| name.to_string());
    write!(out, "{buf_name}[{idx}]").expect("write");
    Ok(elem)
}

/// Render an `f32` value as a WGSL float literal token. WGSL float
/// literals require a decimal point or exponent; `{:?}` on an f64 always
/// renders one. Non-finite f32 values have no WGSL literal, so they are
/// refused upstream (a `LitFloat` is f64 in the meta-HIR; the value is
/// finite for any real source literal).
fn wgsl_float_literal(v: f64) -> String {
    // `{:?}` renders e.g. `2.0`, `-0.5`, `100.0` — all valid WGSL float
    // literal bodies once wrapped in `f32(...)`.
    format!("{v:?}")
}

fn emit_unop(
    op: UnOp,
    operand: &Expr,
    scope: &Scope,
    out: &mut String,
) -> Result<WgslTy, BackendError> {
    match op {
        UnOp::Neg => {
            // PMAT-1401 — FOLD a negated integer LITERAL into the conversion
            // instead of negating the conversion's result.
            //
            // The Python frontend does NOT fold unary minus: `-2147483648`
            // arrives as `UnOp{Neg, LitInt(2147483648)}`. The generic path
            // below emits `(-(i32(2147483648)))`, whose INNER conversion is
            // out of range even though the value being denoted, i32::MIN, is
            // perfectly representable. naga rejected it — so the lane
            // accepted i32::MAX and refused i32::MIN, blaming an "abstract
            // value `2147483648`" that appears nowhere in the user's source.
            //
            // Folding is EXACT, not a coercion: for every other magnitude
            // `i32(-n)` and `(-(i32(n)))` denote the same value, and a
            // magnitude genuinely outside i32 still fails the SAME naga range
            // check — now naming the value the user actually wrote. That
            // distinction is the PMAT-1395/1399 lesson: an over-refusal must
            // be fixed by denoting the right value, never by widening what is
            // accepted.
            if let Expr::LitInt(v) = operand {
                // `checked_neg` guards i64::MIN, whose magnitude has no i64
                // representation; it falls through to the generic path, where
                // it refuses at naga exactly as before (it is far outside i32
                // either way).
                if let Some(neg) = v.checked_neg() {
                    write!(out, "i32({neg})").expect("write");
                    return Ok(WgslTy::I32);
                }
            }
            let mut buf = String::new();
            let t = emit_expr(operand, scope, &mut buf)?;
            match t {
                WgslTy::I32 | WgslTy::F32 => {
                    write!(out, "(-({buf}))").expect("write");
                    Ok(t)
                }
                WgslTy::U32 => Err(unsupported("unary negation of an unsigned (u32) value")),
                WgslTy::Bool => Err(unsupported("unary negation of a bool value")),
            }
        }
        UnOp::Not => {
            let mut buf = String::new();
            let t = emit_expr(operand, scope, &mut buf)?;
            if t != WgslTy::Bool {
                return Err(unsupported("logical `not` of a non-bool value"));
            }
            write!(out, "(!({buf}))").expect("write");
            Ok(WgslTy::Bool)
        }
        UnOp::BitNot => {
            let mut buf = String::new();
            let t = emit_expr(operand, scope, &mut buf)?;
            if !t.is_int() {
                return Err(unsupported("bitwise `~` of a non-integer value"));
            }
            // WGSL bitwise complement is `~`.
            write!(out, "(~({buf}))").expect("write");
            Ok(t)
        }
    }
}

fn emit_binop(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
) -> Result<WgslTy, BackendError> {
    // Short-circuit logical and/or — WGSL `&&` / `||` are short-circuiting
    // over bool, matching Python/Rust.
    if matches!(op, BinOp::And | BinOp::Or) {
        let mut lbuf = String::new();
        let lt = emit_expr(lhs, scope, &mut lbuf)?;
        let mut rbuf = String::new();
        let rt = emit_expr(rhs, scope, &mut rbuf)?;
        if lt != WgslTy::Bool || rt != WgslTy::Bool {
            return Err(unsupported(&format!(
                "logical {op:?} requires bool operands (got {} and {})",
                lt.keyword(),
                rt.keyword()
            )));
        }
        let sym = if matches!(op, BinOp::And) { "&&" } else { "||" };
        write!(out, "({lbuf} {sym} {rbuf})").expect("write");
        return Ok(WgslTy::Bool);
    }

    let mut lbuf = String::new();
    let lt = emit_expr(lhs, scope, &mut lbuf)?;
    let mut rbuf = String::new();
    let rt = emit_expr(rhs, scope, &mut rbuf)?;
    if lt != rt {
        return Err(unsupported(&format!(
            "binary op {op:?} over mixed WGSL types ({} and {})",
            lt.keyword(),
            rt.keyword()
        )));
    }
    let ty = lt;

    // Comparisons yield bool; arithmetic/bitwise yield the operand type.
    let (sym, result) = match (op, ty) {
        // ── arithmetic ──
        (BinOp::Add, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => ("+", ty),
        (BinOp::Sub, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => ("-", ty),
        (BinOp::Mul, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => ("*", ty),
        // FloorDiv / Mod need Python's floor-toward-−∞ correction. WGSL
        // `/` truncates toward zero and `%` takes the dividend's sign, so
        // a faithful Python `//` / `%` over a SIGNED i32 needs the floor
        // correction — refuse it here rather than emit truncating-div that
        // silently disagrees with Python on negative operands. (`u32` div /
        // mod ARE Python-faithful — non-negative — so those are allowed.)
        (BinOp::FloorDiv, WgslTy::U32) => ("/", ty),
        (BinOp::Mod, WgslTy::U32) => ("%", ty),
        // ── bitwise / shift over integers ──
        (BinOp::BitAnd, WgslTy::I32 | WgslTy::U32) => ("&", ty),
        (BinOp::BitOr, WgslTy::I32 | WgslTy::U32) => ("|", ty),
        (BinOp::BitXor, WgslTy::I32 | WgslTy::U32) => ("^", ty),
        // ── comparisons → bool ──
        (BinOp::Eq, _) => ("==", WgslTy::Bool),
        (BinOp::NotEq, _) => ("!=", WgslTy::Bool),
        (BinOp::Lt, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => ("<", WgslTy::Bool),
        (BinOp::LtEq, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => ("<=", WgslTy::Bool),
        (BinOp::Gt, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => (">", WgslTy::Bool),
        (BinOp::GtEq, WgslTy::I32 | WgslTy::U32 | WgslTy::F32) => (">=", WgslTy::Bool),
        (op, ty) => {
            return Err(unsupported(&format!(
                "binary op {op:?} over WGSL {} (not in the scalar/control subset — \
                 signed ///% need a floor correction not yet lowered; shifts, \
                 pow are refused)",
                ty.keyword()
            )));
        }
    };
    write!(out, "({lbuf} {sym} {rbuf})").expect("write");
    Ok(result)
}

fn emit_float_binop(
    op: FloatOp,
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope,
    out: &mut String,
) -> Result<WgslTy, BackendError> {
    let mut lbuf = String::new();
    let lt = emit_expr(lhs, scope, &mut lbuf)?;
    let mut rbuf = String::new();
    let rt = emit_expr(rhs, scope, &mut rbuf)?;
    if lt != WgslTy::F32 || rt != WgslTy::F32 {
        return Err(unsupported(&format!(
            "float op {op:?} requires f32 operands (got {} and {})",
            lt.keyword(),
            rt.keyword()
        )));
    }
    let sym = match op {
        FloatOp::Add => "+",
        FloatOp::Sub => "-",
        FloatOp::Mul => "*",
        FloatOp::Div => "/",
        other => {
            return Err(unsupported(&format!(
                "float op {other:?} (only + - * / are in the WGSL scalar subset; \
                 floordiv/mod/pow/hypot/atan2/log are refused)"
            )));
        }
    };
    write!(out, "({lbuf} {sym} {rbuf})").expect("write");
    Ok(WgslTy::F32)
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
        Expr::Unit => "Unit",
        _ => "<container/aggregate/builtin expression>",
    }
}

// ─── naga CPU-only validation ───────────────────────────────────────────

/// Reasons emitted WGSL fails the [`naga_validate_wgsl`] front-end gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NagaValidationError {
    /// `naga::front::wgsl::parse_str` rejected the WGSL text.
    Parse(String),
    /// `naga::valid::Validator` rejected the parsed module.
    Validate(String),
}

impl std::fmt::Display for NagaValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(m) => write!(f, "naga WGSL parse error: {m}"),
            Self::Validate(m) => write!(f, "naga WGSL validation error: {m}"),
        }
    }
}

impl std::error::Error for NagaValidationError {}

/// CPU-only naga front-end validation of WGSL text: parse + type-check
/// via `naga::front::wgsl::parse_str` + `naga::valid::Validator`. No GPU.
///
/// This is the real "naga-validate the emitted WGSL" gate for the
/// meta-HIR lowering — a STRONGER check than the text-structural
/// [`crate::validate_wgsl`] (which only greps for `@compute`/`fn`). The
/// same naga pin the sibling `xpile-spirv-codegen` crate uses.
pub fn naga_validate_wgsl(wgsl: &str) -> Result<(), NagaValidationError> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|e| NagaValidationError::Parse(e.to_string()))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .map_err(|e| NagaValidationError::Validate(format!("{e:?}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_meta_hir::{Function, Item, Module, Param, SourceLang};

    fn module(items: Vec<Item>) -> Module {
        Module {
            name: "kernel".into(),
            source_lang: SourceLang::Rust,
            items,
            ffi_boundaries: Vec::new(),
        }
    }

    fn param(name: &str, ty: Type) -> Param {
        Param {
            name: name.into(),
            ty,
            mutable: false,
        }
    }

    fn ident(n: &str) -> Expr {
        Expr::Ident(n.into())
    }

    fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::BinOp {
            op,
            lhs: Box::new(l),
            rhs: Box::new(r),
        }
    }

    /// Lower a single-function module and assert the WGSL passes the
    /// CPU-only naga front-end (parse + validate). Returns the WGSL.
    fn lower_and_naga(f: Function) -> String {
        let wgsl = emit_wgsl_module(&module(vec![Item::Function(f)]))
            .expect("lowering should succeed for an in-subset function");
        naga_validate_wgsl(&wgsl).unwrap_or_else(|e| {
            panic!("emitted WGSL must pass naga validation: {e}\n--- WGSL ---\n{wgsl}")
        });
        wgsl
    }

    // ── arithmetic + comparisons + the real meta-HIR signature ──────────

    #[test]
    fn scalar_arithmetic_fn_naga_validates() {
        // fn add3(a: i32, b: i32) -> i32 { return (a + b) + i32(3); }
        let f = Function {
            name: "add3".into(),
            params: vec![param("a", Type::I64), param("b", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: binop(
                    BinOp::Add,
                    binop(BinOp::Add, ident("a"), ident("b")),
                    Expr::LitInt(3),
                ),
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("fn add3(a: i32, b: i32) -> i32"), "{wgsl}");
        assert!(wgsl.contains("return ((a + b) + i32(3));"), "{wgsl}");
        // contract citation rides along
        assert!(wgsl.contains(CONTRACT_ID));
    }

    #[test]
    fn comparison_returns_bool_and_naga_validates() {
        // fn lt(a: i32, b: i32) -> bool { return (a < b); }
        let f = Function {
            name: "lt".into(),
            params: vec![param("a", Type::I64), param("b", Type::I64)],
            return_type: Type::Bool,
            body: Block {
                stmts: vec![],
                trailing_return: binop(BinOp::Lt, ident("a"), ident("b")),
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("fn lt(a: i32, b: i32) -> bool"), "{wgsl}");
        assert!(wgsl.contains("return (a < b);"), "{wgsl}");
    }

    #[test]
    fn float32_arithmetic_naga_validates() {
        // fn saxpy(x: f32) -> f32 { return ((x * f32(2.0)) + f32(1.0)); }
        let f = Function {
            name: "saxpy".into(),
            params: vec![param("x", Type::F32)],
            return_type: Type::F32,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::FloatBinOp {
                    op: FloatOp::Add,
                    lhs: Box::new(Expr::FloatBinOp {
                        op: FloatOp::Mul,
                        lhs: Box::new(ident("x")),
                        rhs: Box::new(Expr::LitFloat(2.0)),
                    }),
                    rhs: Box::new(Expr::LitFloat(1.0)),
                },
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("fn saxpy(x: f32) -> f32"), "{wgsl}");
        assert!(wgsl.contains("((x * f32(2.0)) + f32(1.0))"), "{wgsl}");
    }

    // ── control flow: if/else statement, while → loop, break/continue ───

    #[test]
    fn if_else_statement_and_let_var_naga_validates() {
        // fn clamp_low(n: i32) -> i32 {
        //   var r: i32;
        //   r = n;
        //   if (n < i32(0)) { r = i32(0); }
        //   return r;
        // }
        let f = Function {
            name: "clamp_low".into(),
            params: vec![param("n", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "r".into(),
                        ty: Type::I64,
                        value: ident("n"),
                        mutable: true,
                    },
                    Stmt::If {
                        cond: binop(BinOp::Lt, ident("n"), Expr::LitInt(0)),
                        then_body: vec![Stmt::Assign {
                            name: "r".into(),
                            value: Expr::LitInt(0),
                        }],
                        else_body: vec![],
                    },
                ],
                trailing_return: ident("r"),
            },
        };
        let wgsl = lower_and_naga(f);
        // `r` is reassigned → hoisted as a `var`, not a `let`.
        assert!(wgsl.contains("var r: i32;"), "{wgsl}");
        assert!(wgsl.contains("if ((n < i32(0))) {"), "{wgsl}");
        assert!(wgsl.contains("return r;"), "{wgsl}");
    }

    #[test]
    fn while_loop_with_break_continue_naga_validates() {
        // fn count_to(n: i32) -> i32 {
        //   var i: i32;
        //   i = i32(0);
        //   while (i < n) {
        //     if (i == i32(5)) { break; }
        //     i = (i + i32(1));
        //   }
        //   return i;
        // }
        let f = Function {
            name: "count_to".into(),
            params: vec![param("n", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "i".into(),
                        ty: Type::I64,
                        value: Expr::LitInt(0),
                        mutable: true,
                    },
                    Stmt::While {
                        cond: binop(BinOp::Lt, ident("i"), ident("n")),
                        body: vec![
                            Stmt::If {
                                cond: binop(BinOp::Eq, ident("i"), Expr::LitInt(5)),
                                then_body: vec![Stmt::Break],
                                else_body: vec![],
                            },
                            Stmt::Assign {
                                name: "i".into(),
                                value: binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                            },
                        ],
                    },
                ],
                trailing_return: ident("i"),
            },
        };
        let wgsl = lower_and_naga(f);
        // while → loop { if (!(cond)) { break; } ... }
        assert!(wgsl.contains("loop {"), "{wgsl}");
        assert!(wgsl.contains("if (!((i < n))) { break; }"), "{wgsl}");
        assert!(wgsl.contains("break;"), "{wgsl}");
    }

    #[test]
    fn if_expression_lowers_to_select_and_naga_validates() {
        // fn pick(c: bool, a: i32, b: i32) -> i32 { return select(b, a, c); }
        let f = Function {
            name: "pick".into(),
            params: vec![
                param("c", Type::Bool),
                param("a", Type::I64),
                param("b", Type::I64),
            ],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::IfExpr {
                    cond: Box::new(ident("c")),
                    then_expr: Box::new(ident("a")),
                    else_expr: Box::new(ident("b")),
                },
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("select(b, a, c)"), "{wgsl}");
    }

    // ── buffer (array<T>) indexing over a list[scalar] param ────────────

    #[test]
    fn list_param_lowers_to_storage_buffer_index_naga_validates() {
        // fn first(xs: list[int]) -> i32 { return xs[i32(0)]; }
        let f = Function {
            name: "first".into(),
            params: vec![param("xs", Type::List(Box::new(Type::I64)))],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::Index {
                    collection: Box::new(ident("xs")),
                    index: Box::new(Expr::LitInt(0)),
                },
            },
        };
        let wgsl = lower_and_naga(f);
        // The list param becomes a module-scope storage buffer, NOT a fn arg.
        assert!(
            wgsl.contains("@group(0) @binding(0) var<storage, read> first_xs: array<i32>;"),
            "{wgsl}"
        );
        assert!(wgsl.contains("fn first() -> i32"), "{wgsl}");
        // i32 index narrows to u32 for the WGSL subscript.
        assert!(wgsl.contains("first_xs[u32(i32(0))]"), "{wgsl}");
    }

    #[test]
    fn float_buffer_sum_index_naga_validates() {
        // fn pair_sum(xs: list[f32]) -> f32 { return (xs[i32(0)] + xs[i32(1)]); }
        let f = Function {
            name: "pair_sum".into(),
            params: vec![param("xs", Type::List(Box::new(Type::F32)))],
            return_type: Type::F32,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::FloatBinOp {
                    op: FloatOp::Add,
                    lhs: Box::new(Expr::Index {
                        collection: Box::new(ident("xs")),
                        index: Box::new(Expr::LitInt(0)),
                    }),
                    rhs: Box::new(Expr::Index {
                        collection: Box::new(ident("xs")),
                        index: Box::new(Expr::LitInt(1)),
                    }),
                },
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(
            wgsl.contains("var<storage, read> pair_sum_xs: array<f32>;"),
            "{wgsl}"
        );
        assert!(wgsl.contains("pair_sum_xs[u32(i32(0))]"), "{wgsl}");
    }

    // ── buffer (array<T>) WRITE over a list[scalar] param (PMAT-979) ────

    #[test]
    fn list_param_index_write_lowers_to_storage_store_naga_validates() {
        // fn set_first(xs: list[int]) -> () { xs[0] = 7; }
        // The written list param flips its buffer to var<storage, read_write>.
        let f = Function {
            name: "set_first".into(),
            params: vec![param("xs", Type::List(Box::new(Type::I64)))],
            return_type: Type::Unit,
            body: Block {
                stmts: vec![Stmt::IndexAssign {
                    list_name: "xs".into(),
                    indices: vec![Expr::LitInt(0)],
                    value: Expr::LitInt(7),
                }],
                trailing_return: Expr::Unit,
            },
        };
        let wgsl = lower_and_naga(f);
        // Written param → read_write access mode, NOT read.
        assert!(
            wgsl.contains(
                "@group(0) @binding(0) var<storage, read_write> set_first_xs: array<i32>;"
            ),
            "{wgsl}"
        );
        // The store: i32 index narrows to u32, value matches element type.
        assert!(
            wgsl.contains("set_first_xs[u32(i32(0))] = i32(7);"),
            "{wgsl}"
        );
        assert!(wgsl.contains("fn set_first()"), "{wgsl}");
    }

    #[test]
    fn read_modify_write_kernel_in_while_loop_naga_validates() {
        // A real compute-kernel shape: read every element, double it, write
        // it back, over a counter-driven while loop.
        //
        // fn double_all(xs: list[f32], n: i32) -> () {
        //   var i: i32; i = 0;
        //   while (i < n) {
        //     xs[i] = xs[i] * 2.0;
        //     i = i + 1;
        //   }
        // }
        let f = Function {
            name: "double_all".into(),
            params: vec![
                param("xs", Type::List(Box::new(Type::F32))),
                param("n", Type::I64),
            ],
            return_type: Type::Unit,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "i".into(),
                        ty: Type::I64,
                        value: Expr::LitInt(0),
                        mutable: true,
                    },
                    Stmt::While {
                        cond: binop(BinOp::Lt, ident("i"), ident("n")),
                        body: vec![
                            Stmt::IndexAssign {
                                list_name: "xs".into(),
                                indices: vec![ident("i")],
                                value: Expr::FloatBinOp {
                                    op: FloatOp::Mul,
                                    lhs: Box::new(Expr::Index {
                                        collection: Box::new(ident("xs")),
                                        index: Box::new(ident("i")),
                                    }),
                                    rhs: Box::new(Expr::LitFloat(2.0)),
                                },
                            },
                            Stmt::Assign {
                                name: "i".into(),
                                value: binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                            },
                        ],
                    },
                ],
                trailing_return: Expr::Unit,
            },
        };
        let wgsl = lower_and_naga(f);
        // read_write buffer (it is both read AND written in the body).
        assert!(
            wgsl.contains("var<storage, read_write> double_all_xs: array<f32>;"),
            "{wgsl}"
        );
        // The store reads the same buffer on its RHS and writes the LHS.
        assert!(
            wgsl.contains("double_all_xs[u32(i)] = (double_all_xs[u32(i)] * f32(2.0));"),
            "{wgsl}"
        );
        // `n` is still a scalar fn param; `xs` is not.
        assert!(wgsl.contains("fn double_all(n: i32)"), "{wgsl}");
    }

    #[test]
    fn read_only_list_param_stays_read_access() {
        // A param only READ keeps `var<storage, read>` — the write pre-scan
        // must not flip a read-only buffer to read_write.
        let f = Function {
            name: "first".into(),
            params: vec![param("xs", Type::List(Box::new(Type::I64)))],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::Index {
                    collection: Box::new(ident("xs")),
                    index: Box::new(Expr::LitInt(0)),
                },
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(
            wgsl.contains("var<storage, read> first_xs: array<i32>;"),
            "{wgsl}"
        );
        assert!(!wgsl.contains("read_write"), "{wgsl}");
    }

    #[test]
    fn refuses_nested_multi_index_write() {
        // grid[i][j] = v has no flat-buffer lowering — honest refusal.
        let f = Function {
            name: "set2d".into(),
            params: vec![param("grid", Type::List(Box::new(Type::I64)))],
            return_type: Type::Unit,
            body: Block {
                stmts: vec![Stmt::IndexAssign {
                    list_name: "grid".into(),
                    indices: vec![Expr::LitInt(0), Expr::LitInt(1)],
                    value: Expr::LitInt(9),
                }],
                trailing_return: Expr::Unit,
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    #[test]
    fn refuses_index_write_type_mismatch() {
        // Storing an i32 into a list[f32] buffer is a type error → refusal.
        let f = Function {
            name: "bad".into(),
            params: vec![param("xs", Type::List(Box::new(Type::F32)))],
            return_type: Type::Unit,
            body: Block {
                stmts: vec![Stmt::IndexAssign {
                    list_name: "xs".into(),
                    indices: vec![Expr::LitInt(0)],
                    value: Expr::LitInt(1), // i32, not f32
                }],
                trailing_return: Expr::Unit,
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    // ── bitwise / unsigned / logical ────────────────────────────────────

    #[test]
    fn bitwise_and_logical_naga_validates() {
        // fn mask(a: u32, b: u32) -> u32 { return ((a & b) | a); }
        let f = Function {
            name: "mask".into(),
            params: vec![param("a", Type::CUInt), param("b", Type::CUInt)],
            return_type: Type::CUInt,
            body: Block {
                stmts: vec![],
                trailing_return: binop(
                    BinOp::BitOr,
                    binop(BinOp::BitAnd, ident("a"), ident("b")),
                    ident("a"),
                ),
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("fn mask(a: u32, b: u32) -> u32"), "{wgsl}");
        assert!(wgsl.contains("((a & b) | a)"), "{wgsl}");
    }

    #[test]
    fn short_circuit_and_or_naga_validates() {
        // fn both(p: bool, q: bool) -> bool { return (p && q); }
        let f = Function {
            name: "both".into(),
            params: vec![param("p", Type::Bool), param("q", Type::Bool)],
            return_type: Type::Bool,
            body: Block {
                stmts: vec![],
                trailing_return: binop(BinOp::And, ident("p"), ident("q")),
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("(p && q)"), "{wgsl}");
    }

    // ── HONEST REFUSALS — what stays unhandled is a clean Lower error ────

    #[test]
    fn refuses_f64_no_silent_narrowing() {
        // f64 has no WGSL core type; the lane refuses rather than narrow.
        let f = Function {
            name: "d".into(),
            params: vec![param("x", Type::F64)],
            return_type: Type::F64,
            body: Block {
                stmts: vec![],
                trailing_return: ident("x"),
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("f64"), "{msg}");
        assert!(msg.contains("WGSL core has no 64-bit float"), "{msg}");
    }

    #[test]
    fn refuses_string_param() {
        let f = Function {
            name: "s".into(),
            params: vec![param("x", Type::Str)],
            return_type: Type::Bool,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::LitBool(true),
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    #[test]
    fn refuses_signed_floordiv_pending_floor_correction() {
        // Signed `//` needs the Python floor correction (not yet lowered);
        // an honest refusal, NOT a silently-wrong truncating div.
        let f = Function {
            name: "fd".into(),
            params: vec![param("a", Type::I64), param("b", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: binop(BinOp::FloorDiv, ident("a"), ident("b")),
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    #[test]
    fn refuses_struct_item() {
        let m = module(vec![Item::Struct {
            name: "P".into(),
            fields: vec![("x".into(), Type::I64)],
            methods: vec![],
            frozen: false,
            order: false,
        }]);
        let err = emit_wgsl_module(&m).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    #[test]
    fn refuses_list_of_bool_element() {
        let f = Function {
            name: "lb".into(),
            params: vec![param("xs", Type::List(Box::new(Type::Bool)))],
            return_type: Type::Bool,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::LitBool(false),
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    #[test]
    fn naga_rejects_garbage_text() {
        // Sanity: the naga gate actually rejects malformed WGSL (so a
        // passing validation in the other tests is meaningful).
        assert!(naga_validate_wgsl("this is not wgsl {{{").is_err());
    }

    #[test]
    fn empty_module_refused() {
        let err = emit_wgsl_module(&module(vec![])).unwrap_err();
        assert!(matches!(err, BackendError::Lower(_)));
    }

    // ── PMAT-1391: reserved-prefix sanitizing + the emission-boundary gate ──

    /// The RED half, pinned against naga itself rather than asserted:
    /// these are exactly the identifier shapes WGSL forbids. If a future
    /// naga bump relaxes either rule this test fails loudly and the
    /// mangler's justification gets re-derived instead of assumed.
    #[test]
    fn naga_really_does_reject_the_shapes_wgsl_ident_exists_to_fix() {
        // `__`-prefixed — the frontend's `__forc0` / `__forstop1` /
        // `__broke0` desugar family.
        assert!(naga_validate_wgsl("fn __forc0() -> i32 { return 1; }").is_err());
        // bare `_`.
        assert!(naga_validate_wgsl("fn f() -> i32 { let _: i32 = 1; return _; }").is_err());
        // ...while a SINGLE leading underscore is legal, so the mangler
        // must not over-reach and rewrite `_foo`.
        assert!(naga_validate_wgsl("fn _foo() -> i32 { return 1; }").is_ok());
    }

    /// `wgsl_ident` is TOTAL (every output is a legal WGSL identifier)
    /// and INJECTIVE (no two distinct inputs collide). Injectivity is the
    /// load-bearing half: a naive `__forc0` → `forc0` would silently
    /// alias a user variable actually named `forc0`.
    #[test]
    fn wgsl_ident_is_legal_and_injective() {
        let inputs = [
            // frontend synthetics
            "__forc0",
            "__forstop1",
            "__broke0",
            "__fe0",
            "__feset0",
            "__unpack0",
            "__augi0",
            // ordinary user names — must be preserved verbatim
            "s",
            "i",
            "last_i",
            "forc0",
            "n",
            // user names that are THEMSELVES illegal WGSL
            "__x",
            "___y",
            // adversarial: names shaped like the mangler's own output
            "xpm",
            "xpm_u__forc0",
            "xpm_e_xpm",
            "_leading",
        ];

        let mut seen: Vec<(String, String)> = Vec::new();
        for input in inputs {
            let out = wgsl_ident(input);
            // TOTAL: the output is a legal WGSL identifier. Checked
            // against naga, not against a restatement of the rule.
            assert!(
                naga_validate_wgsl(&format!("fn {out}() -> i32 {{ return 1; }}")).is_ok(),
                "wgsl_ident({input:?}) = {out:?} is not a legal WGSL identifier"
            );
            // INJECTIVE.
            if let Some((prev_in, _)) = seen.iter().find(|(_, o)| *o == out) {
                panic!("wgsl_ident collision: {prev_in:?} and {input:?} both map to {out:?}");
            }
            seen.push((input.to_string(), out));
        }

        // The common case is the IDENTITY, so emitted WGSL stays readable.
        for plain in ["s", "i", "last_i", "forc0", "n"] {
            assert_eq!(wgsl_ident(plain), plain);
        }
        // ...and the collision the naive fix would have caused does not
        // happen: the synthetic and the user name stay distinct.
        assert_ne!(wgsl_ident("__forc0"), wgsl_ident("forc0"));
    }

    /// The production regression: a `for i in range(n)` loop desugars to
    /// `__forc0` / `__forstop1` locals. Before PMAT-1391 this emitted at
    /// exit 0 and the repo's OWN `naga_validate_wgsl` rejected the result
    /// with "Identifier starts with a reserved prefix: `__forc0`".
    #[test]
    fn desugared_loop_counter_names_are_sanitized_and_naga_validates() {
        // fn last_i(n) { var __forc0; __forc0 = 0; while (__forc0 < n) { __forc0 = __forc0 + 1 } return __forc0 }
        let f = Function {
            name: "last_i".into(),
            params: vec![param("n", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![
                    Stmt::Let {
                        name: "__forc0".into(),
                        ty: Type::I64,
                        value: Expr::LitInt(0),
                        mutable: true,
                    },
                    Stmt::While {
                        cond: binop(BinOp::Lt, ident("__forc0"), ident("n")),
                        body: vec![Stmt::Assign {
                            name: "__forc0".into(),
                            value: binop(BinOp::Add, ident("__forc0"), Expr::LitInt(1)),
                        }],
                    },
                ],
                trailing_return: ident("__forc0"),
            },
        };
        // `lower_and_naga` asserts the naga round-trip; with the gate now
        // inside `emit_wgsl_module` the `.expect` would already fire.
        let wgsl = lower_and_naga(f);
        // No emitted identifier starts with `__`. Checked over TOKENS so
        // the assertion cannot be satisfied by the substring appearing
        // mid-identifier (`xpm_u__forc0` legitimately contains `__`).
        for tok in wgsl.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            assert!(
                !tok.starts_with("__"),
                "emitted a reserved-prefix identifier {tok:?}:\n{wgsl}"
            );
        }
        // The USER-facing name survives untouched — the sanitizer is
        // surgical, not a blanket rename.
        assert!(wgsl.contains("fn last_i(n: i32)"), "{wgsl}");
        assert!(wgsl.contains("xpm_u__forc0"), "{wgsl}");
    }

    /// A user parameter literally named `__x` is EQUALLY illegal WGSL, so
    /// sanitizing only the frontend's known synthetics would be an
    /// incomplete fix. `wgsl_ident` keys on SHAPE, so this works too.
    #[test]
    fn user_written_double_underscore_param_is_sanitized_too() {
        let f = Function {
            name: "__k".into(),
            params: vec![param("__x", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: binop(BinOp::Add, ident("__x"), Expr::LitInt(1)),
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(wgsl.contains("fn xpm_u__k(xpm_u__x: i32)"), "{wgsl}");
    }

    /// A `list[scalar]` param binds a MODULE-SCOPE `var<storage>` whose
    /// name is the `<fn>_<param>` composite — a separate emission site
    /// with its own reserved-prefix exposure.
    #[test]
    fn storage_buffer_binding_name_is_sanitized() {
        let f = Function {
            name: "__k".into(),
            params: vec![
                param("xs", Type::List(Box::new(Type::I64))),
                param("i", Type::I64),
            ],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::Index {
                    collection: Box::new(ident("xs")),
                    index: Box::new(ident("i")),
                },
            },
        };
        let wgsl = lower_and_naga(f);
        assert!(
            wgsl.contains("var<storage, read> xpm_u__k_xs: array<i32>"),
            "{wgsl}"
        );
        assert!(wgsl.contains("xpm_u__k_xs[u32(i)]"), "{wgsl}");
    }

    /// The other half of the slice: the emission-boundary gate. A
    /// self-recursive function is legal meta-HIR (and legal Rust/WASM)
    /// but ILLEGAL WGSL — naga rejects `declaration of `f` is recursive`.
    /// Before PMAT-1391 `emit_wgsl_module` returned Ok and the CLI exited
    /// 0 handing the user WGSL no adapter would accept.
    #[test]
    fn recursive_function_is_an_honest_refusal_not_a_silent_bad_emit() {
        let f = Function {
            name: "f".into(),
            params: vec![param("n", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::Call {
                    callee: "f".into(),
                    args: vec![ident("n")],
                },
            },
        };
        let err = emit_wgsl_module(&module(vec![Item::Function(f)])).unwrap_err();
        let BackendError::Lower(msg) = &err else {
            panic!("expected BackendError::Lower, got {err:?}");
        };
        // The refusal must NAME the construct — an opaque "lowering
        // failed" would be a regression in a different direction.
        assert!(
            msg.contains("recursive"),
            "refusal should name why naga rejected it: {msg}"
        );
    }

    /// The gate must not be a no-op that greens everything: a NON-
    /// recursive call through the same `Expr::Call` path still emits.
    /// Without this, the test above would also pass if `Expr::Call` had
    /// simply started refusing outright.
    #[test]
    fn non_recursive_call_still_lowers() {
        let callee = Function {
            name: "helper".into(),
            params: vec![param("a", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: binop(BinOp::Add, ident("a"), Expr::LitInt(1)),
            },
        };
        let caller = Function {
            name: "top".into(),
            params: vec![param("n", Type::I64)],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::Call {
                    callee: "helper".into(),
                    args: vec![ident("n")],
                },
            },
        };
        let wgsl = emit_wgsl_module(&module(vec![
            Item::Function(callee),
            Item::Function(caller),
        ]))
        .expect("a non-recursive intra-module call is in the subset");
        naga_validate_wgsl(&wgsl).expect("should validate");
        assert!(wgsl.contains("return helper(n);"), "{wgsl}");
    }

    /// PMAT-1404 — the `list[…]` element site refuses `CLong` for the same
    /// reason the scalar site does.
    ///
    /// **This arm is not frontend-reachable today, and saying so is the
    /// point.** `decy-frontend` cannot parse a subscript at all (`int f(long*
    /// xs) { return xs[0]; }` fails with "unexpected character `[` in C
    /// source"), and the Python frontend never produces `Type::CLong` — so no
    /// CLI path currently delivers a `list[CLong]` here. The CLI-level sweep in
    /// `crates/xpile/tests/c_long_gpu_width_witness.rs` therefore does NOT
    /// cover this site, and this unit test over hand-built meta-HIR is what
    /// does. It guards the fix against the day a frontend does reach it,
    /// rather than pretending the CLI already exercises it.
    #[test]
    fn list_of_clong_is_refused_at_the_element_site() {
        let err = map_list_elem_type(&Type::CLong)
            .expect_err("list[CLong] narrows every element to i32 and must refuse");
        let msg = format!("{err}");
        assert!(
            msg.contains("list element type"),
            "the refusal must name the POSITION so it is distinguishable from \
             the scalar one: {msg}"
        );
        assert!(
            msg.contains("64-bit"),
            "the refusal must name the WIDTH as the reason: {msg}"
        );
        // The accept side, so this is not satisfiable by refusing every list.
        assert_eq!(
            map_list_elem_type(&Type::I64).expect("list[int] is in the subset"),
            WgslTy::I32
        );
    }
}
