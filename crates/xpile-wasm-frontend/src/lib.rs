//! WebAssembly text (WAT) frontend — the LIFT half of first-class
//! bidirectional native WASM (PMAT-954, the inverse of the
//! `Target::Wasm` emit half PMAT-951).
//!
//! Lifts the **WAT scalar/control subset** — specifically the image of
//! [`xpile-wasm-codegen`] — back to canonical meta-HIR via a
//! stack→expression-tree reconstruction with **structured control-flow
//! recovery** (PMAT-959, the control half of PMAT-952). This is a **lossy
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
//!       `i32.const 0`/`i32.const 1` → [`Expr::LitBool`] (the emit's bool
//!       encoding). Any OTHER `i32.const` literal is **refused**, not
//!       folded — see the lossy-posture section below.
//!     * `i64.*` / `f64.*` / `i32.*` arithmetic & comparison ops → the
//!       matching [`BinOp`] / [`FloatOp`].
//!     * `call $__wasm_floordiv_i64` / `$__wasm_floormod_i64` → the
//!       Python floor [`BinOp::FloorDiv`] / [`BinOp::Mod`] (the emit's
//!       helper calls, lifted back to the high-level op).
//!     * `call $__wasm_add_i64` / `$__wasm_sub_i64` / `$__wasm_mul_i64`
//!       (PMAT-1402) and `$__wasm_shl_i64` / `$__wasm_shr_i64` (PMAT-1379,
//!       whose arms this slice supplies) → [`BinOp::Add`] / [`Sub`] /
//!       [`Mul`] / [`Shl`] / [`Shr`]. EVERY helper the emit routes an
//!       operator through needs an arm here or the right-inverse property
//!       below is false for any module using that operator — which it
//!       silently was for `<<`/`>>` between PMAT-1379 and PMAT-1402.
//!
//!       [`Sub`]: BinOp::Sub
//!       [`Mul`]: BinOp::Mul
//!       [`Shl`]: BinOp::Shl
//!       [`Shr`]: BinOp::Shr
//!     * `call $f` → an intra-module [`Expr::Call`] (arity from the
//!       callee's parsed signature).
//!
//!   The single value left on the stack is the [`Block::trailing_return`].
//!
//! ## Structured control-flow recovery (PMAT-959)
//!
//! The lift now inverts the **canonical control shapes** the emit produces,
//! recursively (the right-inverse-on-image property still holds — it only
//! needs to invert what `xpile-wasm-codegen` emits, not arbitrary WASM):
//!
//!   * `(block $brk (loop $cont <cond> i32.eqz br_if $brk <body> br $cont))`
//!     → [`Stmt::While`] — the `i32.eqz`+`br_if $brk` guard is stripped to
//!     recover the un-negated loop condition; the trailing `br $cont`
//!     back-edge closes the body.
//!   * `if <then-stmts> [else <else-stmts>] end` (no `(result …)`) →
//!     [`Stmt::If`] (the decy-style statement-if shape).
//!   * `if (result T) <then-expr> else <else-expr> end` → [`Expr::IfExpr`].
//!   * `return` → [`Stmt::Return`] (popping the residual value, if any).
//!   * `br $brk` → [`Stmt::Break`]; `br $cont` → [`Stmt::Continue`].
//!   * Nested control recurses.
//!
//! ## What the lift loses / refuses (the honest lossy posture)
//!
//! - **Type identity collapses to the canonical scalar:** `i64`→`I64`
//!   (a `CLong` is indistinguishable), `i32`→`Bool` (a `CUInt` / a raw
//!   bool flag are indistinguishable), `f64`→`F64`, `f32`→`F32`. The
//!   high-level Python/Rust type is irreversibly gone.
//! - **`i32.const` outside `{0, 1}` is REFUSED** (PMAT-1392). In the emit
//!   image an `i32` IS the 0/1 bool encoding and an integer literal is an
//!   `i64`, so there is no meta-HIR representative for `i32.const 2`. Until
//!   PMAT-1392 the lift folded every nonzero literal to `true`, so
//!   `i32.const 2` re-emitted as `i32.const 1` — valid WAT that `wat2wasm`
//!   accepts and `wasm-interp` runs to a DIFFERENT value (2 → 1; `-5` → 1
//!   against a reference of 4294967291), at exit 0 on every leg. Refusing is
//!   the honest boundary; `.wat` is an advertised frontend, so hand-written
//!   and third-party WAT reach this path. RESIDUAL, deliberately NOT fixed
//!   here: the `i32` → [`Type::Bool`] *type* mapping below still renders a
//!   hand-written `(param $a i32)` as `bool` on `--target rust`. That one is
//!   a genuine emit-image ambiguity (`Bool` and `CUInt` both lower to `i32`),
//!   not a value corruption — the WAT round trip stays value-preserving.
//! - **Names** survive only because the emit kept them (`$x`); a stripped
//!   WAT would lose them.
//! - **Non-canonical control flow** — any block/loop/branch nesting OUTSIDE
//!   the `xpile-wasm-codegen` image (e.g. a `(block …)` whose label is not
//!   `$brk`, a `br_table`, a raw stack-machine branch xpile never emits) —
//!   is still **refused** with a hard [`FrontendError::Lower`], never a
//!   wrong lift. The emit's synthetic `$__wasm_floordiv_i64`/
//!   `$__wasm_floormod_i64` helpers (which DO contain an inner `(if …)`)
//!   are skipped wholesale, not parsed.
//!
//! ## Correctness witness
//!
//! The lift is a **right-inverse of emit on its WAT image** — pinned by
//! executed round-trip fixed-point tests in `tests.rs`:
//! `emit(lift(emit(M))) == emit(M)` for every straight-line scalar AND
//! structured-control fixture (a `while` sum, an `if`/`else` max, an
//! if-expr, and a nested loop+if). (A full `lift(emit(M)) == M` is *not*
//! claimed — the type collapse above makes the lift lossy; the fixed point
//! is the honest, checkable invariant.)

use std::collections::HashMap;
use std::path::Path;

use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Stmt, Type,
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
            // Anything else is the first BODY construct (e.g. a `while`
            // loop emitted as `(block $brk (loop $cont …))` as the very
            // first statement) — the header is over, stop and let the
            // structured body lifter take it (PMAT-959).
            _ => break,
        }
        k = close + 1;
    }

    // Body: an instruction stream [k, end), now WITH structured control
    // flow (PMAT-959). Reconstruct via a recursive lifter that simulates
    // the operand stack AND recovers the canonical control shapes the emit
    // produces (`(block $brk (loop $cont …))` → `While`, bare
    // `if … else … end` → `Stmt::If`, `if (result T) …` → `Expr::IfExpr`,
    // `return` → `Stmt::Return`, `br $brk`/`br $cont` → `Break`/`Continue`).
    let body_toks = &slice[k..end];
    let set_counts = count_local_sets(body_toks);
    let local_ty: HashMap<String, Type> =
        locals.iter().map(|(n, t)| (n.clone(), t.clone())).collect();

    let mut ctx = BodyCtx {
        arity,
        local_names: &local_names,
        local_ty: &local_ty,
        set_counts: &set_counts,
        assigned: std::collections::HashSet::new(),
    };

    // Lift the whole function body as a top-level block (terminated by the
    // end of the slice). Any residual operand-stack values are the trailing
    // return; any structural mismatch is a hard refusal (never a wrong lift).
    let (stmts, mut stack) = lift_block(body_toks, &mut ctx, BlockEnd::Eof)?;

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

// ─── Structured body lifter (PMAT-959) ──────────────────────────────
//
// A recursive descent over the WAT body token stream that simulates the
// operand stack AND recovers the canonical control shapes the emit
// produces. It is a *right-inverse on the emit image*: it inverts exactly
// what `xpile-wasm-codegen` emits, refusing anything else.

/// Shared lowering context threaded through the recursive body lifter.
struct BodyCtx<'a> {
    /// name → param count, for `call $f` arity.
    arity: &'a HashMap<String, usize>,
    /// Declared `(local …)` names (drives Let-vs-Assign).
    local_names: &'a std::collections::HashSet<String>,
    /// local name → its lifted [`Type`] (for the `Let`'s annotation).
    local_ty: &'a HashMap<String, Type>,
    /// `local.set` count per name over the WHOLE body (drives `let mut`).
    set_counts: &'a HashMap<String, usize>,
    /// Names already bound by a `Let` (a later set is an `Assign`).
    assigned: std::collections::HashSet<String>,
}

/// What terminates the block currently being lifted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockEnd {
    /// End of the function body slice (the top-level block).
    Eof,
    /// An `else` or `end` keyword (an `if`/`else` arm).
    ElseOrEnd,
    /// The matching `)` of a `(loop …)` form (a `while` body).
    Loop,
}

/// Outcome of consuming one block: its statements and the residual operand
/// stack, plus the terminator keyword actually seen (so an `if` lifter can
/// tell `else` from `end`).
struct BlockOut {
    stmts: Vec<Stmt>,
    stack: Vec<Expr>,
    /// The terminator token (`"else"`, `"end"`, or `""` for EOF/loop-close).
    term: String,
}

/// Lift a block of body tokens, returning its statements and residual stack.
/// (Convenience wrapper used at the function-body top level.)
fn lift_block(
    toks: &[String],
    ctx: &mut BodyCtx<'_>,
    end: BlockEnd,
) -> Result<(Vec<Stmt>, Vec<Expr>), FrontendError> {
    let out = lift_block_inner(toks, &mut 0usize, ctx, end)?;
    Ok((out.stmts, out.stack))
}

/// The core recursive lifter. Consumes tokens from `*pos` until the block
/// terminator for `end` is reached, simulating the operand stack and
/// emitting statements for the control shapes / `local.set` / `return`.
fn lift_block_inner(
    toks: &[String],
    pos: &mut usize,
    ctx: &mut BodyCtx<'_>,
    end: BlockEnd,
) -> Result<BlockOut, FrontendError> {
    let mut stack: Vec<Expr> = Vec::new();
    let mut stmts: Vec<Stmt> = Vec::new();

    while *pos < toks.len() {
        let instr = toks[*pos].as_str();
        match instr {
            // ── block terminators ──
            "else" | "end" if end == BlockEnd::ElseOrEnd => {
                let term = instr.to_string();
                *pos += 1;
                return Ok(BlockOut { stmts, stack, term });
            }
            // A `(loop …)` body terminates at the loop form's closing `)`.
            ")" if end == BlockEnd::Loop => {
                *pos += 1;
                return Ok(BlockOut {
                    stmts,
                    stack,
                    term: String::new(),
                });
            }
            // ── operand-producing leaves ──
            "local.get" => {
                let n = ident(peek_slice(toks, *pos + 1)?);
                stack.push(Expr::Ident(n.to_string()));
                *pos += 2;
            }
            "local.set" => {
                let n = ident(peek_slice(toks, *pos + 1)?).to_string();
                let value = pop(&mut stack, instr)?;
                stmts.push(lift_local_set(n, value, ctx)?);
                *pos += 2;
            }
            "i64.const" => {
                let v: i64 = peek_slice(toks, *pos + 1)?
                    .parse()
                    .map_err(|_| FrontendError::Parse("bad i64.const literal".to_string()))?;
                stack.push(Expr::LitInt(v));
                *pos += 2;
            }
            "i32.const" => {
                stack.push(lift_i32_const(peek_slice(toks, *pos + 1)?, "")?);
                *pos += 2;
            }
            "f64.const" => {
                let tok = peek_slice(toks, *pos + 1)?;
                let v: f64 = tok
                    .parse()
                    .map_err(|_| FrontendError::Parse(format!("bad f64.const literal `{tok}`")))?;
                stack.push(Expr::LitFloat(v));
                *pos += 2;
            }
            "call" => {
                let callee = ident(peek_slice(toks, *pos + 1)?).to_string();
                lift_call(callee, &mut stack, ctx)?;
                *pos += 2;
            }
            // ── structured control (the PMAT-959 recovery) ──
            "return" => {
                // The single residual value (if the fn is non-void) is the
                // returned expression; a void return takes nothing.
                let value = if stack.is_empty() {
                    Expr::Unit
                } else {
                    pop(&mut stack, instr)?
                };
                stmts.push(Stmt::Return(value));
                *pos += 1;
            }
            "br" => {
                // `br $brk` → break; `br $cont` → continue (a while body's
                // back-edge — the trailing `br $cont` is the loop's own and
                // is consumed by `lift_while`, so any `br` reaching here is a
                // user break/continue).
                let label = ident(peek_slice(toks, *pos + 1)?);
                match label {
                    "brk" => stmts.push(Stmt::Break),
                    "cont" => stmts.push(Stmt::Continue),
                    other => return Err(refuse_noncanonical(&format!("br ${other}"))),
                }
                *pos += 2;
            }
            "if" => {
                lift_if(toks, pos, ctx, &mut stack, &mut stmts)?;
            }
            // ── a nested `(…)` form: the `while` idiom, or refuse ──
            "(" => {
                let kw = peek_slice(toks, *pos + 1)?;
                if kw == "block" {
                    let while_stmt = lift_while(toks, pos, ctx)?;
                    stmts.push(while_stmt);
                } else {
                    return Err(refuse_noncanonical(kw));
                }
            }
            // ── arithmetic / comparison binary ops ──
            other => {
                if let Some(op) = int_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else if let Some(fop) = float_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else {
                    return Err(refuse_control(other));
                }
            }
        }
    }

    // Reached the end of the slice.
    if end != BlockEnd::Eof {
        return Err(FrontendError::Parse(
            "WAT block ended before its `else`/`end`/`)` terminator".to_string(),
        ));
    }
    Ok(BlockOut {
        stmts,
        stack,
        term: String::new(),
    })
}

/// Lift a `local.set $n` into a `Let` (first write of a declared local) or
/// an `Assign` (a re-write / a non-declared name).
fn lift_local_set(n: String, value: Expr, ctx: &mut BodyCtx<'_>) -> Result<Stmt, FrontendError> {
    if ctx.local_names.contains(&n) && !ctx.assigned.contains(&n) {
        let ty = ctx
            .local_ty
            .get(&n)
            .ok_or_else(|| FrontendError::Parse(format!("unknown local `{n}`")))?
            .clone();
        let mutable = ctx.set_counts.get(&n).copied().unwrap_or(0) > 1;
        ctx.assigned.insert(n.clone());
        Ok(Stmt::Let {
            name: n,
            ty,
            value,
            mutable,
        })
    } else {
        Ok(Stmt::Assign { name: n, value })
    }
}

/// Lift a `call $f` (a helper call → the high-level op, or an intra-module
/// call → [`Expr::Call`]), pushing the result onto `stack`.
fn lift_call(
    callee: String,
    stack: &mut Vec<Expr>,
    ctx: &BodyCtx<'_>,
) -> Result<(), FrontendError> {
    match callee.as_str() {
        "__wasm_floordiv_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::FloorDiv,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        "__wasm_floormod_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::Mod,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        // PMAT-1402: the CHECKED arithmetic helpers. `+`/`-`/`*` stopped
        // emitting bare `i64.add`/`i64.sub`/`i64.mul` and started emitting
        // these calls, so without these arms the lift reconstructs an
        // `Expr::Call` to a function that is not in the lifted module and the
        // re-emit refuses — the fixed point breaks.
        //
        // ⚠️ `__wasm_shl_i64`/`__wasm_shr_i64` are here for the SAME reason and
        // are a PMAT-1402 REPAIR OF PMAT-1379, not new work: that slice routed
        // `<<`/`>>` through helpers and did NOT add these arms, so `lift(emit(M))`
        // has been broken for every shifting module since it merged. No fixture
        // caught it because none of them shifted — `roundtrip_shift_and_arith`
        // is the fixture that now does.
        "__wasm_add_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        "__wasm_sub_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::Sub,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        "__wasm_mul_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::Mul,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        "__wasm_shl_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::Shl,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        "__wasm_shr_i64" => {
            let (lhs, rhs) = pop2(stack, "call")?;
            stack.push(Expr::BinOp {
                op: BinOp::Shr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
        _ => {
            let n = *ctx.arity.get(&callee).ok_or_else(|| {
                FrontendError::Lower(format!(
                    "call to unknown function `{callee}` (no parsed signature)"
                ))
            })?;
            let mut args = Vec::with_capacity(n);
            for _ in 0..n {
                args.push(pop(stack, "call")?);
            }
            args.reverse();
            stack.push(Expr::Call { callee, args });
        }
    }
    Ok(())
}

/// Lift an `if` form — distinguishing the **if-expression** shape
/// `if (result T) <then> else <else> end` (pushes an [`Expr::IfExpr`]) from
/// the **statement-if** shape `if <then-stmts> [else <else-stmts>] end`
/// (emits a [`Stmt::If`]). The condition is the value already on top of
/// `stack` (the emit pushes it just before the `if`).
fn lift_if(
    toks: &[String],
    pos: &mut usize,
    ctx: &mut BodyCtx<'_>,
    stack: &mut Vec<Expr>,
    stmts: &mut Vec<Stmt>,
) -> Result<(), FrontendError> {
    // The condition is the residual operand.
    let cond = pop(stack, "if")?;
    *pos += 1; // consume `if`

    // An if-EXPRESSION is `if (result T) …` — the next tokens are
    // `( result <ty> )`. Detect and skip that prefix.
    let is_expr = peek_slice(toks, *pos)? == "(" && peek_slice(toks, *pos + 1)? == "result";
    if is_expr {
        // skip `( result <ty> )`
        let close = matching_paren(toks, *pos)?;
        *pos = close + 1;

        // then-arm: must reduce to exactly one operand.
        let then_out = lift_block_inner(toks, pos, ctx, BlockEnd::ElseOrEnd)?;
        if then_out.term != "else" {
            return Err(refuse_noncanonical(
                "if (result …) without an `else` arm (xpile always emits both)",
            ));
        }
        let then_expr = single_value(then_out, "if-expr then-arm")?;

        // else-arm: also exactly one operand, terminated by `end`.
        let else_out = lift_block_inner(toks, pos, ctx, BlockEnd::ElseOrEnd)?;
        if else_out.term != "end" {
            return Err(refuse_noncanonical(
                "if (result …) else-arm not closed by `end`",
            ));
        }
        let else_expr = single_value(else_out, "if-expr else-arm")?;

        stack.push(Expr::IfExpr {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        });
        return Ok(());
    }

    // A statement-`if`: arms are statement blocks (no residual value).
    let then_out = lift_block_inner(toks, pos, ctx, BlockEnd::ElseOrEnd)?;
    let then_body = stmts_only(then_out.stmts, then_out.stack, "if then-body")?;

    let else_body = match then_out.term.as_str() {
        "else" => {
            let else_out = lift_block_inner(toks, pos, ctx, BlockEnd::ElseOrEnd)?;
            if else_out.term != "end" {
                return Err(FrontendError::Parse(
                    "statement-if else-arm not closed by `end`".to_string(),
                ));
            }
            stmts_only(else_out.stmts, else_out.stack, "if else-body")?
        }
        "end" => Vec::new(),
        other => {
            return Err(FrontendError::Parse(format!(
                "statement-if then-arm closed by unexpected `{other}`"
            )))
        }
    };

    stmts.push(Stmt::If {
        cond,
        then_body,
        else_body,
    });
    Ok(())
}

/// Lift the canonical `while` idiom:
/// `(block $brk (loop $cont <cond> i32.eqz br_if $brk <body> br $cont))`.
/// `*pos` is at the opening `(` of the `(block …)`.
fn lift_while(
    toks: &[String],
    pos: &mut usize,
    ctx: &mut BodyCtx<'_>,
) -> Result<Stmt, FrontendError> {
    // `( block $brk`
    expect(toks, pos, "(")?;
    expect(toks, pos, "block")?;
    if ident(peek_slice(toks, *pos)?) != "brk" {
        return Err(refuse_noncanonical("(block …) whose label is not $brk"));
    }
    *pos += 1; // $brk

    // `( loop $cont`
    expect(toks, pos, "(")?;
    expect(toks, pos, "loop")?;
    if ident(peek_slice(toks, *pos)?) != "cont" {
        return Err(refuse_noncanonical("(loop …) whose label is not $cont"));
    }
    *pos += 1; // $cont

    // <cond instrs> i32.eqz br_if $brk — lift the condition by simulating
    // the operand stack until the `i32.eqz` guard, which negates the loop
    // test (the emit's `while c` → `c i32.eqz br_if $brk`).
    let cond = lift_loop_cond(toks, pos, ctx)?;

    // <body stmts> br $cont — the loop body is a statement block whose
    // trailing `br $cont` back-edge closes it; then the loop's `)` and the
    // block's `)`.
    let body_out = lift_loop_body(toks, pos, ctx)?;

    Ok(Stmt::While {
        cond,
        body: body_out,
    })
}

/// Lift the loop condition: the operand-stack instructions up to and
/// including `i32.eqz br_if $brk` (the negated loop test). Returns the
/// recovered (un-negated) condition expression.
fn lift_loop_cond(
    toks: &[String],
    pos: &mut usize,
    ctx: &mut BodyCtx<'_>,
) -> Result<Expr, FrontendError> {
    let mut stack: Vec<Expr> = Vec::new();
    loop {
        let instr = peek_slice(toks, *pos)?;
        match instr {
            "i32.eqz" => {
                // The negation guard. The next two tokens MUST be
                // `br_if $brk`; the value on the stack is the un-negated
                // condition.
                *pos += 1;
                expect(toks, pos, "br_if")?;
                if ident(peek_slice(toks, *pos)?) != "brk" {
                    return Err(refuse_noncanonical("loop guard `br_if` not targeting $brk"));
                }
                *pos += 1; // $brk
                if stack.len() != 1 {
                    return Err(refuse_noncanonical(
                        "loop condition did not reduce to a single value",
                    ));
                }
                return Ok(stack.pop().unwrap());
            }
            "local.get" => {
                let n = ident(peek_slice(toks, *pos + 1)?);
                stack.push(Expr::Ident(n.to_string()));
                *pos += 2;
            }
            "i64.const" => {
                let v: i64 = peek_slice(toks, *pos + 1)?
                    .parse()
                    .map_err(|_| FrontendError::Parse("bad i64.const in loop cond".to_string()))?;
                stack.push(Expr::LitInt(v));
                *pos += 2;
            }
            "i32.const" => {
                stack.push(lift_i32_const(
                    peek_slice(toks, *pos + 1)?,
                    " in loop condition",
                )?);
                *pos += 2;
            }
            "f64.const" => {
                let tok = peek_slice(toks, *pos + 1)?;
                let v: f64 = tok.parse().map_err(|_| {
                    FrontendError::Parse(format!("bad f64.const `{tok}` in loop cond"))
                })?;
                stack.push(Expr::LitFloat(v));
                *pos += 2;
            }
            "call" => {
                let callee = ident(peek_slice(toks, *pos + 1)?).to_string();
                lift_call(callee, &mut stack, ctx)?;
                *pos += 2;
            }
            other => {
                if let Some(op) = int_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else if let Some(fop) = float_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else {
                    return Err(refuse_noncanonical(&format!(
                        "loop condition contains `{other}` before the `i32.eqz` guard"
                    )));
                }
            }
        }
    }
}

/// Lift the loop body: statements up to the trailing `br $cont` back-edge,
/// then the loop's `)` and the surrounding block's `)`.
fn lift_loop_body(
    toks: &[String],
    pos: &mut usize,
    ctx: &mut BodyCtx<'_>,
) -> Result<Vec<Stmt>, FrontendError> {
    // The body is a statement block; the emit always ends it with the
    // `br $cont` back-edge immediately before the loop's `)`. We lift
    // statements until we hit that trailing `br $cont )`.
    let mut stmts: Vec<Stmt> = Vec::new();
    let mut stack: Vec<Expr> = Vec::new();
    loop {
        let instr = peek_slice(toks, *pos)?;
        // The trailing back-edge: `br $cont` followed by the loop close `)`.
        if instr == "br"
            && ident(peek_slice(toks, *pos + 1)?) == "cont"
            && peek_slice(toks, *pos + 2)? == ")"
        {
            *pos += 2; // consume `br $cont`
            expect(toks, pos, ")")?; // loop close
            expect(toks, pos, ")")?; // block close
            if !stack.is_empty() {
                return Err(refuse_noncanonical(
                    "while body left a residual value on the operand stack",
                ));
            }
            return Ok(stmts);
        }
        // Otherwise lift one body construct via a one-shot sub-block that
        // stops at the back-edge. Simplest: reuse the leaf/control handling
        // inline by delegating to lift_block_inner with a sentinel is hard
        // (no keyword terminator), so we hand-walk here mirroring the leaf
        // cases, recursing for nested control.
        match instr {
            "local.get" => {
                let n = ident(peek_slice(toks, *pos + 1)?);
                stack.push(Expr::Ident(n.to_string()));
                *pos += 2;
            }
            "local.set" => {
                let n = ident(peek_slice(toks, *pos + 1)?).to_string();
                let value = pop(&mut stack, instr)?;
                stmts.push(lift_local_set(n, value, ctx)?);
                *pos += 2;
            }
            "i64.const" => {
                let v: i64 = peek_slice(toks, *pos + 1)?
                    .parse()
                    .map_err(|_| FrontendError::Parse("bad i64.const in loop body".to_string()))?;
                stack.push(Expr::LitInt(v));
                *pos += 2;
            }
            "i32.const" => {
                stack.push(lift_i32_const(
                    peek_slice(toks, *pos + 1)?,
                    " in loop body",
                )?);
                *pos += 2;
            }
            "f64.const" => {
                let tok = peek_slice(toks, *pos + 1)?;
                let v: f64 = tok.parse().map_err(|_| {
                    FrontendError::Parse(format!("bad f64.const `{tok}` in loop body"))
                })?;
                stack.push(Expr::LitFloat(v));
                *pos += 2;
            }
            "call" => {
                let callee = ident(peek_slice(toks, *pos + 1)?).to_string();
                lift_call(callee, &mut stack, ctx)?;
                *pos += 2;
            }
            "return" => {
                let value = if stack.is_empty() {
                    Expr::Unit
                } else {
                    pop(&mut stack, instr)?
                };
                stmts.push(Stmt::Return(value));
                *pos += 1;
            }
            "br" => {
                let label = ident(peek_slice(toks, *pos + 1)?);
                match label {
                    "brk" => stmts.push(Stmt::Break),
                    "cont" => stmts.push(Stmt::Continue),
                    other => return Err(refuse_noncanonical(&format!("br ${other}"))),
                }
                *pos += 2;
            }
            "if" => {
                lift_if(toks, pos, ctx, &mut stack, &mut stmts)?;
            }
            "(" => {
                let kw = peek_slice(toks, *pos + 1)?;
                if kw == "block" {
                    let while_stmt = lift_while(toks, pos, ctx)?;
                    stmts.push(while_stmt);
                } else {
                    return Err(refuse_noncanonical(kw));
                }
            }
            other => {
                if let Some(op) = int_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else if let Some(fop) = float_binop(other) {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::FloatBinOp {
                        op: fop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else {
                    return Err(refuse_control(other));
                }
            }
        }
    }
}

/// Require a block to have reduced to exactly one operand (an if-expr arm).
fn single_value(out: BlockOut, what: &str) -> Result<Expr, FrontendError> {
    if !out.stmts.is_empty() || out.stack.len() != 1 {
        return Err(refuse_noncanonical(&format!(
            "{what} did not reduce to a single value (got {} stmt(s), stack depth {})",
            out.stmts.len(),
            out.stack.len()
        )));
    }
    Ok(out.stack.into_iter().next().unwrap())
}

/// Require a statement block to have left NO residual operand (a
/// statement-if arm).
fn stmts_only(stmts: Vec<Stmt>, stack: Vec<Expr>, what: &str) -> Result<Vec<Stmt>, FrontendError> {
    if !stack.is_empty() {
        return Err(refuse_noncanonical(&format!(
            "{what} left {} residual operand(s) on the stack",
            stack.len()
        )));
    }
    Ok(stmts)
}

/// A refusal for any instruction outside the lift subset — the honest
/// lossy boundary, never a wrong lift. PMAT-959 moved the structured
/// control-flow shapes (`while`/`if`/`if-expr`/`return`/`break`/`continue`)
/// INSIDE the subset; the boundary now refuses only the WAT shapes the lift
/// still cannot invert (e.g. an arbitrary `br_table`, a `(block …)` that is
/// not the canonical while idiom, or a raw stack-machine branch xpile's emit
/// never produces — anything outside the `xpile-wasm-codegen` image).
fn refuse_control(instr: &str) -> FrontendError {
    FrontendError::Lower(format!(
        "WAT instruction `{instr}` is outside the lift subset — the lift inverts the \
         `xpile-wasm-codegen` image (the straight-line scalar subset plus the canonical \
         structured-control shapes `while`/`if`/`if-expr`/`return`/`break`/`continue`, \
         PMAT-959); an arbitrary stack-machine branch / non-canonical `(block …)` / \
         `br_table` outside that image is refused rather than mis-reconstructed"
    ))
}

/// Lift one `i32.const` operand — the SINGLE decision point for all three
/// call sites (straight-line body, loop condition, loop body), so the guard
/// cannot be added to one arm and forgotten in the other two.
///
/// In the `xpile-wasm-codegen` image an `i32` IS the 0/1 bool encoding (an
/// integer is an `i64`), so `0`/`1` invert to `LitBool` and nothing else
/// inverts at all. PMAT-1392: the old `LitBool(v != 0)` fold silently
/// mapped EVERY nonzero literal to `true`, so `i32.const 2` re-emitted as
/// `i32.const 1` — VALID WAT that `wat2wasm` accepts and `wasm-interp` runs
/// to a DIFFERENT value (2 → 1, and `-5` → 1 against a reference of
/// 4294967291), at exit 0 on every leg. A literal outside `{0, 1}` is a
/// genuine 32-bit integer the lift has no meta-HIR representative for, so it
/// is refused here rather than mis-lifted — the same honest-boundary rule
/// the neighbouring `i32.add` / `i32.popcnt` / `drop` / `(memory …)` shapes
/// already follow.
fn lift_i32_const(tok: &str, site: &str) -> Result<Expr, FrontendError> {
    let v: i64 = tok
        .parse()
        .map_err(|_| FrontendError::Parse(format!("bad i32.const literal{site}")))?;
    match v {
        0 => Ok(Expr::LitBool(false)),
        1 => Ok(Expr::LitBool(true)),
        _ => Err(refuse_i32_const(v, site)),
    }
}

/// The honest boundary for an `i32.const` outside the 0/1 bool encoding
/// (PMAT-1392) — see [`lift_i32_const`].
fn refuse_i32_const(v: i64, site: &str) -> FrontendError {
    FrontendError::Lower(format!(
        "`i32.const {v}`{site} is outside the lift subset — the lift inverts the \
         `xpile-wasm-codegen` image, in which an `i32` is the 0/1 bool encoding \
         and an integer literal is an `i64`; only `i32.const 0` and `i32.const 1` \
         invert (to `false`/`true`). Folding `{v}` to a bool would re-emit \
         `i32.const 1` — valid WAT that runs to a DIFFERENT value than the \
         source — so it is refused rather than mis-lifted; use `i64.const {v}` \
         for an integer literal"
    ))
}

/// Refuse a `(block …)`/`(loop …)` form that is NOT the canonical
/// `while` idiom xpile emits — the honest boundary for control shapes
/// outside the `xpile-wasm-codegen` image.
fn refuse_noncanonical(what: &str) -> FrontendError {
    FrontendError::Lower(format!(
        "non-canonical control shape `{what}` is outside the lift subset — the lift \
         only inverts the exact `(block $brk (loop $cont <cond> i32.eqz br_if $brk \
         <body> br $cont))` while idiom and `if … else … end` forms xpile's emit \
         produces (PMAT-959); any other block/loop/branch nesting is refused"
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
