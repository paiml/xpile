//! PMAT-1338 — EXECUTED witness for native-WASM `abs(x)`: the FIRST scalar
//! numeric builtin (`Expr::NumBuiltin`) the WASM lane emits. Runs on the scalar
//! runtime (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## What this slice delivers
//!
//! Before this slice EVERY `Expr::NumBuiltin` (Python `abs` / `min(a,b)` /
//! `max(a,b)` / the `math.*` family) fell through to the generic
//! "unsupported expression" refusal — the WASM backend handled the variant
//! NOWHERE. PMAT-1338 wires `abs`:
//!
//!   * `abs(float)` → the native single-instruction `f64.abs` (clears the sign
//!     bit). Bit-exact with CPython on EVERY IEEE input — `abs(-0.0) == 0.0`,
//!     `abs(±inf) == inf`, `abs(nan)` is a NaN — because clearing the sign bit
//!     is exactly Python's float `abs`.
//!   * `abs(int)` → `call $__wasm_abs_i64`, a branch-form helper (`x < 0 ? -x :
//!     x`) so the operand is evaluated ONCE (pushed as the arg — a
//!     side-effecting `d.pop(k)` operand is not double-run). CPython-exact over
//!     the representable i64 range.
//!
//! The frontend only lets `abs` through for an `I64`/`F64` first arg (a `bool`
//! is coerced to int, PMAT-795), and records the choice in `of_float` — so the
//! backend dispatches `f64.abs` vs the i64 helper with no ambiguity.
//!
//! The `math.*` transcendentals (sin/cos/tan/exp/log/log10/log2) REFUSE
//! honestly — no WASM instruction, so emitting one would mean a polynomial
//! approximation that diverges from CPython's libm. The ROUNDING
//! `math.floor`/`ceil`/`trunc` (PMAT-1340 — see `math_rounding_witness.rs`) and
//! `math.sqrt` (PMAT-1341, domain-guarded native `f64.sqrt` — see
//! `sqrt_num_builtin_witness.rs`) ARE in the subset. A FLOAT/bool/str `min(a,b)`/`max(a,b)` also
//! refuses; the all-INT `min`/`max` is in the subset (PMAT-1339 — see
//! `int_minmax_witness.rs`).
//!
//! ## The load-bearing edges
//!
//!   * a NEGATIVE operand (`abs(-5) == 5`) — the whole point; the sign is
//!     stripped, not merely `x > 0` filtered;
//!   * `abs(0)` / `abs(0.0)` / `abs(-0.0)` — the zero + IEEE negative-zero
//!     identities (`-0.0` → `+0.0`);
//!   * `abs` COMPOSED — inside arithmetic (`abs(a-b)`), nested (`abs(abs(n)-9)`),
//!     over a unary (`abs(~n)`, `abs(-n)`), in an `if`-expression, over
//!     `len(...)` (which also exercises the literal-collection walker thread),
//!     and summed (`abs(-4)+abs(-6)`);
//!   * a value flowing across a FUNCTION param and `abs`-ed in the callee;
//!   * `str(abs(n))` in a `+`-concatenation — the int-abs feeds `str(int)`.
//!
//! ## The i64::MIN boundary (documented, NOT tested as CPython-equal)
//!
//! Python `abs(-2**63)` is the bignum `2**63`, which is OUTSIDE the modeled i64
//! range (max `2**63 - 1`), so `$__wasm_abs_i64` wraps it back to `i64::MIN` —
//! the SAME i64-domain limitation the lane already has for `-x` and every other
//! int op. The corpus deliberately stays within range; the boundary is not a
//! CPython-equality claim (CPython's answer is unrepresentable here).
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` → `emit_module`
//! → `wat2wasm` → `wasm-interp`), value-matched against LIVE python3 executing
//! the IDENTICAL source. Gated on `wasm_runtime_available()` — a clean skip
//! (still asserting emit + refusals) without WABT.

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- frontend lowering (the CLI's `--target wasm` path) ---------------------

fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// FULL pipeline: Python source (one or more `def`s) → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the probe corpus --------------------------------------------------------

/// The observable probes — each `(name, return-kw, body)` becomes a zero-arg
/// `def <name>() -> <kw>` that embeds its inputs as literals and returns an
/// `abs(...)` value. `wasm-interp` prints an int as `i64:N`, a bool as `i32:0/1`,
/// a float as `f64:N`; every value is chosen so the f64 differential is exact.
fn probes() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // ── int: the core sign strip ───────────────────────────────────────────
        ("iabs_pos", "int", "    n: int = 5\n    return abs(n)\n"),
        ("iabs_neg", "int", "    n: int = -5\n    return abs(n)\n"),
        ("iabs_zero", "int", "    n: int = 0\n    return abs(n)\n"),
        ("iabs_neg_literal", "int", "    return abs(-42)\n"),
        (
            "iabs_big",
            "int",
            "    n: int = -1000000\n    return abs(n)\n",
        ),
        // ── int: COMPOSED with arithmetic / unary / nesting / control-flow ─────
        (
            "iabs_sub",
            "int",
            "    a: int = 3\n    b: int = 10\n    return abs(a - b)\n",
        ),
        (
            "iabs_sub_rev",
            "int",
            "    a: int = 10\n    b: int = 3\n    return abs(a - b)\n",
        ),
        ("iabs_compose_add", "int", "    return abs(-4) + abs(-6)\n"),
        (
            "iabs_mul",
            "int",
            "    n: int = -3\n    return abs(n) * 2\n",
        ),
        (
            "iabs_nested",
            "int",
            "    n: int = -5\n    return abs(abs(n) - 9)\n",
        ),
        ("iabs_bitnot", "int", "    n: int = 5\n    return abs(~n)\n"),
        (
            "iabs_neg_of_neg",
            "int",
            "    n: int = -8\n    return abs(-n)\n",
        ),
        (
            "iabs_in_cond",
            "int",
            "    n: int = -3\n    return abs(n) if n < 0 else 0\n",
        ),
        // ── int: abs(len(...)) — the literal-collection walker thread ──────────
        (
            "iabs_of_len",
            "int",
            "    s: str = \"hello\"\n    return abs(len(s) - 8)\n",
        ),
        // ── bool: str(abs(n)) in a `+`-concat, compared for equality ───────────
        (
            "str_abs_eq",
            "bool",
            "    n: int = -42\n    return \"x=\" + str(abs(n)) == \"x=42\"\n",
        ),
        // ── float: the native f64.abs, exact binary fractions ──────────────────
        (
            "fabs_neg",
            "float",
            "    x: float = -2.5\n    return abs(x)\n",
        ),
        (
            "fabs_pos",
            "float",
            "    x: float = 3.5\n    return abs(x)\n",
        ),
        (
            "fabs_zero",
            "float",
            "    x: float = 0.0\n    return abs(x)\n",
        ),
        // IEEE: abs(-0.0) == +0.0 (the sign bit is cleared).
        (
            "fabs_negzero",
            "float",
            "    x: float = -0.0\n    return abs(x)\n",
        ),
        (
            "fabs_big",
            "float",
            "    x: float = -1000000.5\n    return abs(x)\n",
        ),
        (
            "fabs_sub",
            "float",
            "    a: float = 1.5\n    b: float = 4.0\n    return abs(a - b)\n",
        ),
        ("fabs_compose", "float", "    return abs(-1.5) + abs(2.5)\n"),
        (
            "fabs_mul",
            "float",
            "    x: float = -1.25\n    return abs(x) * 2.0\n",
        ),
        (
            "fabs_in_cond",
            "float",
            "    x: float = -3.5\n    return abs(x) if x < 0.0 else 0.0\n",
        ),
    ]
}

/// The corpus source: the observable exports PLUS two param-boundary helpers (an
/// int and a float `abs`-in-the-callee) and their observable callers.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, ret, body) in probes() {
        src.push_str(&format!("def {name}() -> {ret}:\n{body}\n"));
    }
    // Boundary: an int flows across a param and is abs-ed in the callee.
    src.push_str("def iabs_helper(n: int) -> int:\n    return abs(n)\n");
    src.push_str("def call_iabs_param() -> int:\n    return iabs_helper(-8)\n");
    // Boundary: a float flows across a param and is abs-ed in the callee.
    src.push_str("def fabs_helper(x: float) -> float:\n    return abs(x)\n");
    src.push_str("def call_fabs_param() -> float:\n    return fabs_helper(-8.5)\n");
    src
}

/// Every observable export name (all probes + the two param-boundary callers —
/// NOT the `*_helper`s, which take a param).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = probes().iter().map(|(n, _, _)| n.to_string()).collect();
    names.push("call_iabs_param".to_string());
    names.push("call_fabs_param".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn abs_int_uses_the_branch_helper_and_float_is_inline() {
    // An int abs CALLS + DECLARES the branch helper.
    let wat = emit("def f(n: int) -> int:\n    return abs(n)\n").expect("int abs must lower");
    assert!(
        wat.contains("call $__wasm_abs_i64") && wat.contains("(func $__wasm_abs_i64"),
        "int abs must call AND declare the branch helper (else wat2wasm rejects):\n{wat}"
    );
    assert!(
        !wat.contains("f64.abs"),
        "an int abs must not emit the float instruction:\n{wat}"
    );

    // A float abs is the inline native instruction — NO helper.
    let watf =
        emit("def f(x: float) -> float:\n    return abs(x)\n").expect("float abs must lower");
    assert!(
        watf.contains("f64.abs"),
        "float abs must be the native f64.abs:\n{watf}"
    );
    assert!(
        !watf.contains("$__wasm_abs_i64"),
        "a float abs needs no int helper:\n{watf}"
    );
}

#[test]
fn abs_int_helper_declared_once_across_many_uses() {
    let wat = emit(&corpus_source()).expect("abs corpus must lower");
    assert_eq!(
        wat.matches("(func $__wasm_abs_i64").count(),
        1,
        "the int-abs helper must be emitted EXACTLY once per module, however many \
         call sites use it:\n{wat}"
    );
}

/// `f"...{abs(n)}..."` — the int-abs feeds the f-string `str(int)` wrap
/// (`concat_operand_is_int` classifies int abs as int). Asserted structurally
/// (a bare f-string RETURN yields a str pointer, not a scalar the differential
/// reads): the path cites BOTH the abs helper and the int→decimal helper.
#[test]
fn abs_in_fstring_wraps_via_str_int() {
    let wat = emit("def g() -> str:\n    n: int = -9\n    return f\"v{abs(n)}\"\n")
        .expect("f-string of int abs must lower");
    assert!(
        wat.contains("call $__wasm_abs_i64") && wat.contains("call $__wasm_int_to_str"),
        "an f-string interpolating abs(int) must abs THEN str(int)-materialise:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The `NumBuiltin` ops OUTSIDE `abs` refuse — never silently mis-lowered.
#[test]
fn non_abs_num_builtins_refuse() {
    for (label, src, needle) in [
        // PMAT-1339: an all-INT min/max now LOWERS (see int_minmax_witness.rs);
        // a FLOAT min/max still refuses (WASM f64.min/max mismatch CPython NaN).
        (
            "min(a, b) float",
            "def f(a: float, b: float) -> float:\n    return min(a, b)\n",
            "float (f64)",
        ),
        (
            "max(a, b) float",
            "def f(a: float, b: float) -> float:\n    return max(a, b)\n",
            "float (f64)",
        ),
        // PMAT-1340/1341: the ROUNDING math ops (floor/ceil/trunc) and `math.sqrt`
        // now LOWER natively (see math_rounding_witness.rs /
        // sqrt_num_builtin_witness.rs); the transcendentals still refuse (no WASM
        // instruction at all).
        (
            "math.sin",
            "import math\ndef f(x: float) -> float:\n    return math.sin(x)\n",
            "transcendental",
        ),
    ] {
        let err = match emit(src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains(needle),
            "{label} refusal should mention {needle:?}, got: {err}"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

/// Parse the value out of a `name() => <ty>:<v>` interp line as an `f64`.
/// Integers print as `i64:N` / bools as `i32:0/1` (exactly representable in
/// f64 for our small non-negative values); floats print as `f64:N`.
fn parse_export_f64(stdout: &str, name: &str) -> f64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    line.rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim()
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("parse value for {name} from {line:?}"))
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    // A per-process-unique dir keeps parallel libtest threads from racing on
    // `prog.wat` (the multi-execution-path witness gotcha).
    let dir = std::env::temp_dir().join(format!("xpile-wasm-abs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("prog.wat");
    let wasm_path = dir.join("prog.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

/// Execute the IDENTICAL corpus source in live python3, returning `name=value`
/// pairs for the observable exports — the differential ground truth. A bool is
/// coerced to int and the value is emitted as a repr `float()` so the Rust side
/// parses every observable uniformly as f64.
fn python_truth(src: &str) -> Option<Vec<(String, f64)>> {
    let names = observable_names();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{float(globals()[n]())}}' for n in {names:?}))\n");
    let out = Command::new("python3")
        .arg("-c")
        .arg(&driver)
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "python3 failed on the witness corpus:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(
        stdout
            .trim()
            .split(';')
            .map(|kv| {
                let (k, v) = kv.split_once('=').expect("k=v");
                (k.to_string(), v.parse::<f64>().expect("float"))
            })
            .collect(),
    )
}

// ---- EXECUTED witness (gated on WABT + python3) --------------------------------

#[test]
fn abs_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("abs corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1338: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1338: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        observable_names().len(),
        "python3 must produce one value per observable probe"
    );

    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_export_f64(&stdout, name);
        assert_eq!(
            got, *expected,
            "abs export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1338: {} abs observables (int + float — the sign strip, the zero / \
         IEEE -0.0 identities, composed with arithmetic/unary/nesting/if-expr/len, \
         str(abs) in a concat, and two param-boundary abs) all == live python3.",
        truth.len()
    );
}
