//! WebAssembly text (WAT) frontend — the LIFT half of first-class
//! bidirectional native WASM (PMAT-954, the inverse of the
//! `Target::Wasm` emit half PMAT-951).
//!
//! Lifts the **WAT scalar/control subset** — specifically the image of
//! [`xpile-wasm-codegen`] — back to canonical meta-HIR via a
//! stack→expression-tree reconstruction. This is a **lossy
//! decompilation**, the honest other side of the asymmetry recorded in
//! `project-bidirectional-wasm`: emit is clean, lift is lossy.
//!
//! ## What the lift recovers
//!
//! - The `(module …)` skeleton + its `;; source module: <name>` comment.
//! - Each user `(func $name (param …) (result …) (local …) <body>)`:
//!   the signature (params → [`Param`], result → return [`Type`]), the
//!   `(local …)` declarations, and a **straight-line** body reconstructed
//!   by simulating the WASM operand stack:
//!     * `local.get $x` → [`Expr::Ident`]; `local.set $x` → a
//!       [`Stmt::Let`] (first write of a declared local) or
//!       [`Stmt::Assign`].
//!     * `i64.const` → [`Expr::LitInt`], `f64.const` → [`Expr::LitFloat`],
//!       `i32.const` → [`Expr::LitBool`] (0/1, the emit's bool encoding).
//!     * `i64.*` / `f64.*` / `i32.*` arithmetic & comparison ops → the
//!       matching [`BinOp`] / [`FloatOp`].
//!     * `call $__wasm_floordiv_i64` / `$__wasm_floormod_i64` → the
//!       Python floor [`BinOp::FloorDiv`] / [`BinOp::Mod`] (the emit's
//!       helper calls, lifted back to the high-level op).
//!     * `call $f` → an intra-module [`Expr::Call`] (arity from the
//!       callee's parsed signature).
//!
//!   The single value left on the stack is the [`Block::trailing_return`].
//!
//! ## What the lift loses / refuses (the honest lossy posture)
//!
//! - **Type identity collapses to the canonical scalar:** `i64`→`I64`
//!   (a `CLong` is indistinguishable), `i32`→`Bool` (a `CUInt` / a raw
//!   bool flag are indistinguishable), `f64`→`F64`, `f32`→`F32`. The
//!   high-level Python/Rust type is irreversibly gone.
//! - **Names** survive only because the emit kept them (`$x`); a stripped
//!   WAT would lose them.
//! - **Structured control-flow recovery** (`if`/`else`/`end`,
//!   `(block …)`, `(loop …)`, `br`/`br_if`, `return`, `drop`, `i32.eqz`,
//!   `i64.div_s`/`rem_s`) is **refused** at this first cut with a hard
//!   [`FrontendError::Lower`] — never a wrong lift. Reconstructing
//!   `if`/`while` from the stack-machine block/branch form is deferred to
//!   PMAT-952 (alongside the `WasmDiffExecEngine` runtime witness). The
//!   emit's synthetic `$__wasm_floordiv_i64`/`$__wasm_floormod_i64`
//!   helpers (which DO contain control flow) are skipped, not parsed.
//!
//! ## Correctness witness
//!
//! The lift is a **right-inverse of emit on its WAT image** — pinned by
//! an executed round-trip fixed-point test in `tests.rs`:
//! `emit(lift(emit(M))) == emit(M)` for every straight-line scalar
//! fixture. (A full `lift(emit(M)) == M` is *not* claimed — the type
//! collapse above makes the lift lossy; the fixed point is the honest,
//! checkable invariant.)

use std::collections::HashMap;
use std::path::Path;

use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Type,
};

/// WAT frontend. Lifts the WAT scalar/control subset (the
/// `xpile-wasm-codegen` image) to meta-HIR.
#[derive(Default)]
pub struct WasmFrontend;

impl WasmFrontend {
    pub fn new() -> Self {
        Self
    }
}

impl Frontend for WasmFrontend {
    fn name(&self) -> &'static str {
        "wasm"
    }

    fn extensions(&self) -> &[&'static str] {
        &["wat"]
    }

    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError> {
        let fallback = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("wasm_module");
        let name = recover_module_name(source, fallback);
        lift_wat(&name, source)
    }
}

/// Lift a WAT source string to a meta-HIR [`Module`] tagged
/// [`SourceLang::Wasm`]. Exposed for the round-trip witness (which has no
/// `Path`).
pub fn lift_wat(module_name: &str, source: &str) -> Result<Module, FrontendError> {
    let toks = tokenize(source);
    let mut i = 0usize;
    expect(&toks, &mut i, "(")?;
    expect(&toks, &mut i, "module")?;

    // Split the module body into top-level `(func …)` slices (skipping
    // `(export …)` directives and refusing anything else).
    let mut func_spans: Vec<(usize, usize)> = Vec::new(); // inclusive [open, close]
    loop {
        let t = peek(&toks, i)?;
        if t == ")" {
            break; // module close
        }
        if t != "(" {
            return Err(FrontendError::Parse(format!(
                "expected `(` or `)` in module body, found `{t}`"
            )));
        }
        let close = matching_paren(&toks, i)?;
        let kw = peek(&toks, i + 1)?;
        match kw.as_str() {
            "func" => func_spans.push((i, close)),
            "export" => { /* re-derived from the function on re-emit; skip */ }
            other => {
                return Err(FrontendError::Lower(format!(
                    "WAT top-level `({other} …)` is outside the lift subset \
                     (only `(func …)` / `(export …)`; memory/import/table/global \
                     decompilation is deferred to PMAT-952)"
                )));
            }
        }
        i = close + 1;
    }

    // Pass 1: arity map (name → param count) over EVERY func, so a
    // `call $f` knows how many operands to pop — including forward
    // references and the synthetic helpers.
    let mut arity: HashMap<String, usize> = HashMap::new();
    for &(open, close) in &func_spans {
        let (name, n_params) = parse_func_arity(&toks[open..=close])?;
        arity.insert(name, n_params);
    }

    // Pass 2: lift each USER func (skip the synthetic `$__wasm_*` helpers,
    // whose bodies carry control flow we intentionally do not parse).
    let mut items = Vec::new();
    for &(open, close) in &func_spans {
        let slice = &toks[open..=close];
        let raw_name = func_name(slice)?;
        if raw_name.starts_with("__wasm_") {
            continue;
        }
        items.push(Item::Function(lift_function(slice, &arity)?));
    }

    Ok(Module {
        name: module_name.to_string(),
        source_lang: SourceLang::Wasm,
        items,
        ffi_boundaries: Vec::new(),
    })
}

// ─── Module-name recovery ───────────────────────────────────────────

/// Recover the original module name from the emit's
/// `;; source module: <name>` comment (which `tokenize` strips), so the
/// lifted module re-emits an identical header. Falls back to the file
/// stem when absent.
fn recover_module_name(src: &str, fallback: &str) -> String {
    for line in src.lines() {
        if let Some(rest) = line.trim_start().strip_prefix(";; source module:") {
            let n = rest.trim();
            if !n.is_empty() {
                return n.to_string();
            }
        }
    }
    fallback.to_string()
}

// ─── Tokenizer ──────────────────────────────────────────────────────

/// Tokenize WAT into atoms, with `(` / `)` as standalone tokens and
/// `;; …`-to-EOL line comments stripped. xpile's emit is one instruction
/// per line with whitespace-delimited atoms, so this flat tokenizer is
/// sufficient for the lift subset (float atoms like `2.0` / `-inf` /
/// `1e30` stay single tokens).
fn tokenize(src: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // `;;` line comment → skip to end of line.
            ';' if chars.peek() == Some(&';') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            '(' | ')' => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
                toks.push(c.to_string());
            }
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

// ─── Token cursor helpers ───────────────────────────────────────────

fn peek(toks: &[String], i: usize) -> Result<&String, FrontendError> {
    toks.get(i)
        .ok_or_else(|| FrontendError::Parse("unexpected end of WAT input".to_string()))
}

fn expect(toks: &[String], i: &mut usize, want: &str) -> Result<(), FrontendError> {
    let got = peek(toks, *i)?;
    if got != want {
        return Err(FrontendError::Parse(format!(
            "expected `{want}`, found `{got}`"
        )));
    }
    *i += 1;
    Ok(())
}

/// Index of the `)` matching the `(` at `open`.
fn matching_paren(toks: &[String], open: usize) -> Result<usize, FrontendError> {
    let mut depth = 0usize;
    for (k, t) in toks.iter().enumerate().skip(open) {
        match t.as_str() {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    return Ok(k);
                }
            }
            _ => {}
        }
    }
    Err(FrontendError::Parse(
        "unbalanced parentheses in WAT".to_string(),
    ))
}

/// Strip a leading `$` from a WAT identifier (meta-HIR names have none).
fn ident(tok: &str) -> &str {
    tok.strip_prefix('$').unwrap_or(tok)
}

// ─── Function header parsing ────────────────────────────────────────

/// The raw (`$`-stripped) name of a `(func $name …)` slice.
fn func_name(slice: &[String]) -> Result<String, FrontendError> {
    // slice = [ "(", "func", "$name", … , ")" ]
    let n = slice
        .get(2)
        .ok_or_else(|| FrontendError::Parse("`(func` missing a name".to_string()))?;
    Ok(ident(n).to_string())
}

/// Parse just the name + parameter count of a func slice (Pass 1).
fn parse_func_arity(slice: &[String]) -> Result<(String, usize), FrontendError> {
    let name = func_name(slice)?;
    let mut n = 0usize;
    let mut k = 3; // after "(", "func", "$name"
    let end = slice.len() - 1; // final ")"
    while k < end {
        if slice[k] == "(" && slice.get(k + 1).map(String::as_str) == Some("param") {
            n += 1;
            k = local_matching(slice, k)? + 1;
        } else if slice[k] == "(" {
            // result / local / control sub-expr — not a param; skip it.
            k = local_matching(slice, k)? + 1;
        } else {
            // First body instruction — params are all before the body.
            break;
        }
    }
    Ok((name, n))
}

/// `matching_paren` relative to a slice.
fn local_matching(slice: &[String], open: usize) -> Result<usize, FrontendError> {
    matching_paren(slice, open)
}

/// Map a WAT value type keyword to its canonical meta-HIR [`Type`]. This
/// is the lossy direction: the emit collapses several meta types onto
/// each WAT type, so the lift picks the canonical representative.
fn map_wat_type(kw: &str) -> Result<Type, FrontendError> {
    match kw {
        "i64" => Ok(Type::I64),
        // The emit lowers BOTH `Bool` and `CUInt` to i32; `Bool` is the
        // canonical representative (the dominant i32 source — comparisons,
        // bool literals). Re-emits as i32 either way, so the fixed point
        // holds.
        "i32" => Ok(Type::Bool),
        "f64" => Ok(Type::F64),
        "f32" => Ok(Type::F32),
        other => Err(FrontendError::Lower(format!(
            "WAT value type `{other}` is outside the lift subset \
             (only i64/i32/f64/f32)"
        ))),
    }
}

/// Fully lift one user `(func …)` slice to a meta-HIR [`Function`].
fn lift_function(
    slice: &[String],
    arity: &HashMap<String, usize>,
) -> Result<Function, FrontendError> {
    let name = func_name(slice)?;
    let end = slice.len() - 1; // final ")"
    let mut k = 3; // after "(", "func", "$name"

    let mut params: Vec<Param> = Vec::new();
    let mut return_type = Type::Unit;
    let mut locals: Vec<(String, Type)> = Vec::new();
    let mut local_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Header: zero+ (param …), optional (result …), zero+ (local …).
    while k < end {
        if slice[k] != "(" {
            break; // body begins
        }
        let close = local_matching(slice, k)?;
        let kw = slice
            .get(k + 1)
            .ok_or_else(|| FrontendError::Parse("empty `(` form in func header".to_string()))?;
        match kw.as_str() {
            "param" => {
                // ( param $name ty )
                let pname = ident(peek_slice(slice, k + 2)?);
                let ty = map_wat_type(peek_slice(slice, k + 3)?)?;
                params.push(Param {
                    name: pname.to_string(),
                    ty,
                    mutable: false,
                });
            }
            "result" => {
                // ( result ty )
                return_type = map_wat_type(peek_slice(slice, k + 2)?)?;
            }
            "local" => {
                // ( local $name ty )
                let lname = ident(peek_slice(slice, k + 2)?).to_string();
                let ty = map_wat_type(peek_slice(slice, k + 3)?)?;
                local_names.insert(lname.clone());
                locals.push((lname, ty));
            }
            "block" | "loop" | "if" => {
                return Err(refuse_control(kw));
            }
            other => {
                return Err(FrontendError::Lower(format!(
                    "unexpected `({other} …)` in func `{name}` header"
                )));
            }
        }
        k = close + 1;
    }

    // Body: a flat instruction stream [k, end). Reconstruct via the stack.
    let body_toks = &slice[k..end];
    let set_counts = count_local_sets(body_toks);
    let local_ty: HashMap<&str, &Type> = locals.iter().map(|(n, t)| (n.as_str(), t)).collect();

    let mut stack: Vec<Expr> = Vec::new();
    let mut stmts: Vec<xpile_meta_hir::Stmt> = Vec::new();
    let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut j = 0usize;
    while j < body_toks.len() {
        let instr = body_toks[j].as_str();
        match instr {
            "local.get" => {
                let n = ident(peek_slice(body_toks, j + 1)?);
                stack.push(Expr::Ident(n.to_string()));
                j += 2;
            }
            "local.set" => {
                let n = ident(peek_slice(body_toks, j + 1)?).to_string();
                let value = pop(&mut stack, instr)?;
                if local_names.contains(&n) && !assigned.contains(&n) {
                    // First write of a declared local → a `let`. Mutable
                    // iff it is written again later in the body.
                    let ty = (*local_ty
                        .get(n.as_str())
                        .ok_or_else(|| FrontendError::Parse(format!("unknown local `{n}`")))?)
                    .clone();
                    let mutable = set_counts.get(&n).copied().unwrap_or(0) > 1;
                    stmts.push(xpile_meta_hir::Stmt::Let {
                        name: n.clone(),
                        ty,
                        value,
                        mutable,
                    });
                    assigned.insert(n);
                } else {
                    stmts.push(xpile_meta_hir::Stmt::Assign { name: n, value });
                }
                j += 2;
            }
            "i64.const" => {
                let v: i64 = peek_slice(body_toks, j + 1)?
                    .parse()
                    .map_err(|_| FrontendError::Parse("bad i64.const literal".to_string()))?;
                stack.push(Expr::LitInt(v));
                j += 2;
            }
            "i32.const" => {
                let v: i64 = peek_slice(body_toks, j + 1)?
                    .parse()
                    .map_err(|_| FrontendError::Parse("bad i32.const literal".to_string()))?;
                // In the emit image an i32.const is the 0/1 bool encoding.
                stack.push(Expr::LitBool(v != 0));
                j += 2;
            }
            "f64.const" => {
                let tok = peek_slice(body_toks, j + 1)?;
                let v: f64 = tok
                    .parse()
                    .map_err(|_| FrontendError::Parse(format!("bad f64.const literal `{tok}`")))?;
                stack.push(Expr::LitFloat(v));
                j += 2;
            }
            "call" => {
                let callee = ident(peek_slice(body_toks, j + 1)?).to_string();
                match callee.as_str() {
                    // The emit's Python floor helpers, lifted back to the op.
                    "__wasm_floordiv_i64" => {
                        let (lhs, rhs) = pop2(&mut stack, instr)?;
                        stack.push(Expr::BinOp {
                            op: BinOp::FloorDiv,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        });
                    }
                    "__wasm_floormod_i64" => {
                        let (lhs, rhs) = pop2(&mut stack, instr)?;
                        stack.push(Expr::BinOp {
                            op: BinOp::Mod,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        });
                    }
                    _ => {
                        let n = *arity.get(&callee).ok_or_else(|| {
                            FrontendError::Lower(format!(
                                "call to unknown function `{callee}` (no parsed signature)"
                            ))
                        })?;
                        let mut args = Vec::with_capacity(n);
                        for _ in 0..n {
                            args.push(pop(&mut stack, instr)?);
                        }
                        args.reverse();
                        stack.push(Expr::Call { callee, args });
                    }
                }
                j += 2;
            }
            // Binary ops — pop rhs then lhs (lhs was pushed first).
            other => {
                if let Some(op) = int_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    j += 1;
                } else if let Some(fop) = float_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    j += 1;
                } else {
                    return Err(refuse_control(other));
                }
            }
        }
    }

    // The single residual value is the trailing return (or unit/void).
    let trailing_return = if matches!(return_type, Type::Unit) {
        if !stack.is_empty() {
            return Err(FrontendError::Lower(format!(
                "void function `{name}` left {} value(s) on the stack",
                stack.len()
            )));
        }
        Expr::Unit
    } else {
        if stack.len() != 1 {
            return Err(FrontendError::Lower(format!(
                "function `{name}` did not reconstruct to a single trailing value \
                 (stack depth {}) — likely outside the straight-line scalar subset",
                stack.len()
            )));
        }
        stack.pop().unwrap()
    };

    Ok(Function {
        name,
        params,
        return_type,
        body: Block {
            stmts,
            trailing_return,
        },
    })
}

/// A refusal for any control-flow / non-scalar-subset instruction — the
/// honest lossy boundary (structured recovery deferred to PMAT-952).
fn refuse_control(instr: &str) -> FrontendError {
    FrontendError::Lower(format!(
        "WAT instruction `{instr}` is outside the lift subset — the lift handles \
         the straight-line scalar subset only; structured control-flow recovery \
         (`if`/`block`/`loop`/`br`/`return`/`drop`/`i32.eqz`/`i64.div_s`) is \
         deferred to PMAT-952"
    ))
}

fn peek_slice(slice: &[String], i: usize) -> Result<&str, FrontendError> {
    slice
        .get(i)
        .map(String::as_str)
        .ok_or_else(|| FrontendError::Parse("unexpected end of WAT form".to_string()))
}

fn pop(stack: &mut Vec<Expr>, instr: &str) -> Result<Expr, FrontendError> {
    stack
        .pop()
        .ok_or_else(|| FrontendError::Parse(format!("operand stack underflow at `{instr}`")))
}

/// Pop two operands, returning (lhs, rhs) in source order (rhs is on top).
fn pop2(stack: &mut Vec<Expr>, instr: &str) -> Result<(Expr, Expr), FrontendError> {
    let rhs = pop(stack, instr)?;
    let lhs = pop(stack, instr)?;
    Ok((lhs, rhs))
}

/// Count `local.set $name` occurrences per name in a body token slice
/// (drives the `let mut` decision for a reassigned local).
fn count_local_sets(body: &[String]) -> HashMap<String, usize> {
    let mut m: HashMap<String, usize> = HashMap::new();
    let mut j = 0;
    while j < body.len() {
        if body[j] == "local.set" {
            if let Some(n) = body.get(j + 1) {
                *m.entry(ident(n).to_string()).or_insert(0) += 1;
            }
            j += 2;
        } else {
            j += 1;
        }
    }
    m
}

/// Map an i64/i32 WAT binary-op mnemonic to its meta-HIR [`BinOp`]. The
/// inverse of `xpile-wasm-codegen`'s `emit_binop` instruction table.
fn int_binop(instr: &str) -> Option<BinOp> {
    Some(match instr {
        "i64.add" => BinOp::Add,
        "i64.sub" => BinOp::Sub,
        "i64.mul" => BinOp::Mul,
        "i64.and" => BinOp::BitAnd,
        "i64.or" => BinOp::BitOr,
        "i64.xor" => BinOp::BitXor,
        "i64.shl" => BinOp::Shl,
        "i64.shr_s" => BinOp::Shr,
        "i64.eq" | "f64.eq" | "i32.eq" => BinOp::Eq,
        "i64.ne" | "f64.ne" | "i32.ne" => BinOp::NotEq,
        "i64.lt_s" | "f64.lt" => BinOp::Lt,
        "i64.le_s" | "f64.le" => BinOp::LtEq,
        "i64.gt_s" | "f64.gt" => BinOp::Gt,
        "i64.ge_s" | "f64.ge" => BinOp::GtEq,
        _ => return None,
    })
}

/// Map an f64 WAT arithmetic mnemonic to its meta-HIR [`FloatOp`].
/// (f64 *comparisons* lift to a [`BinOp`] via [`int_binop`], matching the
/// emit, which routes them through `Expr::BinOp`.)
fn float_binop(instr: &str) -> Option<FloatOp> {
    Some(match instr {
        "f64.add" => FloatOp::Add,
        "f64.sub" => FloatOp::Sub,
        "f64.mul" => FloatOp::Mul,
        "f64.div" => FloatOp::Div,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
