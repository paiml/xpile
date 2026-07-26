//! PMAT-1341 — EXECUTED witness for native-WASM `math.sqrt(x)`: the FOURTH
//! scalar numeric builtin (`Expr::NumBuiltin`) the WASM lane emits, after
//! PMAT-1338's `abs`, PMAT-1339's `min`/`max`, and PMAT-1340's rounding trio.
//! Runs on the scalar runtime (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## What this slice delivers
//!
//! WASM's `f64.sqrt` is IEEE-754 *correctly-rounded*, exactly like the C
//! `sqrt()` CPython's `math.sqrt` calls — so over `math.sqrt`'s DOMAIN the two
//! agree BIT-for-BIT, not merely to a tolerance. The one place they part is
//! OUTSIDE that domain: `math.sqrt(-1.0)` RAISES `ValueError: math domain
//! error` in CPython where a bare `f64.sqrt` quietly returns NaN — a value
//! CPython never yields. So the lane emits a DOMAIN-GUARDED wrapper,
//! `$__wasm_sqrt_f64`, which traps (`unreachable`) on `x < 0` rather than
//! fabricating that NaN — the same trap-where-CPython-raises discipline
//! PMAT-1340's `i64.trunc_f64_s` narrow uses for `inf`/`nan`.
//!
//! That guard is a single `f64.lt`, which is FALSE for every non-negative input
//! AND for NaN, so the IEEE edges flow through to the instruction and match
//! CPython exactly: `sqrt(-0.0) == -0.0` (sign preserved), `sqrt(inf) == inf`,
//! `sqrt(nan)` is NaN (CPython raises only for a NEGATIVE arg). `-inf` IS
//! negative, so it traps — matching CPython's ValueError.
//!
//! sin/cos/tan/exp/log/log10/log2 stay REFUSED: unlike sqrt they have NO WASM
//! instruction at all, so emitting one would mean shipping a polynomial
//! approximation that diverges from CPython's libm in the low bits.
//!
//! ## The load-bearing edges
//!
//!   * ULP-EXACTNESS — the differential is taken at SEVEN significant digits
//!     (`floor(sqrt(2.0) * 1_000_000)` == 1414213, `sqrt(3.0)` == 1732050), so
//!     an approximation that is merely "close" cannot pass;
//!   * PERFECT SQUARES — `sqrt(9.0)==3`, `sqrt(1000000.0)==1000`,
//!     `sqrt(1e18)==1e9` (no drift at magnitude);
//!   * SUB-ONE inputs — `sqrt(0.25)==0.5`, `sqrt(0.0001)==0.01` (sqrt of a
//!     fraction is LARGER than its input);
//!   * the NEGATIVE domain — the guard TRAPS where CPython RAISES (asserted by
//!     running a trap module AND by running the identical source in python3);
//!   * `-0.0` SIGN preservation — `sqrt(-0.0)` is `-0.0`, not `+0.0`, and it
//!     does NOT trap (`-0.0 < 0.0` is false);
//!   * COMPOSITION — sqrt under `math.floor`, nested in itself
//!     (`sqrt(sqrt(16.0))`), over a float `abs`, in a sum of two sqrts, in a
//!     comparison, bound to a local, inside an `if`-expression, accumulated in a
//!     `while` loop, and across a FUNCTION param boundary;
//!   * the GATE — a module with no sqrt carries no helper; two uses declare it
//!     exactly once; a sqrt buried in a nested `if`/loop still arms the gate (an
//!     undeclared helper at a `call` site is a hard `wat2wasm` failure).
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

/// The observable probes — each `(name, ret-kw, body)` becomes a zero-arg
/// `def <name>() -> <kw>` embedding its inputs as float literals. `wasm-interp`
/// prints an int as `i64:N`, a bool as `i32:0/1`, a float as `f64:N` — but only
/// to SIX digits, so an irrational root is observed through an INT scaling
/// (`floor(sqrt(x) * 10**k)`), which is both exactly comparable AND a sharper
/// (7-digit) check than the printed float would be.
fn probes() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // ── ULP-exactness: irrational roots at 7 significant digits ───────────
        (
            "sq_two",
            "int",
            "    return math.floor(math.sqrt(2.0) * 1000000.0)\n",
        ),
        (
            "sq_three",
            "int",
            "    return math.floor(math.sqrt(3.0) * 1000000.0)\n",
        ),
        // ── perfect squares (no drift, incl. at magnitude) ────────────────────
        (
            "sq_perfect",
            "int",
            "    return math.floor(math.sqrt(9.0))\n",
        ),
        (
            "sq_perfect_big",
            "int",
            "    return math.floor(math.sqrt(1000000.0))\n",
        ),
        ("sq_big", "int", "    return math.floor(math.sqrt(1e18))\n"),
        // ── sub-one inputs: the root is LARGER than the input ─────────────────
        (
            "sq_quarter",
            "int",
            "    return math.floor(math.sqrt(0.25) * 100.0)\n",
        ),
        (
            "sq_tiny",
            "int",
            "    return math.floor(math.sqrt(0.0001) * 10000.0)\n",
        ),
        // ── the zero identity ─────────────────────────────────────────────────
        ("sq_zero", "int", "    return math.floor(math.sqrt(0.0))\n"),
        // ── composition ───────────────────────────────────────────────────────
        (
            "sq_sum",
            "int",
            "    return math.floor((math.sqrt(2.0) + math.sqrt(3.0)) * 100000.0)\n",
        ),
        (
            "sq_nested",
            "int",
            "    return math.floor(math.sqrt(math.sqrt(16.0)))\n",
        ),
        (
            "sq_of_absf",
            "int",
            "    return math.floor(math.sqrt(abs(-4.0)))\n",
        ),
        (
            "sq_pythagoras",
            "int",
            "    return math.floor(math.sqrt(3.0 * 3.0 + 4.0 * 4.0))\n",
        ),
        (
            "sq_local",
            "int",
            "    y: float = math.sqrt(2.0)\n    return math.floor(y * 1000000.0)\n",
        ),
        (
            "sq_in_cond",
            "int",
            "    return math.floor(math.sqrt(4.0)) if 2 < 5 else 0\n",
        ),
        ("sq_cmp", "bool", "    return math.sqrt(2.0) < 1.5\n"),
        // ── accumulated in a WHILE loop (the gate must see a nested sqrt) ──────
        (
            "sq_loop",
            "int",
            "    total: float = 0.0\n    x: float = 1.0\n    while x < 5.0:\n        \
             total = total + math.sqrt(x)\n        x = x + 1.0\n    \
             return math.floor(total * 100000.0)\n",
        ),
        // ── float-VALUED returns (compared at the interp's 6-digit print) ─────
        ("sqf_nine", "float", "    return math.sqrt(9.0)\n"),
        ("sqf_negzero", "float", "    return math.sqrt(-0.0)\n"),
    ]
}

/// The corpus source: `import math`, the observable exports, PLUS a
/// param-boundary helper (a float rounded in the callee) and its caller.
fn corpus_source() -> String {
    let mut src = String::from("import math\n");
    for (name, ret, body) in probes() {
        src.push_str(&format!("def {name}() -> {ret}:\n{body}\n"));
    }
    // Boundary: a float flows across a param and is rooted in the callee.
    src.push_str("def sq_helper(x: float) -> float:\n    return math.sqrt(x)\n");
    src.push_str(
        "def call_sq_param() -> int:\n    return math.floor(sq_helper(2.0) * 1000000.0)\n",
    );
    src
}

/// Every observable export name (all probes + the param-boundary caller — NOT
/// `sq_helper`, which takes a param).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = probes().iter().map(|(n, _, _)| n.to_string()).collect();
    names.push("call_sq_param".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn sqrt_emits_the_domain_guarded_native_helper() {
    let wat = emit("import math\ndef f(x: float) -> float:\n    return math.sqrt(x)\n")
        .expect("math.sqrt must lower");
    // The call site …
    assert!(
        wat.contains("call $__wasm_sqrt_f64"),
        "math.sqrt must call the domain-guarded wrapper:\n{wat}"
    );
    // … and the helper, declared EXACTLY once.
    assert_eq!(
        wat.matches("(func $__wasm_sqrt_f64").count(),
        1,
        "the sqrt helper must be declared exactly once:\n{wat}"
    );
    // It wraps the NATIVE instruction — no libm import, no approximation.
    assert!(
        wat.contains("f64.sqrt"),
        "the helper must use the native f64.sqrt:\n{wat}"
    );
    assert!(
        !wat.contains("(import"),
        "a native sqrt must not pull a libm import:\n{wat}"
    );
    // The GUARD: `x < 0` → trap, where CPython raises ValueError.
    let helper = wat
        .split_once("(func $__wasm_sqrt_f64")
        .map(|(_, r)| r.split_once("\n  (func").map_or(r, |(h, _)| h))
        .expect("helper body");
    assert!(
        helper.contains("f64.lt") && helper.contains("unreachable"),
        "the helper must guard the NEGATIVE domain with f64.lt → unreachable:\n{helper}"
    );
}

#[test]
fn the_sqrt_helper_is_gated_and_deduplicated() {
    // A module with no sqrt carries no helper (no dead code).
    let none = emit("def f(x: float) -> float:\n    return x * 2.0\n").expect("plain float fn");
    assert!(
        !none.contains("$__wasm_sqrt_f64"),
        "a sqrt-free module must not carry the helper:\n{none}"
    );
    // TWO sqrt uses still declare it exactly once.
    let two = emit(
        "import math\ndef f(x: float) -> float:\n    return math.sqrt(x) + math.sqrt(x + 1.0)\n",
    )
    .expect("two sqrts");
    assert_eq!(
        two.matches("(func $__wasm_sqrt_f64").count(),
        1,
        "two sqrt uses must share ONE helper declaration:\n{two}"
    );
    assert_eq!(
        two.matches("call $__wasm_sqrt_f64").count(),
        2,
        "each sqrt use is its own call:\n{two}"
    );
}

#[test]
fn a_deeply_nested_sqrt_still_arms_the_gate() {
    // The recurring gate-hole class: a `call $__wasm_sqrt_f64` whose helper the
    // walker failed to detect is an UNDECLARED function — a hard wat2wasm
    // failure. Bury a sqrt under an `if` inside a `while` inside a function that
    // also uses other helpers, and assert the declaration is still emitted.
    let src = "import math\n\
               def f(x: float) -> int:\n\
               \x20   total: float = 0.0\n\
               \x20   while x < 4.0:\n\
               \x20       if x > 1.0:\n\
               \x20           total = total + math.sqrt(x)\n\
               \x20       x = x + 1.0\n\
               \x20   return math.floor(total * 1000.0)\n";
    let wat = emit(src).expect("nested sqrt must lower");
    assert!(
        wat.contains("call $__wasm_sqrt_f64") && wat.contains("(func $__wasm_sqrt_f64"),
        "a sqrt nested in if-in-while must both CALL and DECLARE the helper:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

#[test]
fn transcendentals_and_an_int_arg_still_refuse() {
    // The transcendentals have NO WASM instruction — still refused, and the
    // message no longer claims sqrt is out of the subset.
    for fname in ["sin", "cos", "tan", "exp", "log", "log10", "log2"] {
        let src = format!("import math\ndef f(x: float) -> float:\n    return math.{fname}(x)\n");
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!("math.{fname} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains("transcendental"),
            "math.{fname} refusal should name the transcendental limit, got: {err}"
        );
        assert!(
            !err.contains("sqrt/sin"),
            "the refusal must no longer list sqrt as refused, got: {err}"
        );
    }
    // An INT argument (`math.sqrt(16)`) needs the int→float widen, which is NOT
    // in the WASM subset — it must refuse rather than silently mis-lower.
    assert!(
        emit("import math\ndef f() -> float:\n    return math.sqrt(16)\n").is_err(),
        "math.sqrt over an int arg must refuse (int→float widen unsupported)"
    );
}

// ---- WABT harness -------------------------------------------------------------

/// Parse the value out of a `name() => <ty>:<v>` interp line as an `f64`.
fn parse_export_f64(stdout: &str, name: &str) -> f64 {
    export_line(stdout, name)
        .rsplit_once(':')
        .expect("malformed export line")
        .1
        .trim()
        .parse::<f64>()
        .expect("parse export value")
}

fn export_line<'a>(stdout: &'a str, name: &str) -> &'a str {
    stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"))
}

/// Assemble + run. `tag` keeps the per-test work dirs disjoint (the
/// multi-execution-path witness gotcha: parallel libtest threads racing on one
/// `prog.wat`).
fn assemble_and_run(wat: &str, tag: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-sqrt-{}-{tag}", std::process::id()));
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
/// coerced through `float()` so every observable parses uniformly as f64.
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
fn sqrt_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the sqrt corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1341: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1341: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        observable_names().len(),
        "python3 must produce one value per observable probe"
    );

    let (stdout, ok) = assemble_and_run(&wat, "corpus");
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_export_f64(&stdout, name);
        // The int-scaled probes are EXACT; the two float-valued probes are read
        // back from a 6-digit print, so they compare at that resolution.
        let float_probe = name.starts_with("sqf_");
        if float_probe {
            assert!(
                (got - *expected).abs() <= 1e-5 * expected.abs().max(1.0),
                "sqrt export `{name}`: wasm {got} != cpython {expected} (6-digit print)\n{stdout}"
            );
        } else {
            assert_eq!(
                got, *expected,
                "sqrt export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
            );
        }
    }
    assert_eq!(
        truth.len(),
        19,
        "expected 19 observable sqrt probes value-matched, got {}",
        truth.len()
    );

    // `sqrt(-0.0)` must keep the SIGN — the generic f64 differential cannot see
    // it (`-0.0 == 0.0`), so read the printed line directly. CPython yields
    // `-0.0`; a `+0.0` here would be a silent IEEE divergence.
    let negzero = export_line(&stdout, "sqf_negzero");
    assert!(
        negzero.contains(":-0"),
        "sqrt(-0.0) must print as NEGATIVE zero (CPython: -0.0), got {negzero:?}"
    );
}

#[test]
fn sqrt_of_a_negative_traps_where_cpython_raises() {
    // The domain guard, executed. A separate single-export module: running it
    // under `--run-all-exports` must report the TRAP, not a NaN.
    let src = "import math\ndef neg() -> float:\n    return math.sqrt(-1.0)\n";
    let wat = emit(src).expect("a negative-literal sqrt still LOWERS (it traps at run time)");

    // CPython's own behaviour on the identical source: ValueError, not a value.
    let py = Command::new("python3")
        .arg("-c")
        .arg(format!("{src}\nneg()\n"))
        .output();
    if let Ok(out) = py {
        assert!(
            !out.status.success()
                && String::from_utf8_lossy(&out.stderr).contains("math domain error"),
            "CPython must RAISE ValueError on math.sqrt(-1.0): {out:?}"
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1341: WABT absent — trap leg asserted at emit level only");
        return;
    }
    let (stdout, _) = assemble_and_run(&wat, "trap");
    assert!(
        stdout.contains("unreachable executed"),
        "math.sqrt(-1.0) must TRAP (CPython raises ValueError), got:\n{stdout}"
    );
    assert!(
        !stdout.contains("nan"),
        "the guard must trap rather than yield the NaN a bare f64.sqrt returns:\n{stdout}"
    );
}
