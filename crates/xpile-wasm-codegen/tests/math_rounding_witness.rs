//! PMAT-1340 — EXECUTED witness for native-WASM `math.floor(x)` /
//! `math.ceil(x)` / `math.trunc(x)`: the ROUNDING scalar numeric builtins
//! (`Expr::NumBuiltin`) the WASM lane emits, after PMAT-1338's `abs` and
//! PMAT-1339's `min`/`max`. Runs on the scalar runtime (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## What this slice delivers
//!
//! WASM has a native single-instruction rounding op for each direction, matching
//! CPython's rounding SENSE exactly:
//!
//!   * `math.floor(x)` → `f64.floor` (toward −∞): `floor(-2.7) == -3`;
//!   * `math.ceil(x)`  → `f64.ceil`  (toward +∞): `ceil(-2.3)  == -2`;
//!   * `math.trunc(x)` → `f64.trunc` (toward 0):  `trunc(-2.9) == -2`.
//!
//! Python's `math.floor`/`ceil`/`trunc` return an `int`, so the (already
//! integral) f64 is narrowed with `i64.trunc_f64_s` — an integral value → i64 is
//! a no-op truncation, no extra rounding. The three ops DIFFER precisely on a
//! negative non-integer: `floor(-2.9) == -3` but `trunc(-2.9) == -2` (and
//! `ceil(-2.9) == -2`), so the corpus pins that discriminator so a swapped
//! opcode cannot pass.
//!
//! CPython-EXACT over the lane's whole modeled int domain: for a finite `x`
//! whose rounding lands in `[i64::MIN, i64::MAX]` the result equals CPython's.
//! The ONE boundary — `|value| >= 2**63`, `±inf`, `nan` — is where
//! `i64.trunc_f64_s` TRAPS (`2**63` is outside the modeled i64 range, the same
//! limit `abs(i64::MIN)` / int overflow have; for `inf`/`nan` CPython itself
//! RAISES), so a trap refuses to fabricate a value rather than emit a wrong one.
//!
//! `math.sqrt` and the transcendentals REFUSE: `f64.sqrt(-1.0)` is NaN where
//! CPython's `math.sqrt(-1.0)` RAISES ValueError — a divergence over a normal
//! negative input, the same NaN/order class that makes the lane refuse a float
//! `min`/`max`.
//!
//! ## The load-bearing edges
//!
//!   * ROUNDING SENSE — `floor`/`ceil`/`trunc` over the SAME negative input
//!     (`-2.9`) give three DIFFERENT ints (−3 / −2 / −2), so a swapped opcode is
//!     caught;
//!   * POSITIVE non-integer — `floor(2.7)==2`, `ceil(2.3)==3`, `trunc(2.7)==2`;
//!   * EXACT integer float — `floor(5.0)==ceil(5.0)==trunc(5.0)==5` (no drift);
//!   * the INT result flows into int arithmetic (`* 10`, `+ 100`), through
//!     `abs(...)` (`abs(math.floor(-3.5))==4` — floor feeds the int-abs helper),
//!     under a float `abs` arg (`math.floor(abs(-2.5))==2`), and in an
//!     `if`-expression;
//!   * a float flows across a FUNCTION param and is rounded in the callee.
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
/// -> int` embedding its inputs as float literals and returning an int
/// floor/ceil/trunc value. Every RESULT is non-negative so `wasm-interp` (which
/// prints an i64 UNSIGNED) and live python3 agree through the shared f64
/// differential (negatives are shifted `+ 100` to stay non-negative).
fn probes() -> Vec<(&'static str, &'static str)> {
    vec![
        // ── positive non-integer (the plain rounding) ─────────────────────────
        ("mfloor_pos", "    return math.floor(2.7)\n"),
        ("mceil_pos", "    return math.ceil(2.3)\n"),
        ("mtrunc_pos", "    return math.trunc(2.7)\n"),
        // ── exact-integer float (no drift; all three agree) ───────────────────
        ("mfloor_exact", "    return math.floor(5.0)\n"),
        ("mceil_exact", "    return math.ceil(5.0)\n"),
        ("mtrunc_exact", "    return math.trunc(5.0)\n"),
        // ── the DISCRIMINATOR: same negative input, three different ints ──────
        //    floor(-2.9)=-3, ceil(-2.9)=-2, trunc(-2.9)=-2 (shifted +100).
        ("mfloor_neg", "    return math.floor(-2.9) + 100\n"),
        ("mceil_neg", "    return math.ceil(-2.9) + 100\n"),
        ("mtrunc_neg", "    return math.trunc(-2.9) + 100\n"),
        // ── trunc vs floor on a negative near-integer ─────────────────────────
        //    floor(-2.1)=-3 but trunc(-2.1)=-2 (shifted +100).
        ("mfloor_neg_small", "    return math.floor(-2.1) + 100\n"),
        ("mtrunc_neg_small", "    return math.trunc(-2.1) + 100\n"),
        // ── ceil rounds a positive non-integer UP; floor a negative DOWN ──────
        ("mceil_up", "    return math.ceil(0.1)\n"),
        ("mfloor_down", "    return math.floor(-0.1) + 100\n"),
        // ── the int result composed with int arithmetic ───────────────────────
        ("mfloor_arith", "    return math.floor(3.7) * 10\n"),
        (
            "mceil_arith",
            "    x: float = 2.2\n    return math.ceil(x) + 100\n",
        ),
        // ── nested with abs: floor result feeds the INT-abs helper ────────────
        ("mabs_of_floor", "    return abs(math.floor(-3.5))\n"),
        // ── float abs feeds floor (abs is float here; no int-abs helper) ──────
        ("mfloor_of_absf", "    return math.floor(abs(-2.5))\n"),
        // ── in an if-expression ───────────────────────────────────────────────
        (
            "mtrunc_in_cond",
            "    return math.trunc(2.9) if 2 < 5 else 0\n",
        ),
    ]
}

/// The corpus source: `import math`, the observable exports, PLUS three
/// param-boundary helpers (a float rounded in the callee) and their observable
/// callers.
fn corpus_source() -> String {
    let mut src = String::from("import math\n");
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    // Boundary: a float flows across a param and is rounded in the callee.
    src.push_str("def mfloor_helper(x: float) -> int:\n    return math.floor(x)\n");
    src.push_str("def call_mfloor_param() -> int:\n    return mfloor_helper(3.7)\n");
    src.push_str("def mceil_helper(x: float) -> int:\n    return math.ceil(x)\n");
    src.push_str("def call_mceil_param() -> int:\n    return mceil_helper(-2.3) + 100\n");
    src
}

/// Every observable export name (all probes + the two param-boundary callers —
/// NOT the `*_helper`s, which take params).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = probes().iter().map(|(n, _)| n.to_string()).collect();
    names.push("call_mfloor_param".to_string());
    names.push("call_mceil_param".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn floor_ceil_trunc_emit_native_rounding_plus_narrow() {
    // Each rounding op emits its NATIVE instruction followed by the int narrow —
    // no helper function, no libm import.
    for (fname, opcode) in [
        ("floor", "f64.floor"),
        ("ceil", "f64.ceil"),
        ("trunc", "f64.trunc"),
    ] {
        let src = format!("import math\ndef f(x: float) -> int:\n    return math.{fname}(x)\n");
        let wat = emit(&src).unwrap_or_else(|e| panic!("math.{fname} must lower: {e}"));
        assert!(
            wat.contains(opcode),
            "math.{fname} must emit the native {opcode}:\n{wat}"
        );
        assert!(
            wat.contains("i64.trunc_f64_s"),
            "math.{fname} must narrow the integral f64 → i64 (Python int result):\n{wat}"
        );
        // Native ops need NO libm import (they are emitted INLINE — unlike int
        // abs/min/max, which call a select helper). The `$f` body carries the
        // opcode directly; the baseline floordiv/floormod prelude helpers are
        // unrelated.
        assert!(
            !wat.contains("(import"),
            "a rounding builtin must not pull a libm import:\n{wat}"
        );
        // The rounding op sits in the `$f` body immediately before the narrow —
        // i.e. `<opcode>` then `i64.trunc_f64_s`, no intervening `call`.
        let body = wat.split_once("(func $f ").map(|(_, r)| r).unwrap_or(&wat);
        assert!(
            body.contains(&format!("{opcode}\n")) || body.contains(&format!("{opcode} ")),
            "math.{fname}'s native op must be inline in the `$f` body:\n{wat}"
        );
    }
}

#[test]
fn floor_and_trunc_differ_on_a_negative() {
    // The load-bearing discriminator: over the SAME negative input floor and
    // trunc pick DIFFERENT opcodes — so a copy-paste that emitted the same op
    // for both would still assemble but MISCOMPUTE. Assert the two WATs differ in
    // exactly the rounding instruction.
    let wf = emit("import math\ndef f() -> int:\n    return math.floor(-2.9)\n").expect("floor");
    let wt = emit("import math\ndef f() -> int:\n    return math.trunc(-2.9)\n").expect("trunc");
    assert!(wf.contains("f64.floor") && !wf.contains("f64.trunc"));
    assert!(wt.contains("f64.trunc") && !wt.contains("f64.floor"));
}

#[test]
fn fstring_over_rounding_materialises_via_str_int() {
    // `f"{math.floor(x)}"` is int-VALUED, so it materialises via the sign-aware
    // `$__wasm_int_to_str` (the PMAT-1340 `concat_operand_is_int` extension) —
    // it must NOT refuse.
    let wat = emit("import math\ndef f() -> str:\n    return f\"{math.floor(2.9)}\"\n")
        .expect("f-string over math.floor must lower (int → str(int))");
    assert!(
        wat.contains("$__wasm_int_to_str"),
        "f\"{{math.floor(x)}}\" must stringify the int via $__wasm_int_to_str:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

#[test]
fn sqrt_and_transcendentals_and_int_arg_refuse() {
    // sqrt + transcendentals are out of the WASM subset (NaN-domain / no
    // bit-exact instruction) — refused, never silently mis-lowered.
    for (label, fname) in [
        ("sqrt", "sqrt"),
        ("sin", "sin"),
        ("cos", "cos"),
        ("exp", "exp"),
        ("log10", "log10"),
    ] {
        let src = format!("import math\ndef f(x: float) -> float:\n    return math.{fname}(x)\n");
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!("math.{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains("transcendental") || err.contains("not in the WASM subset"),
            "math.{label} refusal should name the subset limit, got: {err}"
        );
    }
    // An INT argument (`math.floor(5)`) needs the int→float widen, which is NOT
    // in the WASM subset — it must refuse (not silently lower to a wrong path).
    assert!(
        emit("import math\ndef f() -> int:\n    return math.floor(5)\n").is_err(),
        "math.floor over an int arg must refuse (int→float widen unsupported)"
    );
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-mround-{}", std::process::id()));
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
fn floor_ceil_trunc_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("math rounding corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1340: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1340: python3 absent — witness asserted at emit level only");
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
            "rounding export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    assert_eq!(
        truth.len(),
        20,
        "expected 20 observable rounding probes value-matched, got {}",
        truth.len()
    );
}
