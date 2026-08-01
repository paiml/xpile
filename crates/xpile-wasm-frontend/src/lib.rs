//! WebAssembly text (WAT) frontend — the LIFT half of first-class
//! bidirectional native WASM (PMAT-954, the inverse of the
//! `Target::Wasm` emit half PMAT-951).
//!
//! Lifts the **WAT scalar/control subset** — specifically the image of
//! `xpile-wasm-codegen` — back to canonical meta-HIR via a
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
//!     * The bare `i64` / `f64` / `i32` ops the emit still produces in a
//!       user body — `i64.{and,or,xor}`, the `i64`/`f64`/`i32` comparisons
//!       and `f64.{add,sub,mul}` — → the matching [`BinOp`] /
//!       [`FloatOp`]. Bare `i64.{add,sub,mul,shl,shr_s}` are **refused**
//!       (PMAT-1421) and bare `f64.div` is **refused** (PMAT-1422): the
//!       emit routes those five operators through `$__wasm_*` helpers and
//!       guards every float division against a zero divisor, so each bare
//!       opcode is outside the image and carries WASM (mask/wrap/IEEE)
//!       semantics the high-level operator does not have — see the
//!       lossy-posture section below.
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
//! recursively (the right-inverse property is scoped to what
//! `xpile-wasm-codegen` emits, not arbitrary WASM — minus the two emitted
//! constructs PMAT-1422 measured as unliftable; see the correctness-witness
//! section):
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
//! - **Bare `i64.{add,sub,mul,shl,shr_s}` are REFUSED** (PMAT-1421). These
//!   five had inverse arms written when the emit produced the bare opcode;
//!   PMAT-1379 re-routed `<<`/`>>` and PMAT-1402 re-routed `+`/`-`/`*`
//!   through `$__wasm_*` helpers, and neither slice deleted the stale arms.
//!   They then fired only on WAT the emit does NOT produce — hand-written
//!   and third-party input — and gave it Python semantics: WASM masks a
//!   shift count modulo 64 and wraps arithmetic on overflow, where the
//!   helpers saturate `>>` to 63 and trap on `<<` ≥ 64 and on overflow.
//!   Executed under `wasm-interp` with both legs `wat2wasm`-clean and
//!   transpile at exit 0, `1024 i64.shr_s 70` ran to **16** at the source
//!   and **0** after the round trip; the other four turned a defined
//!   wraparound into a trap. In-domain (shift counts < 64, non-overflowing
//!   arithmetic) the two agree exactly, which is what made the divergence
//!   silent. Refusing follows PMAT-1395 — coercing to one of two
//!   incompatible semantics installs a wrong answer; a correct lift needs
//!   meta-HIR operators carrying WASM wraparound semantics (0.1.619).
//! - **Bare `f64.div` is REFUSED** (PMAT-1422) — the f64 analogue of the
//!   arm above, and the finding of the table sweep PMAT-1421's standing
//!   lead (b) asked for. PMAT-1002 had already written down that a bare
//!   WASM `f64.div` is IEEE 754 (`1.0/0.0` → `inf`, `0.0/0.0` → `NaN`)
//!   where Python's `/` raises `ZeroDivisionError`, and made the emit guard
//!   EVERY float division against a zero divisor and trap. So the emit
//!   never produces a bare `f64.div` (verified from the binary for a
//!   variable, a parameter and a literal divisor) and this arm fired only
//!   on hand-written / third-party WAT, handing it Python semantics.
//!   Executed under `wasm-interp`, both legs `wat2wasm`-clean and transpile
//!   at exit 0: `1.0 f64.div 0.0` ran to **inf** at the source and
//!   **trapped** after the round trip; `0.0 f64.div 0.0` ran to **nan** and
//!   trapped. A non-zero divisor agrees exactly, which is why no fixture
//!   saw it. See `refuse_ieee_div`.
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
//! The lift is a **right-inverse of emit on the part of its WAT image the
//! lift accepts** — pinned by executed round-trip fixed-point tests in
//! `tests.rs`: `emit(lift(emit(M))) == emit(M)` for every straight-line
//! scalar AND structured-control fixture (a `while` sum, an `if`/`else`
//! max, an if-expr, and a nested loop+if). (A full `lift(emit(M)) == M` is
//! *not* claimed — the type collapse above makes the lift lossy; the fixed
//! point is the honest, checkable invariant.)
//!
//! **That qualifier is load-bearing and was missing until PMAT-1422.** It
//! measured the gap at two constructs; PMAT-1423 re-measured over a corpus
//! reaching every scalar construct the emit accepts and found **twelve**.
//! `emit → lift` succeeds for `+ - *`, `// %`, `<< >>`, `& | ^`, `~`, unary
//! `-` on an int, the int and float comparisons, `bool ==`, float `+`, an
//! `f64` literal, an `f32` passthrough, `while`, `if/else` and short-circuit
//! `and`. It **refuses** these, each an emitted construct with no meta-HIR
//! representative to lift back to:
//!
//!   * `not` — lowered to `i32.eqz`, which the lift handles ONLY inside a
//!     loop condition (where it is the negation guard).
//!   * float `/` — the zero-divisor guard ends in `unreachable`.
//!   * unary `-` on a float (`f64.neg`) and on an `f32` (`f32.neg`). An
//!     integer `-x` routes through `call $__wasm_mul_i64` and DOES lift.
//!   * `abs()` on a float (`f64.abs`), `math.floor` (`f64.floor`),
//!     `math.ceil` (`f64.ceil`).
//!   * an `f32` literal (`f32.const`) — the lift inverts
//!     `i64.const`/`i32.const`/`f64.const` only.
//!   * `abs()` on an int, `min`/`max` on ints, and `math.sqrt` — the emit
//!     routes these through `$__wasm_abs_i64` / `$__wasm_min_i64` /
//!     `$__wasm_max_i64` / `$__wasm_sqrt_f64`, prelude helpers the lift has
//!     no inverse arm for. See `refuse_uninvertible_helper`.
//!
//! All refuse honestly (hard [`FrontendError::Lower`], exit 1) — but the last
//! four did **not** until PMAT-1423. The lift reconstructed them as an
//! `Expr::Call` to a helper it had just dropped, and while `--target wasm`
//! caught the dangling call, `--target rust` wrote uncompilable Rust at exit
//! 0 under a contract citation. That asymmetry is why the frontend, not a
//! backend, has to be the one to refuse.
//!
//! Closing the hole needs meta-HIR representatives the language does not
//! have: a unary `not`, a trap statement, a unary float negation, `f32`
//! literals, and float builtins carrying the emit's Python semantics
//! (0.1.619). See `IN_IMAGE_UNINVERTED`, whose entries are checked both
//! ways — every one reachable from an emitted construct, and every refusal
//! the corpus produces named by one.
//!
//! # The module's export surface (PMAT-1424)
//!
//! Everything above is about INSTRUCTIONS. The module's public ABI is a
//! separate vocabulary, and it was not checked at all: the top-level
//! `(export …)` arm skipped every directive unread, on the comment "re-derived
//! from the function on re-emit". That is true of the INTERNAL symbol and
//! false of the EXTERNAL name — and the internal symbol is the one thing a
//! WASM host never sees. The emit's export image is exactly one flat
//! `(export "n" (func $n))` per user function with no `$__wasm_*` helper
//! exported (`check_export_image`, measured from the emitter by the
//! witness, not restated here), so every other shape re-emitted a
//! `wat2wasm`-clean module under a DIFFERENT ABI at exit 0:
//!
//!   * `(export "compute_total" (func $ct))` re-emitted `(export "ct" …)` —
//!     **renamed**. Executed under `wasm-interp`, both legs `wat2wasm`-clean,
//!     transpile at exit 0 on `--target rust`, `ruchy`, `wasm` and `shell`:
//!     the source answers `compute_total() => i64:42` and the round trip
//!     answers `ct() => i64:42`. A host can only call in BY NAME.
//!   * a function exported as both `"alpha"` and `"beta"` re-emitted a single
//!     `(export "f" …)` — **both names lost and a third invented**.
//!   * the folded header spelling `(func $g (export "n") …)` did refuse, but
//!     as a "non-canonical control shape `export` … any other block/loop/branch
//!     nesting is refused" — the PMAT-1422/1423 misdescription one level up.
//!     See `refuse_inline_export`.
//!
//! Unlike PMAT-1423's dangling call, no backend caught any of this; the
//! refusal has to be here. Carrying an external name distinct from the
//! function's own needs a meta-HIR field that does not exist (0.1.619), so
//! this is refused rather than dropped — PMAT-1395.
//!
//! **What is NOT refused, and is the lift's documented lossiness:** a function
//! defined with no export at all still lifts, and the re-emit **publishes**
//! it. That is a WIDENING, not a rewrite — every name the source published
//! keeps pointing at the same function, so no working host call changes
//! meaning. Refusing it was this fix's first cut and it deleted a capability
//! `claims_drift.rs` witnesses (see `check_export_image`).

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

    /// PMAT-1433: none. `.wat` lifts (lossily, see the module docs) — measured
    /// by `frontend_claim_disposition_witness.rs`, not asserted here.
    fn refused_claims(&self) -> &[&'static str] {
        &[]
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

    // Split the module body into top-level `(func …)` slices, COLLECTING the
    // `(export …)` directives (PMAT-1424 — they used to be skipped unread on
    // the claim that they are "re-derived from the function on re-emit", and
    // re-derivation from the INTERNAL symbol is not preservation of the
    // EXTERNAL name) and refusing anything else.
    let mut func_spans: Vec<(usize, usize)> = Vec::new(); // inclusive [open, close]
    let mut exports: Vec<(String, String)> = Vec::new(); // (external name, target symbol)
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
            "export" => exports.push(parse_export_form(&toks, i, close)?),
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

    // PMAT-1424: the FOLDED header spelling `(func $g (export "n") …)` is
    // checked here, before pass 2, because otherwise it reaches the body
    // lifter and is refused as a "non-canonical control shape" — the
    // misdescribing refusal PMAT-1422/1423 worked one level down.
    for &(open, close) in &func_spans {
        refuse_inline_export(&toks[open..=close])?;
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
        // PMAT-1423: this skip is the REASON an un-armed `call $__wasm_*`
        // dangles, so it reads the same constant [`lift_call`]'s guard does
        // — the two cannot drift apart into a namespace the lift drops but
        // does not refuse calls into.
        if raw_name.starts_with(HELPER_PREFIX) {
            continue;
        }
        items.push(Item::Function(lift_function(slice, &arity)?));
    }

    // PMAT-1424: the lift is a right-inverse of the emit ON ITS IMAGE, and the
    // emit's export image is exactly "one flat `(export "n" (func $n))` per
    // user function, no helper exported". Anything else re-emits under a
    // DIFFERENT public ABI at exit 0, so it refuses rather than being silently
    // rewritten. Deliberately LAST: an instruction the lift cannot invert at
    // all is the more specific fact about a module, and several pre-existing
    // witnesses pin those instruction-level refusals on hand-written modules
    // that happen to export nothing — checking the surface first turned every
    // one of them into an export diagnostic (PMAT-1419: over-refusal is the
    // natural failure mode of a refusal fix, and a pre-existing witness is
    // what catches it).
    let func_names: Vec<String> = func_spans
        .iter()
        .map(|&(open, close)| func_name(&toks[open..=close]))
        .collect::<Result<_, _>>()?;
    check_export_image(&func_names, &exports)?;

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

// ─── Export-directive parsing and the emit's export image ───────────

/// Parse ONE top-level `(export "name" (func $sym))` form into its external
/// name and target symbol (PMAT-1424).
///
/// The emit writes exactly this flat shape, one per user function. Anything
/// else — `(export "m" (memory 0))`, a name the flat tokenizer splits because
/// it contains whitespace, an abbreviated form — refuses here rather than
/// being dropped: an export the lift does not understand is an export the
/// re-emit cannot reproduce, and dropping it changes the module's public ABI
/// at exit 0.
fn parse_export_form(
    toks: &[String],
    open: usize,
    close: usize,
) -> Result<(String, String), FrontendError> {
    // ( export "name" ( func $sym ) )
    //  0   1      2    3   4    5   6  ← offsets from `open`
    let shape_ok = close == open + 7
        && toks.get(open + 3).map(String::as_str) == Some("(")
        && toks.get(open + 4).map(String::as_str) == Some("func")
        && toks.get(open + 6).map(String::as_str) == Some(")");
    let quoted = toks.get(open + 2).cloned().unwrap_or_default();
    let name = quoted
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(str::to_string);
    match (shape_ok, name) {
        (true, Some(name)) => {
            let sym = ident(toks.get(open + 5).map(String::as_str).unwrap_or("")).to_string();
            Ok((name, sym))
        }
        _ => Err(FrontendError::Lower(format!(
            "WAT export `{}` is outside the lift subset — the lift inverts the \
             `xpile-wasm-codegen` image, whose every export is the flat form \
             `(export \"n\" (func $n))`. An export shape the lift cannot read is \
             one the re-emit cannot reproduce, and dropping it silently changes \
             the module's public ABI (PMAT-1424); only function exports in that \
             flat form are lifted, and memory/table/global exports are deferred \
             to PMAT-952",
            toks[open..=close.min(toks.len() - 1)].join(" ")
        ))),
    }
}

/// Refuse the FOLDED export form in a function header —
/// `(func $g (export "n") …)` (PMAT-1424).
///
/// This shape is valid WAT that `wat2wasm` accepts, and it is not what the
/// emit writes. Before this guard it fell through [`lift_function`]'s header
/// loop into the body lifter, which refused it as a
/// "non-canonical control shape `export` … any other block/loop/branch nesting
/// is refused" — telling an author who wrote an inline export that they wrote
/// an arbitrary stack-machine branch. That is exactly the misdescribing
/// refusal PMAT-1422 fixed for two mnemonics and PMAT-1423 swept the rest of
/// the instruction vocabulary for; this is the same shape one level up, in the
/// MODULE-SURFACE vocabulary rather than the instruction one.
fn refuse_inline_export(slice: &[String]) -> Result<(), FrontendError> {
    let end = slice.len().saturating_sub(1);
    let mut k = 3; // after "(", "func", "$name"
    while k < end {
        if slice[k] != "(" {
            break; // body begins — an `(export` past here is not a header form
        }
        if slice.get(k + 1).map(String::as_str) == Some("export") {
            return Err(FrontendError::Lower(format!(
                "the folded export form `(func ${} (export …) …)` is outside the \
                 lift subset — the emit writes every export as a separate \
                 top-level `(export \"n\" (func $n))`, so the lift has no inverse \
                 for the inline spelling and would drop it, re-emitting the \
                 module under a different public ABI at exit 0 (PMAT-1424). \
                 Hoist it to a top-level export directive",
                func_name(slice)?
            )));
        }
        k = local_matching(slice, k)? + 1;
    }
    Ok(())
}

/// Check a module's export set against the emit's export IMAGE (PMAT-1424).
///
/// Measured from the binary at 222549f2: `xpile transpile <m>.py --target wasm`
/// emits, for every user function `$n`, exactly one top-level
/// `(export "n" (func $n))`, and emits NO export for any `$__wasm_*` prelude
/// helper. That is the whole image, so it is the whole acceptance condition —
/// and each way of falling outside it was a SILENT ABI REWRITE at exit 0, with
/// both legs `wat2wasm`-clean:
///
/// | out-of-image shape                          | before PMAT-1424 | now |
/// |---------------------------------------------|------------------|-----|
/// | `(export "compute_total" (func $ct))`       | re-emits `(export "ct" …)` — **renamed** | refused |
/// | `$f` exported as both `"alpha"` and `"beta"`| re-emits `(export "f" …)` — **both names lost, a third invented** | refused |
/// | `(func $g (export "n") …)` folded spelling  | refused, as a "non-canonical control shape" | refused, [`refuse_inline_export`] |
/// | `$priv` defined with no export              | re-emits `(export "priv" …)` — **made public** | ACCEPTED, see below |
///
/// Executed under `wasm-interp`, no-argument export, transpile at exit 0 on
/// both legs: the source module answers `compute_total() => i64:42` and the
/// round-tripped module answers `ct() => i64:42`. A host that looks the export
/// up by name — which is the only way a WASM host can call in — gets an
/// unknown-export failure against a module xpile reported success for.
///
/// This is the module-surface analogue of the instruction-level family
/// PMAT-1421/1422/1423 worked: the top-level `(export …)` arm was not a stale
/// inverse but an UNREAD one, skipped on the comment "re-derived from the
/// function on re-emit". Re-derivation from the internal symbol is not
/// preservation of the external name, and the internal symbol is the one thing
/// a WASM host never sees. Refusing rather than teaching the lift to carry
/// export names follows PMAT-1395: meta-HIR has no place to put an external
/// name distinct from the function's own, so carrying it would mean inventing
/// one, and the honest boundary until it exists (0.1.619) is a refusal.
///
/// # Why an unexported function is ACCEPTED, not refused
///
/// Every rule here is keyed on an export directive that IS present, because
/// only those can be corrupted. A function with no export is a documented
/// LOSSY WIDENING, not a rewrite: the re-emit publishes it, but every name the
/// source published keeps pointing at the same function, so no working host
/// call changes meaning.
///
/// Refusing it was the first cut of this fix and it was WRONG — over-refusal is
/// the natural failure mode here (PMAT-1419), and two pre-existing gates caught
/// it. `claims_drift.rs` feeds every frontend "a real program in its own
/// language", and for `wasm` that is a bare `(func $add …)` with no export
/// section at all; refusing it dropped `wasm` out of the substantive-frontend
/// set and falsified the README's source-language count. Deleting a witnessed
/// capability to close a lesser hole is the trade PMAT-1419 recorded three
/// times. The widening is instead named in the module doc and in
/// `still_open`.
fn check_export_image(
    func_names: &[String],
    exports: &[(String, String)],
) -> Result<(), FrontendError> {
    for (name, sym) in exports {
        if sym.starts_with(HELPER_PREFIX) {
            return Err(FrontendError::Lower(format!(
                "`(export \"{name}\" (func ${sym}))` is outside the lift subset — \
                 `${HELPER_PREFIX}*` is the `xpile-wasm-codegen` prelude namespace, \
                 which the emit never exports and the lift DROPS, so the re-emitted \
                 module would not contain the exported function at all (PMAT-1424)"
            )));
        }
        if !func_names.iter().any(|f| f == sym) {
            return Err(FrontendError::Lower(format!(
                "`(export \"{name}\" (func ${sym}))` names a function this module \
                 does not define — the lift only inverts exports of functions \
                 present in the same module (PMAT-1424)"
            )));
        }
        if name != sym {
            return Err(FrontendError::Lower(format!(
                "`(export \"{name}\" (func ${sym}))` is outside the lift subset — the \
                 lift inverts the `xpile-wasm-codegen` image, which exports every \
                 function under its OWN name, and meta-HIR has no place to carry an \
                 external name distinct from the function's. Lifting this would \
                 re-emit `(export \"{sym}\" (func ${sym}))`: `wat2wasm`-clean, exit 0, \
                 and a DIFFERENT public ABI — a host calling `{name}` gets an \
                 unknown export (PMAT-1424). It is refused rather than silently \
                 renamed; rename the function to `${name}` to lift it"
            )));
        }
    }
    Ok(())
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
                if let Some(op) = int_binop(other)? {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else if let Some(fop) = float_binop(other)? {
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
        // PMAT-1423: every OTHER `$__wasm_*` name. The lift DROPS every
        // `$__wasm_*` function definition (see the pass-2 `starts_with`
        // skip in [`lift_wat`]), so a surviving call to one that has no
        // inverse arm above reconstructs an `Expr::Call` to a callee that
        // is not in the lifted module — a DANGLING call, not a lift.
        //
        // This is not hypothetical and it is not caught downstream on every
        // target. `--target wasm` happens to refuse it ("not a function of
        // this WASM module"), but `--target rust` exits **0** and writes
        // uncompilable Rust carrying a contract citation:
        //
        // ```text
        // $ xpile transpile sqrt.wat --target rust   # exit 0
        // // xpile-contract: C-PY-FLOAT-ARITH
        // pub fn f(a: f64) -> f64 { __wasm_sqrt_f64(a) }
        // $ rustc --crate-type=lib sqrt.rs
        // error[E0425]: cannot find function `__wasm_sqrt_f64` in this scope
        // ```
        //
        // Measured from the BINARY over the emitted-construct corpus, four
        // reachable constructs land here: `abs()` over an int
        // (`$__wasm_abs_i64`), `min`/`max` over ints (`$__wasm_min_i64` /
        // `$__wasm_max_i64`) and `math.sqrt` (`$__wasm_sqrt_f64`). The
        // guard is keyed on the reserved-namespace SHAPE rather than on
        // that list, so a helper the emit grows later refuses instead of
        // dangling (PMAT-1391: key on shape, wire at the single choke
        // point). It cannot over-refuse: the seven invertible helpers are
        // matched by the arms above, and any other `$__wasm_*` definition
        // — even a hand-written one — is dropped by the same pass-2 skip,
        // so the call would dangle either way.
        _ if callee.starts_with(HELPER_PREFIX) => {
            return Err(refuse_uninvertible_helper(&callee));
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
                if let Some(op) = int_binop(other)? {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else if let Some(fop) = float_binop(other)? {
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
                if let Some(op) = int_binop(other)? {
                    let (lhs, rhs) = pop2(&mut stack, instr)?;
                    stack.push(Expr::BinOp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    });
                    *pos += 1;
                } else if let Some(fop) = float_binop(other)? {
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
    // PMAT-1422: `i32.eqz` and `unreachable` are IN the emit image — they are
    // the lowering of `not` and of the emit's own trap guards. Telling their
    // author they wrote "an arbitrary stack-machine branch" is false, and it
    // hid the fact that `emit(M)` for those two constructs cannot be lifted
    // at all. Name the real reason instead. (Closing the hole needs meta-HIR
    // representatives — a unary `not` and a trap statement — which do not
    // exist yet; 0.1.619 capability work.)
    if let Some(construct) = in_image_uninverted(instr) {
        return FrontendError::Lower(format!(
            "WAT instruction `{instr}` IS inside the `xpile-wasm-codegen` emit image \
             ({construct}), but the lift has no meta-HIR representative for it, so \
             `emit(M)` for that construct does not round-trip. The full measured hole \
             is {} (PMAT-1423 re-measured PMAT-1422's two over a corpus reaching every \
             scalar construct the emit accepts), plus the un-armed `$__wasm_*` prelude \
             helpers. It is refused rather than silently dropped; closing it needs \
             meta-HIR representatives the language does not have yet — a unary `not`, \
             a trap statement, a unary float negation, `f32` literals, and float \
             builtins carrying the emit's semantics (0.1.619)",
            IN_IMAGE_UNINVERTED
                .iter()
                .map(|(k, _)| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    FrontendError::Lower(format!(
        "WAT instruction `{instr}` is outside the lift subset — the lift inverts the \
         `xpile-wasm-codegen` image (the straight-line scalar subset plus the canonical \
         structured-control shapes `while`/`if`/`if-expr`/`return`/`break`/`continue`, \
         PMAT-959); an arbitrary stack-machine branch / non-canonical `(block …)` / \
         `br_table` outside that image is refused rather than mis-reconstructed"
    ))
}

/// The instructions the emit DOES produce in a user body but the lift cannot
/// invert. Re-derived from the BINARY over the emitted-construct corpus, not
/// asserted — and both halves are enforced by
/// `every_uninverted_in_image_instruction_is_named_and_reachable`: every entry
/// here must be REACHED by some emitted construct (so an entry cannot go stale
/// the way PMAT-1421's inverse arms did), and every mnemonic the emit produces
/// that the lift refuses must BE here (so a new emit opcode cannot quietly
/// fall back to the "arbitrary stack-machine branch" message).
///
/// ⚠️ PMAT-1422 populated this with two entries and wrote "the hole is exactly
/// these two" into the refusal text, four doc sites and a witness name. That
/// was measured over a 7-row fixture corpus with no float builtin, no unary
/// float `-`, and no `F32` in it — so it could not have found the other six.
/// PMAT-1423 re-measured over a corpus reaching every scalar construct the
/// emit accepts. A fixture corpus cannot establish a whole-image claim; that
/// sentence was already in this crate's test module doc when the claim it
/// warns about was written one screen below it.
///
/// Note the inversion PMAT-1422 exposed — before it, the lift REJECTED the
/// emit's own guarded division (on the `unreachable`) while ACCEPTING and
/// corrupting a hand-written bare `f64.div`. See [`refuse_ieee_div`].
pub(crate) const IN_IMAGE_UNINVERTED: &[(&str, &str)] = &[
    // PMAT-1422
    ("i32.eqz", "it is how the emit lowers a boolean `not`"),
    (
        "unreachable",
        "it is how the emit traps — the `//`/`%` and float-`/` zero-divisor \
         guards and the checked-arithmetic helpers all end in it",
    ),
    // PMAT-1423 — all six reached by an emitted construct, none of which the
    // PMAT-1422 corpus exercised.
    (
        "f64.neg",
        "it is how the emit lowers a unary `-` on a float (an integer `-x` \
         routes through `call $__wasm_mul_i64`, which the lift DOES invert)",
    ),
    (
        "f32.neg",
        "it is how the emit lowers a unary `-` on an `f32` value",
    ),
    ("f64.abs", "it is how the emit lowers `abs()` on a float"),
    ("f64.floor", "it is how the emit lowers `math.floor`"),
    ("f64.ceil", "it is how the emit lowers `math.ceil`"),
    (
        "f32.const",
        "it is how the emit lowers a float literal at type `f32` (a C \
         `float`); the lift inverts `i64.const`/`i32.const`/`f64.const` only",
    ),
];

fn in_image_uninverted(instr: &str) -> Option<&'static str> {
    IN_IMAGE_UNINVERTED
        .iter()
        .find(|(k, _)| *k == instr)
        .map(|(_, why)| *why)
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
///
/// `Ok(None)` means "not a binary op at all" — the caller tries
/// [`float_binop`] next, then refuses. `Err` means "a binary op the lift
/// must NOT invert": see [`refuse_helper_routed`]. Returning a `Result`
/// rather than guarding at the call sites is deliberate — PMAT-1392's
/// lesson, applied to the arithmetic table. This function is the SINGLE
/// decision point for all three lift sites (straight-line body, loop
/// condition, loop body), so the guard cannot be added to one arm and
/// forgotten in the other two.
fn int_binop(instr: &str) -> Result<Option<BinOp>, FrontendError> {
    Ok(Some(match instr {
        // PMAT-1421: the five mnemonics the emit STOPPED producing in user
        // bodies (PMAT-1379 for the shifts, PMAT-1402 for `+`/`-`/`*`).
        // They are no longer the inverse of anything — see
        // [`refuse_helper_routed`] for the executed divergence.
        "i64.add" | "i64.sub" | "i64.mul" | "i64.shl" | "i64.shr_s" => {
            return Err(refuse_helper_routed(instr))
        }
        "i64.and" => BinOp::BitAnd,
        "i64.or" => BinOp::BitOr,
        "i64.xor" => BinOp::BitXor,
        "i64.eq" | "f64.eq" | "i32.eq" => BinOp::Eq,
        "i64.ne" | "f64.ne" | "i32.ne" => BinOp::NotEq,
        "i64.lt_s" | "f64.lt" => BinOp::Lt,
        "i64.le_s" | "f64.le" => BinOp::LtEq,
        "i64.gt_s" | "f64.gt" => BinOp::Gt,
        "i64.ge_s" | "f64.ge" => BinOp::GtEq,
        _ => return Ok(None),
    }))
}

/// The honest boundary for a BARE i64 arithmetic mnemonic whose operator the
/// emit routes through a `$__wasm_*` helper (PMAT-1421).
///
/// These five arms were written when the emit DID produce the bare opcode,
/// and they were correct right-inverses then. PMAT-1379 re-routed `<<`/`>>`
/// through `$__wasm_shl_i64`/`$__wasm_shr_i64` and PMAT-1402 re-routed
/// `+`/`-`/`*` through `$__wasm_add_i64`/`$__wasm_sub_i64`/`$__wasm_mul_i64`;
/// neither slice deleted the now-stale bare arms. Re-derived from the binary,
/// the emit's user-body opcode set is `i64.{and,or,xor}` + the comparisons —
/// these five appear ONLY inside the `$__wasm_*` prelude, which the lift skips
/// wholesale. So the arms could no longer inverse anything the emit produces;
/// they fired only on hand-written / third-party WAT, and gave it PYTHON
/// semantics it does not have.
///
/// The two semantics genuinely differ, and the difference is not cosmetic —
/// executed under `wasm-interp`, both legs `wat2wasm`-clean, transpile at
/// exit 0:
///
/// | bare source                      | source runs to | re-emit runs to |
/// |----------------------------------|----------------|-----------------|
/// | `1024 i64.shr_s 70`              | `16`           | `0`             |
/// | `1 i64.shl 70`                   | `64`           | trap            |
/// | `i64::MAX i64.add 1`             | `i64::MIN`     | trap            |
/// | `i64::MIN i64.sub 1`             | `i64::MAX`     | trap            |
/// | `2^62 i64.mul 4`                 | `0`            | trap            |
///
/// WASM defines `i64.shl`/`i64.shr_s` to MASK the shift count modulo 64 and
/// `i64.{add,sub,mul}` to WRAP on overflow. The helpers implement Python:
/// `>>` saturates the count to 63, `<<` traps at ≥ 64, and the arithmetic
/// traps on overflow. In-domain the two agree exactly (verified: shifts < 64
/// and non-overflowing arithmetic round-trip byte-identically), so the
/// divergence is precisely the edge — which is what makes it a silent wrong
/// answer rather than a visible failure.
///
/// Refusing rather than coercing follows PMAT-1395: making the output *run*
/// by picking one of two incompatible semantics installs a silent wrong
/// answer. A correct lift needs meta-HIR operators carrying WASM wraparound
/// semantics, which do not exist (0.1.619 capability work).
/// The reserved namespace `xpile-wasm-codegen` uses for its synthetic
/// prelude functions. [`lift_wat`] DROPS every function whose name starts
/// with this, so it is also exactly the set of callees a lifted module can
/// never resolve — which is why [`lift_call`] refuses the ones it has no
/// inverse arm for (PMAT-1423).
pub(crate) const HELPER_PREFIX: &str = "__wasm_";

/// The `$__wasm_*` prelude helpers [`lift_call`] CAN invert, each back to the
/// high-level meta-HIR operator the emit routed through it. Every other name
/// in the [`HELPER_PREFIX`] namespace refuses (PMAT-1423).
///
/// Exposed for the witness, which asserts this list and the `lift_call` arms
/// stay in step — a helper listed here but not armed would refuse while
/// claiming to be invertible, and an armed helper missing here would be
/// omitted from the refusal message that tells authors what *is* supported.
pub(crate) const INVERTIBLE_HELPERS: &[&str] = &[
    "__wasm_floordiv_i64",
    "__wasm_floormod_i64",
    "__wasm_add_i64",
    "__wasm_sub_i64",
    "__wasm_mul_i64",
    "__wasm_shl_i64",
    "__wasm_shr_i64",
];

/// The honest boundary for a call to a `$__wasm_*` prelude helper the lift
/// has no inverse arm for (PMAT-1423) — the third shape in the family
/// PMAT-1421 and PMAT-1422 opened, and the one their standing lead (d) named.
///
/// The first two were STALE INVERSE ARMS: an arm that was a correct inverse
/// when written, left live after the emit re-routed the operator, firing only
/// on input the emit no longer produces. This one is the complement — a
/// MISSING arm whose fallback was not a refusal but a *reconstruction*. The
/// generic `_` arm looked the callee up in the parsed arity table, found the
/// prelude helper's real signature there, and built a well-formed
/// `Expr::Call` to it. Then pass 2 dropped the helper's definition, so the
/// lifted module referenced a function it did not contain.
///
/// Measured from the binary, four emitted constructs reach here, and the
/// failure is target-dependent — which is what kept it quiet:
///
/// | source construct  | emitted call            | `--target wasm` | `--target rust` |
/// |-------------------|-------------------------|-----------------|-----------------|
/// | `abs(int)`        | `$__wasm_abs_i64`       | refuses         | **exit 0**, `E0425` |
/// | `min(int, int)`   | `$__wasm_min_i64`       | refuses         | **exit 0**, `E0425` |
/// | `max(int, int)`   | `$__wasm_max_i64`       | refuses         | **exit 0**, `E0425` |
/// | `math.sqrt(f)`    | `$__wasm_sqrt_f64`      | refuses         | **exit 0**, `E0425` |
///
/// The `--target wasm` refusal comes from the BACKEND ("not a function of
/// this WASM module"), not from the lift, so it says nothing about the other
/// eight backends; `--target rust` wrote `pub fn f(a: f64) -> f64 {
/// __wasm_sqrt_f64(a) }` at exit 0, under a `C-PY-FLOAT-ARITH` citation,
/// and `rustc` rejects it with `error[E0425]: cannot find function`.
///
/// Refusing rather than inventing a lowering follows PMAT-1395: `abs`,
/// `min`, `max` and `sqrt` DO have meta-HIR representatives
/// ([`NumBuiltinOp`]), but the emit's helpers implement *Python* semantics
/// (`abs(i64::MIN)` traps rather than wrapping) and re-emitting them through
/// the high-level builtin is only sound if that matches — which is the same
/// question PMAT-1421 left open for the wraparound operators. Adding these
/// arms is 0.1.619 capability work; refusing is the honest boundary until
/// then.
fn refuse_uninvertible_helper(callee: &str) -> FrontendError {
    FrontendError::Lower(format!(
        "`call ${callee}` is outside the lift subset — `${HELPER_PREFIX}*` is the \
         `xpile-wasm-codegen` prelude namespace, and the lift DROPS every function \
         in it, so lifting this call would produce a call to a function the lifted \
         module does not contain. That dangling call is not caught by every \
         backend: `--target wasm` refuses it, but `--target rust` emitted \
         `{callee}(…)` at exit 0 and `rustc` rejects it with `E0425` \
         (PMAT-1423). The helpers the lift CAN invert are: {}. A `${callee}` arm \
         needs the meta-HIR operator to carry the helper's Python semantics \
         (0.1.619), so it is refused rather than mis-lifted",
        INVERTIBLE_HELPERS
            .iter()
            .map(|h| format!("`${h}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn refuse_helper_routed(instr: &str) -> FrontendError {
    let helper = match instr {
        "i64.add" => "$__wasm_add_i64",
        "i64.sub" => "$__wasm_sub_i64",
        "i64.mul" => "$__wasm_mul_i64",
        "i64.shl" => "$__wasm_shl_i64",
        _ => "$__wasm_shr_i64",
    };
    FrontendError::Lower(format!(
        "bare `{instr}` is outside the lift subset — the lift inverts the \
         `xpile-wasm-codegen` image, and the emit routes this operator through \
         `call {helper}` (PMAT-1379 for the shifts, PMAT-1402 for `+`/`-`/`*`), \
         never the bare opcode. The two disagree at the edge: WASM masks a shift \
         count modulo 64 and wraps arithmetic on overflow, where the helper \
         saturates `>>` to 63 and traps on `<<` ≥ 64 and on overflow — so \
         lifting `{instr}` to the high-level operator re-emits WAT that RUNS to \
         a different value (`1024 i64.shr_s 70` is 16 at the source and 0 after \
         the round trip). It is refused rather than mis-lifted; use \
         `call {helper}` for the Python-semantics operator"
    ))
}

/// Map an f64 WAT arithmetic mnemonic to its meta-HIR [`FloatOp`].
/// (f64 *comparisons* lift to a [`BinOp`] via [`int_binop`], matching the
/// emit, which routes them through `Expr::BinOp`.)
///
/// `Ok(None)` means "not a float binary op at all" — the caller refuses.
/// `Err` means "a float binary op the lift must NOT invert": see
/// [`refuse_ieee_div`]. Returning a `Result` rather than guarding at the
/// call sites mirrors [`int_binop`] (PMAT-1421) — this is the SINGLE
/// decision point for all three lift sites (straight-line body, loop
/// condition, loop body), so the guard cannot be added to one arm and
/// forgotten in the other two.
fn float_binop(instr: &str) -> Result<Option<FloatOp>, FrontendError> {
    Ok(Some(match instr {
        "f64.add" => FloatOp::Add,
        "f64.sub" => FloatOp::Sub,
        "f64.mul" => FloatOp::Mul,
        // PMAT-1422: the emit NEVER produces a bare `f64.div` — every `/`
        // is guarded (PMAT-1002). See [`refuse_ieee_div`].
        "f64.div" => return Err(refuse_ieee_div()),
        _ => return Ok(None),
    }))
}

/// The honest boundary for a BARE `f64.div` (PMAT-1422) — the f64 analogue
/// of [`refuse_helper_routed`], and found by the sweep PMAT-1421's standing
/// lead (b) asked for (its binary-operator table was swept; the f64 table
/// was not).
///
/// `FloatOp::Div` is **Python's** `/`, and PMAT-1002 already wrote down why
/// that is not WASM's `f64.div`: CPython raises `ZeroDivisionError` where
/// IEEE 754 returns `inf`/`NaN`. The emit encodes that difference — it
/// guards EVERY float division against a zero divisor and traps, verified
/// from the binary across all three divisor shapes (variable, parameter,
/// literal):
///
/// ```wat
/// local.set $__wasm_fdiv_d
/// local.get $__wasm_fdiv_d
/// f64.const 0.0
/// f64.eq
/// if
///   unreachable
/// end
/// local.get $__wasm_fdiv_d
/// f64.div
/// ```
///
/// So a bare `f64.div` is outside the emit image, and this arm could only
/// ever fire on hand-written / third-party WAT — the input an advertised
/// `.wat` frontend exists to accept. Executed under `wasm-interp`, both legs
/// `wat2wasm`-clean and transpile at exit 0:
///
/// | bare source          | source runs to | re-emit runs to |
/// |----------------------|----------------|-----------------|
/// | `1.0 f64.div 0.0`    | `inf`          | trap            |
/// | `0.0 f64.div 0.0`    | `nan`          | trap            |
/// | `6.0 f64.div 3.0`    | `2.0`          | `2.0`           |
///
/// A non-zero divisor agrees exactly, which is what made the divergence
/// silent — no fixture divided by zero. Refusing rather than coercing
/// follows PMAT-1395: making the output *run* by picking one of two
/// incompatible semantics installs a wrong answer. A correct lift needs a
/// meta-HIR float operator carrying IEEE (non-trapping) division, which does
/// not exist (0.1.619 capability work).
fn refuse_ieee_div() -> FrontendError {
    FrontendError::Lower(
        "bare `f64.div` is outside the lift subset — the lift inverts the \
         `xpile-wasm-codegen` image, and the emit routes float division through a \
         zero-divisor guard (`f64.eq` + `unreachable`, PMAT-1002) before the \
         `f64.div`, never the bare opcode. The two disagree exactly at a zero \
         divisor: WASM's `f64.div` is IEEE 754 and returns `inf`/`NaN`, where \
         meta-HIR's `FloatOp::Div` is Python's `/` and raises `ZeroDivisionError` \
         (the emit's trap) — so lifting a bare `f64.div` to the high-level \
         operator re-emits WAT that RUNS to a different result (`1.0 / 0.0` is \
         `inf` at the source and a trap after the round trip). It is refused \
         rather than mis-lifted"
            .to_string(),
    )
}

#[cfg(test)]
mod tests;
