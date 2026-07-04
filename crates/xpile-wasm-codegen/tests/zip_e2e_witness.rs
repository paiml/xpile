//! PMAT-1261 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM)
//! witness for native-WASM `for a, b in zip(xs, ys)` over TWO named
//! `list[scalar]` sources — the SECOND paired for-loop the WASM lane lowers
//! (after `enumerate`, PMAT-1260).
//!
//! ## Why this witness exists (the PMAT-1244 guard, zip edition)
//!
//! `zip` reaches the codegen as a [`xpile_meta_hir::Stmt::ForEachPair`] with
//! [`xpile_meta_hir::PairIterKind::Zip`] carrying the SECOND source as a boxed
//! `Expr` — a different shape from both the single-var
//! [`xpile_meta_hir::Stmt::ForEach`] and the `enumerate` pair. A hand-built-HIR
//! witness would prove the desugar handles that shape but NOT that the
//! production `PythonFrontend` actually emits it with the fields the desugar
//! reads (`first`/`second`/`iter` + the boxed `other`). This witness lowers
//! REAL Python through the same profile the CLI uses for `--target wasm`,
//! emits, assembles + runs in WABT, and asserts the executed scalar
//! VALUE-MATCHES CPython running the byte-identical program.
//!
//! ## What each probe certifies
//!
//! The fold `acc = acc*31 + a*W + b` folds BOTH loop targets into one
//! order-sensitive scalar, so a single matching result certifies the first
//! element (`a` from `xs`) AND the second element (`b` from `ys`) AND their
//! pairing order across the whole iteration. The SHORTEST-ITERABLE contract —
//! `zip` stops at the shorter operand — is exercised in BOTH directions (xs
//! longer, ys longer) and at the empty-operand boundary (zero iterations). A
//! `continue`, and a `list[float]` pair load, are each exercised.
//!
//! Gated on [`wasm_runtime_available`] — a clean skip (still asserting the full
//! pipeline LOWERS + EMITS) on a host without WABT, so free CI stays green.

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The lowering profile the CLI uses for `--target wasm`.
fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

/// Lower Python source → meta-HIR the way the CLI does for a WASM target.
fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// The FULL pipeline: Python source → meta-HIR → WAT text.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// Assemble the real-emitted WAT + run its zero-arg `go` export in WABT.
fn run_go(wat: &str, tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-zip-e2e-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("go.wat");
    let wasm_path = dir.join("go.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "wasm-interp run failed for {tag}: stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    let line = stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=>"))
        .unwrap_or_else(|| panic!("no `go` export in interp output for {tag}:\n{stdout}"));
    line.rsplit(':').next().unwrap().trim().to_string()
}

/// Run a `go() -> int` probe and return the result as a SIGNED i64
/// (wasm-interp prints i64 as unsigned decimal).
fn run_i64(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<u64>()
        .unwrap_or_else(|_| panic!("parse i64 result {raw:?} for {tag}")) as i64
}

/// Run a `go() -> float` probe and return the result as f64.
fn run_f64(src: &str, tag: &str) -> f64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let raw = run_go(&wat, tag);
    raw.parse::<f64>()
        .unwrap_or_else(|_| panic!("parse f64 result {raw:?} for {tag}"))
}

// ---------------------------------------------------------------------------
// EXECUTED int zip — both elements folded, value-matched to CPython, with the
// SHORTEST-ITERABLE contract exercised in both directions + at empty.
// ---------------------------------------------------------------------------

#[test]
fn int_zip_executes_and_matches_cpython() {
    // (tag, full go() body, CPython fingerprint of the identical program).
    let cases: &[(&str, &str, i64)] = &[
        // Equal length: pairs (1,4)(2,5)(3,6).
        (
            "basic",
            "    xs: list[int] = [1,2,3]\n    ys: list[int] = [4,5,6]\n    acc: int = 0\n    for a, b in zip(xs, ys):\n        acc = acc * 31 + a * 100 + b\n    return acc",
            106605,
        ),
        // xs LONGER — zip must stop after ys (2 iters): (1,10)(2,20).
        (
            "a_longer",
            "    xs: list[int] = [1,2,3,4,5]\n    ys: list[int] = [10,20]\n    acc: int = 0\n    for a, b in zip(xs, ys):\n        acc = acc * 31 + a * 1000 + b\n    return acc",
            33330,
        ),
        // ys LONGER — zip must stop after xs (2 iters): (7,100)(8,200).
        (
            "b_longer",
            "    xs: list[int] = [7,8]\n    ys: list[int] = [100,200,300,400]\n    acc: int = 0\n    for a, b in zip(xs, ys):\n        acc = acc * 31 + a * 1000 + b\n    return acc",
            228300,
        ),
        // `continue` skips the (20,2) pair — proves the shared counter advances
        // BEFORE the body, so `continue` (→ `br` back to the while cond) never
        // spins and the surviving pairs are exact.
        (
            "continue",
            "    xs: list[int] = [10,20,30,40]\n    ys: list[int] = [1,2,3,4]\n    acc: int = 0\n    for a, b in zip(xs, ys):\n        if a == 20:\n            continue\n        acc = acc * 31 + a * 100 + b\n    return acc",
            1059058,
        ),
        // EMPTY second operand — zero iterations, acc untouched.
        (
            "empty",
            "    xs: list[int] = [1,2,3]\n    ys: list[int] = []\n    acc: int = 0\n    for a, b in zip(xs, ys):\n        acc = acc * 31 + a * 100 + b\n    return acc",
            0,
        ),
    ];

    // The pipeline must ALWAYS lower + emit (frontend reachability), even
    // without WABT.
    for (tag, body, _) in cases {
        let src = format!("def go() -> int:\n{body}\n");
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit zip probe {tag}: {:?}",
            emit(&src)
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1261: WABT absent — emit-only int check passed, execution skipped");
        return;
    }

    for (tag, body, expect) in cases {
        let src = format!("def go() -> int:\n{body}\n");
        let got = run_i64(&src, tag);
        assert_eq!(got, *expect, "int zip {tag}: wasm={got} cpython={expect}");
    }
}

// ---------------------------------------------------------------------------
// EXECUTED float zip — an f64 pair load under the zip driver, shortest-iterable
// (xs has 3, ys has 2 → 2 products summed).
// ---------------------------------------------------------------------------

#[test]
fn float_zip_executes_and_matches_cpython() {
    // pairs (1.5,10.0)(2.5,20.0): acc = 1.5*10 + 2.5*20 = 65.0.
    let body = "    xs: list[float] = [1.5,2.5,3.5]\n    ys: list[float] = [10.0,20.0]\n    acc: float = 0.0\n    for a, b in zip(xs, ys):\n        acc = acc + a * b\n    return acc";
    let src = format!("def go() -> float:\n{body}\n");

    assert!(
        emit(&src).is_ok(),
        "pipeline failed to lower+emit float zip probe: {:?}",
        emit(&src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1261: WABT absent — emit-only float check passed, execution skipped");
        return;
    }

    let got = run_f64(&src, "float_zip");
    assert!(
        (got - 65.0).abs() < 1e-9,
        "float zip: wasm={got} cpython=65.0"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — the zip shapes NOT yet in the lane must error, not silently
// miscompile.
// ---------------------------------------------------------------------------

#[test]
fn zip_over_nonname_second_source_refuses_honestly() {
    // `zip(xs, sorted(ys))` — the second iterable is a computed list expr, not
    // a name; the lane needs a declared `list[scalar]` to recover its element
    // type. Refuse rather than miscompile.
    let src = "def go() -> int:\n    xs: list[int] = [1,2,3]\n    ys: list[int] = [3,1,2]\n    acc: int = 0\n    for a, b in zip(xs, sorted(ys)):\n        acc = acc * 31 + a * 100 + b\n    return acc\n";
    let err = emit(src).expect_err("zip over a non-name second source must refuse");
    assert!(
        err.contains("zip"),
        "non-name zip refusal should name zip, got: {err}"
    );
}
