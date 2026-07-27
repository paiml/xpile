//! PMAT-1366 — EXECUTED witness for native-WASM `round(x)`: the FIFTH rounding
//! lowering in the lane, joining PMAT-1340's `math.floor`/`ceil`/`trunc`. Runs
//! on the scalar runtime (`C-COMPILE-RUST-TO-WASM`).
//!
//! ## What this slice delivers
//!
//! Before it, `Expr::RoundToInt` was handled NOWHERE in the WASM backend —
//! `round(x)` fell through to the generic "unsupported expression" refusal. Now
//! it lowers to TWO native instructions:
//!
//! ```text
//!   <operand>          ;; f64
//!   f64.nearest        ;; roundToIntegralTiesToEven
//!   i64.trunc_f64_s    ;; the integral f64 -> Python `int`
//! ```
//!
//! ## Why `f64.nearest` is the CPython-exact instruction
//!
//! Python's built-in `round` is round-half-to-**EVEN** (banker's rounding):
//! `round(0.5) == 0`, `round(1.5) == 2`, `round(2.5) == 2`, `round(3.5) == 4`.
//! WASM's `f64.nearest` is IEEE-754 `roundToIntegralTiesToEven` — the same rule,
//! in one instruction.
//!
//! This is the ONE rounding op where the naive lowering is WRONG: Rust's
//! `f64::round` is half-away-from-**ZERO** (`2.5 -> 3`), which is exactly why
//! the Rust/Ruchy lanes must emit `round_ties_even()`. A WASM emitter that
//! reached for `f64.floor(x + 0.5)` — the other obvious encoding — would answer
//! `3` for `round(2.5)` and `-2` for `round(-2.5)`.
//!
//! The corpus therefore carries EVERY half-way case in both signs. Those probes
//! are the mutation-proof: swap `f64.nearest` for a half-away-from-zero encoding
//! and `round_half_2_5` / `round_half_0_5` / `round_half_neg_2_5` all diverge
//! from live python3.
//!
//! ## The trap boundary (documented, NOT claimed CPython-equal)
//!
//! `i64.trunc_f64_s` TRAPS when the rounded value leaves `[i64::MIN, i64::MAX]`,
//! and on `±inf` / `nan`. That is the same boundary PMAT-1340's rounding narrow
//! has: `2**63` is outside the lane's modeled int domain, and for `inf`/`nan`
//! CPython itself RAISES (`OverflowError` / `ValueError`), so trapping refuses to
//! fabricate a value rather than emit a wrong one. The corpus stays inside
//! `2**53` so every differential is exact in f64.
//!
//! ## What stays REFUSED
//!
//! The 2-argument `round(x, n)` forms (`Expr::RoundToDigits` /
//! `Expr::RoundIntToDigits`) refuse with a SPECIFIC message. Decimal rounding is
//! not a scaling problem: `round(2.675, 2)` is `2.67` in CPython (and in the
//! Rust lane, which formats to `n` places and reparses), but a
//! `x*10**n` → round → `/10**n` encoding answers `2.68`. Verified, not assumed —
//! see `two_arg_round_refuses_and_says_why`.
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

/// Each `(name, return-kw, body)` becomes a zero-arg `def <name>() -> <kw>` that
/// embeds its inputs as literals and returns a `round(...)`-derived value.
fn probes() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // ── THE TIES: round-half-to-EVEN, both signs. These are the probes that
        //    a half-away-from-zero lowering fails.
        ("round_half_0_5", "int", "    x: float = 0.5\n    return round(x)\n"),
        ("round_half_1_5", "int", "    x: float = 1.5\n    return round(x)\n"),
        ("round_half_2_5", "int", "    x: float = 2.5\n    return round(x)\n"),
        ("round_half_3_5", "int", "    x: float = 3.5\n    return round(x)\n"),
        ("round_half_4_5", "int", "    x: float = 4.5\n    return round(x)\n"),
        (
            "round_half_neg_0_5",
            "int",
            "    x: float = -0.5\n    return round(x)\n",
        ),
        (
            "round_half_neg_1_5",
            "int",
            "    x: float = -1.5\n    return round(x)\n",
        ),
        (
            "round_half_neg_2_5",
            "int",
            "    x: float = -2.5\n    return round(x)\n",
        ),
        (
            "round_half_neg_3_5",
            "int",
            "    x: float = -3.5\n    return round(x)\n",
        ),
        // ── the ordinary (non-tie) directions, both signs ──────────────────────
        ("round_down", "int", "    x: float = 2.25\n    return round(x)\n"),
        ("round_up", "int", "    x: float = 2.75\n    return round(x)\n"),
        (
            "round_neg_down",
            "int",
            "    x: float = -2.25\n    return round(x)\n",
        ),
        (
            "round_neg_up",
            "int",
            "    x: float = -2.75\n    return round(x)\n",
        ),
        // ── the zeros, incl. IEEE -0.0 (CPython's `-0` int is `0`) ─────────────
        ("round_zero", "int", "    x: float = 0.0\n    return round(x)\n"),
        (
            "round_neg_zero",
            "int",
            "    x: float = -0.0\n    return round(x)\n",
        ),
        (
            "round_tiny_neg",
            "int",
            "    x: float = -0.25\n    return round(x)\n",
        ),
        // ── an already-integral float is the identity ─────────────────────────
        (
            "round_integral",
            "int",
            "    x: float = 7.0\n    return round(x)\n",
        ),
        (
            "round_integral_neg",
            "int",
            "    x: float = -7.0\n    return round(x)\n",
        ),
        // ── magnitude: still exact in f64 (well inside 2**53) ─────────────────
        (
            "round_big",
            "int",
            "    x: float = 1000000.5\n    return round(x)\n",
        ),
        (
            "round_big_neg",
            "int",
            "    x: float = -1000001.5\n    return round(x)\n",
        ),
        // ── a NEGATIVE literal operand straight through ───────────────────────
        ("round_neg_literal", "int", "    return round(-1.5)\n"),
        // ── COMPOSED: arithmetic feeding round, and round feeding arithmetic ──
        (
            "round_of_sum",
            "int",
            "    a: float = 1.25\n    b: float = 1.25\n    return round(a + b)\n",
        ),
        (
            "round_of_div",
            "int",
            "    a: float = 5.0\n    b: float = 2.0\n    return round(a / b)\n",
        ),
        (
            "round_times_two",
            "int",
            "    x: float = 2.5\n    return round(x) * 2\n",
        ),
        (
            "round_plus_int",
            "int",
            "    x: float = 3.5\n    return round(x) + 10\n",
        ),
        (
            "round_diff",
            "int",
            "    a: float = 2.5\n    b: float = 3.5\n    return round(a) - round(b)\n",
        ),
        // ── COMPOSED with the other NumBuiltin ops (abs / sqrt / floor) ───────
        (
            "round_of_abs",
            "int",
            "    x: float = -2.5\n    return round(abs(x))\n",
        ),
        (
            "round_of_sqrt",
            "int",
            "import math\n    x: float = 2.0\n    return round(math.sqrt(x))\n",
        ),
        (
            "round_vs_floor",
            "int",
            "import math\n    x: float = 2.5\n    return round(x) - math.floor(x)\n",
        ),
        // ── control flow: if-expression + a real `if` statement ───────────────
        (
            "round_in_ifexpr",
            "int",
            "    x: float = -2.5\n    return round(x) if x < 0.0 else 0\n",
        ),
        (
            "round_in_if_stmt",
            "int",
            "    x: float = 6.5\n    r: int = round(x)\n    if r > 6:\n        return r\n    return 0\n",
        ),
        // ── bound to a LOCAL, then used (the `Stmt::Let` typed path) ──────────
        (
            "round_let_bound",
            "int",
            "    x: float = 9.5\n    r: int = round(x)\n    return r + 1\n",
        ),
        // ── accumulated in a LOOP ─────────────────────────────────────────────
        (
            "round_in_loop",
            "int",
            "    t: int = 0\n    i: int = 0\n    x: float = 1.5\n    while i < 3:\n        t = t + round(x)\n        i = i + 1\n    return t\n",
        ),
        // ── the f-string thread: `str(round(x))` through a `+`-concat ─────────
        (
            "str_round_eq",
            "bool",
            "    x: float = 2.5\n    return \"r=\" + str(round(x)) == \"r=2\"\n",
        ),
        // ── a comparison over the int result ──────────────────────────────────
        (
            "round_cmp",
            "bool",
            "    x: float = 2.5\n    return round(x) == 2\n",
        ),
    ]
}

/// The corpus source: the observable exports PLUS a param-boundary helper (the
/// float crosses a call and is rounded in the callee) and its observable caller.
fn corpus_source() -> String {
    let mut src = String::new();
    // A probe body may open with an `import math` line (written first in the
    // tuple); hoist it to module scope where Python expects it.
    let mut needs_math = false;
    let mut bodies = String::new();
    for (name, ret, body) in probes() {
        let body = match body.strip_prefix("import math\n") {
            Some(rest) => {
                needs_math = true;
                rest
            }
            None => body,
        };
        bodies.push_str(&format!("def {name}() -> {ret}:\n{body}\n"));
    }
    if needs_math {
        src.push_str("import math\n");
    }
    src.push_str(&bodies);
    // Boundary: a float flows across a param and is rounded in the callee.
    src.push_str("def round_helper(x: float) -> int:\n    return round(x)\n");
    src.push_str("def call_round_param() -> int:\n    return round_helper(-4.5)\n");
    // Boundary: the ROUNDED int flows back out across a second call.
    src.push_str("def twice(n: int) -> int:\n    return n * 2\n");
    src.push_str("def call_round_result() -> int:\n    return twice(round(2.5))\n");
    src
}

/// Every observable export name (all probes + the two boundary callers — NOT
/// the helpers, which take a param).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = probes().iter().map(|(n, _, _)| n.to_string()).collect();
    names.push("call_round_param".to_string());
    names.push("call_round_result".to_string());
    names
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

/// `round(x)` is the native `f64.nearest` + the `int` narrow — no helper
/// function, and NOT one of the directional rounding instructions.
#[test]
fn round_lowers_to_f64_nearest_and_narrows() {
    let wat = emit("def f(x: float) -> int:\n    return round(x)\n").expect("round must lower");
    assert!(
        wat.contains("f64.nearest"),
        "round(x) must use the round-half-to-EVEN instruction:\n{wat}"
    );
    assert!(
        wat.contains("i64.trunc_f64_s"),
        "Python's 1-arg round returns an int, so the integral f64 must narrow:\n{wat}"
    );
    for wrong in ["f64.floor", "f64.ceil", "f64.trunc\n"] {
        assert!(
            !wat.contains(wrong),
            "round(x) must not lower through the DIRECTIONAL rounding op {wrong:?} \
             (they answer differently at every non-integral input):\n{wat}"
        );
    }
    // No runtime helper: the emitted `$f` body is exactly the three lines.
    // (The module always carries the floordiv/floormod helpers, so scope the
    // check to the function body rather than the whole WAT.)
    let body = wat
        .split_once("(func $f ")
        .expect("emitted $f")
        .1
        .split_once("\n  )")
        .expect("closing paren")
        .0;
    assert!(
        !body.contains("call "),
        "round(x) needs no runtime helper — it is two native instructions:\n{body}"
    );
}

/// `round(int)` never reaches the backend: the frontend (PMAT-502ak) folds it to
/// the identity, so no rounding instruction is emitted at all.
#[test]
fn round_of_int_is_the_frontend_identity() {
    let wat = emit("def f() -> int:\n    return round(5)\n").expect("round(int) must lower");
    assert!(
        !wat.contains("f64.nearest"),
        "round(int) is the identity — it must not round anything:\n{wat}"
    );
    assert!(
        wat.contains("i64.const 5"),
        "round(5) must fold to the literal 5:\n{wat}"
    );
}

/// The rounded value is int-VALUED everywhere an int is expected — including a
/// FORMAT position, which is the PMAT-1342 touchpoint a new int-valued node is
/// most likely to miss (`concat_operand_is_int`).
#[test]
fn round_in_fstring_wraps_via_str_int() {
    let wat = emit("def g(x: float) -> str:\n    return f\"r={round(x)}\"\n")
        .expect("f-string of round must lower");
    assert!(
        wat.contains("f64.nearest") && wat.contains("call $__wasm_int_to_str"),
        "an f-string interpolating round(x) must round THEN str(int)-materialise:\n{wat}"
    );
    // The BARE single-interpolation form (`f"{round(x)}"`, no literal text)
    // travels the `FormatSpec` fold, a separate branch.
    let bare = emit("def g(x: float) -> str:\n    return f\"{round(x)}\"\n")
        .expect("bare f-string of round must lower");
    assert!(
        bare.contains("f64.nearest") && bare.contains("call $__wasm_int_to_str"),
        "a BARE f-string of round(x) must also str(int)-materialise:\n{bare}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The 2-argument `round(x, n)` forms refuse with a message that names the
/// reason — and the reason is TRUE: the obvious `x*10**n` encoding really does
/// disagree with CPython, which this test computes rather than asserts.
#[test]
fn two_arg_round_refuses_and_says_why() {
    for (label, src) in [
        (
            "round(float, n)",
            "def f(x: float) -> float:\n    return round(x, 2)\n",
        ),
        (
            "round(int, n)",
            "def f() -> int:\n    return round(12350, -2)\n",
        ),
    ] {
        let err = match emit(src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains("2-argument `round(x, n)`"),
            "{label} must refuse with the SPECIFIC 2-arg message, got: {err}"
        );
    }

    // The refusal's justification, checked numerically: CPython's decimal
    // rounding and a binary scale-round-unscale differ at 2.675.
    let x: f64 = 2.675;
    let naive = (x * 100.0).round_ties_even() / 100.0;
    assert_eq!(
        naive, 2.68,
        "scale-round-unscale must land on 2.68 — the value the refusal message \
         says would be WRONG"
    );
    // CPython answers 2.67 (verified live below when python3 is present).
    if let Some(out) = Command::new("python3")
        .arg("-c")
        .arg("print(repr(round(2.675, 2)))")
        .output()
        .ok()
        .filter(|o| o.status.success())
    {
        let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(
            got, "2.67",
            "CPython must answer 2.67 for round(2.675, 2) — the divergence the \
             refusal message cites"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

/// Parse the value out of a `name() => <ty>:<v>` interp line as an `f64`.
///
/// `wasm-interp` prints integer exports UNSIGNED, so a negative `i64` arrives as
/// its two's-complement image (`-2` prints as `18446744073709551614`). Half this
/// corpus is negative by construction — the round-half-to-even ties in the minus
/// direction are the whole point — so the width-correct REINTERPRET is
/// load-bearing here, not a nicety: parsing the raw digits as f64 would compare
/// `1.8446744073709552e19` against CPython's `-2`.
fn parse_export_f64(stdout: &str, name: &str) -> f64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let (ty, raw) = line
        .rsplit_once(" => ")
        .and_then(|(_, v)| v.split_once(':'))
        .unwrap_or_else(|| panic!("malformed export line {line:?}"));
    let raw = raw.trim();
    match ty.trim() {
        "i64" => raw
            .parse::<u64>()
            .map(|u| u as i64 as f64)
            .unwrap_or_else(|_| panic!("parse i64 for {name} from {line:?}")),
        "i32" => raw
            .parse::<u32>()
            .map(|u| u as i32 as f64)
            .unwrap_or_else(|_| panic!("parse i32 for {name} from {line:?}")),
        "f64" | "f32" => raw
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("parse float for {name} from {line:?}")),
        other => panic!("unexpected export type {other:?} in {line:?}"),
    }
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    // A per-process-unique dir keeps parallel libtest threads from racing on
    // `prog.wat` (the multi-execution-path witness gotcha).
    let dir = std::env::temp_dir().join(format!("xpile-wasm-round-{}", std::process::id()));
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
fn round_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("round corpus must lower");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1366: WABT (wat2wasm / wasm-interp) absent — witness asserted at \
             emit level only (the executed leg runs every export and value-matches \
             live python3 on the identical source when WABT is present)."
        );
        return;
    }

    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1366: python3 absent — witness asserted at emit level only");
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
            "round export `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }

    // The mutation-proof, stated explicitly: the half-way probes are the ones a
    // half-away-from-zero lowering gets wrong. Assert their CPython values are
    // the banker's-rounding answers, so a future "fix" that flips the
    // instruction cannot pass by also flipping the expectation.
    for (name, want) in [
        ("round_half_0_5", 0.0),
        ("round_half_1_5", 2.0),
        ("round_half_2_5", 2.0),
        ("round_half_3_5", 4.0),
        ("round_half_4_5", 4.0),
        ("round_half_neg_0_5", 0.0),
        ("round_half_neg_1_5", -2.0),
        ("round_half_neg_2_5", -2.0),
        ("round_half_neg_3_5", -4.0),
    ] {
        let got = parse_export_f64(&stdout, name);
        assert_eq!(
            got, want,
            "`{name}` must be the round-half-to-EVEN answer {want} (a \
             half-away-from-zero lowering would differ)"
        );
    }

    eprintln!(
        "PMAT-1366: {} round(x) observables (every half-way tie in both signs, \
         the non-tie directions, ±0.0, integral identities, composed with \
         arithmetic / abs / sqrt / floor / if-expr / if-stmt / let / while, \
         str(round) + f-string, and two call-boundary crossings) all == live \
         python3.",
        truth.len()
    );
}
