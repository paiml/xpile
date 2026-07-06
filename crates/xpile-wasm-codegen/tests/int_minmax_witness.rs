//! PMAT-1339 — EXECUTED witness for native-WASM integer `min(a, b, …)` /
//! `max(a, b, …)`: the SECOND scalar numeric builtin (`Expr::NumBuiltin`) the
//! WASM lane emits, after PMAT-1338's `abs`. Runs on the scalar runtime
//! (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## What this slice delivers
//!
//! WASM has NO `i64.min`/`i64.max` instruction (only the float forms), so an
//! all-INT `min`/`max` folds each pairwise step through a `select` helper:
//!
//!   * `$__wasm_min_i64(a, b)` = `a < b ? a : b` (`i64.lt_s` + `select`);
//!   * `$__wasm_max_i64(a, b)` = `a > b ? a : b` (`i64.gt_s` + `select`).
//!
//! The variadic form is a LEFT fold — `min(a, b, c)` emits `a`, then for each
//! remaining operand emits it and `call`s the helper, so it lowers to
//! `min(min(a, b), c)`, matching CPython's left-to-right reduce. Every operand
//! is emitted EXACTLY once (pushed as a helper arg), so a side-effecting operand
//! is not double-run. CPython-exact over the whole i64 range: a min/max never
//! leaves the operand set, so there is no overflow boundary (unlike
//! `abs(i64::MIN)`), and ties resolve to a value equal to CPython's first-wins
//! pick (`<`/`>` — not `<=`/`>=` — makes a tie fall through to `b`, which equals
//! `a` for equal ints).
//!
//! A FLOAT min/max REFUSES: it needs Python's order-dependent NaN semantics
//! (`min(1.0, nan) == 1.0` but `min(nan, 1.0)` is nan) that WASM's
//! always-NaN-propagating `f64.min`/`f64.max` do NOT match. A `bool`/`str`
//! min/max (an i32 operand) also refuses — bool would need an i64 coercion and
//! str is a heap-pointer content compare, both a separate lane.
//!
//! ## The load-bearing edges
//!
//!   * ORDER independence — `min(3, 10)` and `min(10, 3)` both pick `3`; same for
//!     `max`, in both operand orders (the `select` condition is not accidentally
//!     inverted);
//!   * NEGATIVE operands — `max(-5, 3) == 3` (picks the positive), and
//!     `min(-5, -2)` / `max(-5, -2)` shifted `+ 100` (a min/max over two
//!     negatives, kept non-negative for the unsigned-`i64` interp print);
//!   * a TIE — `min(7, 7) == max(7, 7) == 7`;
//!   * VARIADIC `>2`-arg — `min(9, 4, 6)`, `max(5, 2, 8, 1)` (the left fold);
//!   * NESTED min-in-max / max-in-min — `min(max(1, 9), 4)` exercises BOTH
//!     helpers coexisting (the gate walker must fire each independently);
//!   * COMPOSED — inside arithmetic (`max(2, 3) * 10`, `min(a-b, 2) + 100`), over
//!     `abs(...)` (`min(abs(-4), 2)` — abs AND min helpers coexist), over
//!     `len(...)`, and in an `if`-expression;
//!   * a value flowing across a FUNCTION param and min/max-ed in the callee.
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

/// The observable probes — each `(name, body)` becomes a zero-arg `def <name>()
/// -> int` embedding its inputs as literals and returning an int `min`/`max`
/// value. Every RESULT is non-negative so `wasm-interp` (which prints an i64
/// UNSIGNED) and live python3 agree through the shared f64 differential.
fn probes() -> Vec<(&'static str, &'static str)> {
    vec![
        // ── order independence ────────────────────────────────────────────────
        ("imin2", "    return min(3, 10)\n"),
        ("imin2_rev", "    return min(10, 3)\n"),
        ("imax2", "    return max(3, 10)\n"),
        ("imax2_rev", "    return max(10, 3)\n"),
        // ── negative operands (results kept non-negative) ─────────────────────
        ("imax_neg_pos", "    return max(-5, 3)\n"),
        ("imin_neg_shift", "    return min(-5, -2) + 100\n"),
        ("imax_neg_shift", "    return max(-5, -2) + 100\n"),
        ("imin_negpos_shift", "    return min(-5, 3) + 100\n"),
        // ── ties (first-wins is value-identical for ints) ─────────────────────
        ("imin_tie", "    return min(7, 7)\n"),
        ("imax_tie", "    return max(7, 7)\n"),
        // ── variadic > 2 args (the left fold) ─────────────────────────────────
        ("imin3", "    return min(9, 4, 6)\n"),
        ("imax3", "    return max(9, 4, 6)\n"),
        ("imin4", "    return min(5, 2, 8, 1)\n"),
        ("imax4", "    return max(5, 2, 8, 1)\n"),
        // ── nested min-in-max / max-in-min (both helpers coexist) ─────────────
        ("imin_nested", "    return min(max(1, 9), 4)\n"),
        ("imax_nested", "    return max(min(1, 9), 4)\n"),
        // ── composed with arithmetic / abs / len / control-flow ───────────────
        ("imax_arith", "    return max(2, 3) * 10\n"),
        (
            "imin_arith",
            "    a: int = 3\n    b: int = 10\n    return min(a - b, 2) + 100\n",
        ),
        ("imin_abs", "    return min(abs(-4), 2)\n"),
        ("imax_abs", "    return max(abs(-4), 2)\n"),
        (
            "imin_of_len",
            "    s: str = \"hello\"\n    return min(len(s), 3)\n",
        ),
        (
            "imin_var",
            "    n: int = 6\n    m: int = 2\n    return min(n, m)\n",
        ),
        ("imin_in_cond", "    return min(3, 5) if 3 < 5 else 0\n"),
    ]
}

/// The corpus source: the observable exports PLUS two param-boundary helpers (an
/// int min / max in the callee) and their observable callers.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    // Boundary: two ints flow across params and are min/max-ed in the callee.
    src.push_str("def imin_helper(a: int, b: int) -> int:\n    return min(a, b)\n");
    src.push_str("def call_imin_param() -> int:\n    return imin_helper(8, 3)\n");
    src.push_str("def imax_helper(a: int, b: int) -> int:\n    return max(a, b)\n");
    src.push_str("def call_imax_param() -> int:\n    return imax_helper(-8, 3)\n");
    src
}

/// Every observable export name (all probes + the two param-boundary callers —
/// NOT the `*_helper`s, which take params).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = probes().iter().map(|(n, _)| n.to_string()).collect();
    names.push("call_imin_param".to_string());
    names.push("call_imax_param".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn int_min_uses_the_select_helper_and_max_is_independent() {
    // A pure `min` program CALLS + DECLARES the min helper, and NOT the max one.
    let wat = emit("def f(a: int, b: int) -> int:\n    return min(a, b)\n").expect("min lowers");
    assert!(
        wat.contains("call $__wasm_min_i64") && wat.contains("(func $__wasm_min_i64"),
        "int min must call AND declare its select helper (else wat2wasm rejects):\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_max_i64"),
        "a pure min program must carry no dead max helper:\n{wat}"
    );

    // A pure `max` program is the mirror image.
    let watx = emit("def f(a: int, b: int) -> int:\n    return max(a, b)\n").expect("max lowers");
    assert!(
        watx.contains("call $__wasm_max_i64") && watx.contains("(func $__wasm_max_i64"),
        "int max must call AND declare its select helper:\n{watx}"
    );
    assert!(
        !watx.contains("$__wasm_min_i64"),
        "a pure max program must carry no dead min helper:\n{watx}"
    );
}

#[test]
fn variadic_min_left_folds_to_repeated_helper_calls() {
    // `min(a, b, c)` is a LEFT fold: emit a, then (emit b; call), (emit c; call)
    // — exactly TWO helper calls for three operands.
    let wat = emit("def f() -> int:\n    return min(9, 4, 6)\n").expect("variadic min lowers");
    assert_eq!(
        wat.matches("call $__wasm_min_i64").count(),
        2,
        "min(a, b, c) must fold via TWO helper calls (min(min(a,b),c)):\n{wat}"
    );
    // Four operands → three calls.
    let wat4 = emit("def f() -> int:\n    return max(5, 2, 8, 1)\n").expect("4-ary max lowers");
    assert_eq!(
        wat4.matches("call $__wasm_max_i64").count(),
        3,
        "max(a, b, c, d) must fold via THREE helper calls:\n{wat4}"
    );
}

#[test]
fn nested_min_max_declares_both_helpers_once() {
    // `min(max(1, 9), 4)` uses BOTH ops — each helper must be declared exactly
    // once (the gate walker fires each independently; a missed inner op would
    // leave a helper undeclared at its call site — a hard wat2wasm failure).
    let wat = emit("def f() -> int:\n    return min(max(1, 9), 4)\n").expect("nested lowers");
    assert_eq!(
        wat.matches("(func $__wasm_min_i64").count(),
        1,
        "min helper must be declared exactly once:\n{wat}"
    );
    assert_eq!(
        wat.matches("(func $__wasm_max_i64").count(),
        1,
        "max helper must be declared exactly once even when nested inside min:\n{wat}"
    );
}

#[test]
fn helper_declared_once_across_many_uses() {
    let wat = emit(&corpus_source()).expect("min/max corpus must lower");
    assert_eq!(
        wat.matches("(func $__wasm_min_i64").count(),
        1,
        "the min helper must be emitted EXACTLY once per module:\n{wat}"
    );
    assert_eq!(
        wat.matches("(func $__wasm_max_i64").count(),
        1,
        "the max helper must be emitted EXACTLY once per module:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// A min/max that is NOT an all-int one refuses — never silently mis-lowered.
/// The FLOAT and BOOL forms reach the [`emit_num_builtin`] refusal directly (an
/// f64 / i32 first operand); the STR form is caught even earlier by the
/// string-subset emitter (a `max("a", "b")` sits in a string RETURN position) —
/// a different, equally-honest refusal path, so it is asserted only to NOT
/// lower.
#[test]
fn non_int_min_max_refuse() {
    // float / bool → the min/max-specific refusal, pinned by needle.
    for (label, src, needle) in [
        (
            "float min",
            "def f(a: float, b: float) -> float:\n    return min(a, b)\n",
            "float (f64)",
        ),
        (
            "float max",
            "def f(a: float, b: float) -> float:\n    return max(a, b)\n",
            "float (f64)",
        ),
        (
            "bool min",
            "def f() -> bool:\n    return min(True, False)\n",
            "bool/str (i32)",
        ),
        (
            "bool max",
            "def f() -> bool:\n    return max(True, False)\n",
            "bool/str (i32)",
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
    // str → refuses (via the string-subset emitter — a str min/max is not in the
    // subset either); we only require that it does NOT silently lower.
    for (label, src) in [
        ("str min", "def f() -> str:\n    return min(\"a\", \"b\")\n"),
        ("str max", "def f() -> str:\n    return max(\"a\", \"b\")\n"),
    ] {
        assert!(
            emit(src).is_err(),
            "{label} must be refused (not silently lowered)"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

/// Parse the value out of a `name() => i64:<v>` interp line as an `f64`. Our
/// results are all small non-negative ints, exactly representable in f64.
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-minmax-{}", std::process::id()));
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
/// pairs for the observable exports — the differential ground truth.
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
fn int_min_max_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("min/max corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1339: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1339: python3 absent — witness asserted at emit level only");
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
            "min/max export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1339: {} int min/max observables (order independence, negatives, \
         ties, variadic left-fold, nested min/max, composed with arith/abs/len/\
         if-expr, and two param-boundary calls) all == live python3.",
        truth.len()
    );
}
