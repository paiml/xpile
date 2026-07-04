//! PMAT-1260 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM)
//! witness for native-WASM `for i, x in enumerate(xs[, start])` over a named
//! `list[scalar]` — the first PAIRED for-loop the WASM lane lowers.
//!
//! ## Why this witness exists (the PMAT-1244 guard, enumerate edition)
//!
//! `enumerate` reaches the codegen as a [`xpile_meta_hir::Stmt::ForEachPair`]
//! with [`xpile_meta_hir::PairIterKind::Enumerate`] — a DIFFERENT shape from
//! the single-var [`xpile_meta_hir::Stmt::ForEach`]. A hand-built-HIR witness
//! would prove the desugar handles that shape but NOT that the production
//! `PythonFrontend` actually emits it with the fields the desugar reads
//! (`first`/`second`/`iter`/`start`). This witness lowers REAL Python through
//! the same profile the CLI uses for `--target wasm`, emits, assembles + runs
//! in WABT, and asserts the executed scalar VALUE-MATCHES CPython running the
//! byte-identical program.
//!
//! ## What each probe certifies
//!
//! The fold `acc = acc*31 + i*W + x` folds BOTH loop targets into one
//! order-sensitive scalar, so a single matching result certifies the index
//! (`first`) AND the element (`second`) AND their pairing order across the
//! whole iteration — not just one element. `start` (0 / +10 / −3), a
//! `continue` (proving the increment-before-body ordering still advances), and
//! a `list[float]` element load under the enumerate driver are each exercised.
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
    let dir = std::env::temp_dir().join(format!("xpile-enum-e2e-{}-{}", std::process::id(), tag));
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
// EXECUTED int enumerate — index + element folded, value-matched to CPython.
// ---------------------------------------------------------------------------

#[test]
fn int_enumerate_executes_and_matches_cpython() {
    // (tag, full go() body, CPython fingerprint of the identical program).
    let cases: &[(&str, &str, i64)] = &[
        // start=0 (default): pairs (0,5)(1,7)(2,9)(3,11).
        (
            "basic",
            "    xs: list[int] = [5,7,9,11]\n    acc: int = 0\n    for i, x in enumerate(xs):\n        acc = acc * 31 + i * 100 + x\n    return acc",
            258572,
        ),
        // start=10 positional: index offset by +10.
        (
            "start10",
            "    xs: list[int] = [2,4,6]\n    acc: int = 0\n    for i, x in enumerate(xs, 10):\n        acc = acc * 31 + i * 1000 + x\n    return acc",
            9965052,
        ),
        // start=-3: NEGATIVE indices — the offset add carries the sign.
        (
            "start_neg",
            "    xs: list[int] = [8,8,8,8]\n    acc: int = 0\n    for i, x in enumerate(xs, -3):\n        acc = acc * 31 + i * 10 + x\n    return acc",
            -666988,
        ),
        // `continue` on the first two indices — proves the counter is advanced
        // BEFORE the body, so `continue` (→ `br` back to the while cond) never
        // spins forever and the surviving indices are exact.
        (
            "continue",
            "    xs: list[int] = [10,20,30,40]\n    acc: int = 0\n    for i, x in enumerate(xs):\n        if i < 2:\n            continue\n        acc = acc * 31 + i * 100 + x\n    return acc",
            7470,
        ),
    ];

    // The pipeline must ALWAYS lower + emit (frontend reachability), even
    // without WABT.
    for (tag, body, _) in cases {
        let src = format!("def go() -> int:\n{body}\n");
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit enumerate probe {tag}: {:?}",
            emit(&src)
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1260: WABT absent — emit-only int check passed, execution skipped");
        return;
    }

    for (tag, body, expect) in cases {
        let src = format!("def go() -> int:\n{body}\n");
        let got = run_i64(&src, tag);
        assert_eq!(
            got, *expect,
            "int enumerate {tag}: wasm={got} cpython={expect}"
        );
    }
}

// ---------------------------------------------------------------------------
// EXECUTED float enumerate — f64 element load under the enumerate driver, with
// the index used in an int comparison to gate the accumulation.
// ---------------------------------------------------------------------------

#[test]
fn float_enumerate_executes_and_matches_cpython() {
    // pairs (0,1.5)(1,2.5)(2,3.5): index 0 adds x, index 2 adds x*100.
    let body = "    xs: list[float] = [1.5,2.5,3.5]\n    acc: float = 0.0\n    for i, x in enumerate(xs):\n        if i == 0:\n            acc = acc + x\n        if i == 2:\n            acc = acc + x * 100.0\n    return acc";
    let src = format!("def go() -> float:\n{body}\n");

    assert!(
        emit(&src).is_ok(),
        "pipeline failed to lower+emit float enumerate probe: {:?}",
        emit(&src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1260: WABT absent — emit-only float check passed, execution skipped");
        return;
    }

    let got = run_f64(&src, "float_enum");
    assert!(
        (got - 351.5).abs() < 1e-9,
        "float enumerate: wasm={got} cpython=351.5"
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — the paired-loop shapes NOT yet in the lane must error, not
// silently miscompile.
// ---------------------------------------------------------------------------

#[test]
fn zip_over_named_lists_now_lowers() {
    // PMAT-1261 landed `zip(<named list>, <named list>)`; what used to refuse
    // here now lowers. The EXECUTED value-match lives in `zip_e2e_witness.rs`;
    // this asserts only that the paired-loop shape reaches emit without error.
    let src = "def go() -> int:\n    xs: list[int] = [1,2,3]\n    ys: list[int] = [4,5,6]\n    acc: int = 0\n    for a, b in zip(xs, ys):\n        acc = acc * 31 + a * 100 + b\n    return acc\n";
    assert!(
        emit(src).is_ok(),
        "zip over named lists should lower after PMAT-1261: {:?}",
        emit(src)
    );
}

#[test]
fn dict_items_pairs_refuses_honestly() {
    // `for k, v in d.items()` — a `PairIterKind::Pairs` over a list of
    // 2-tuples. Tuple-element destructuring is not in the WASM lane; refuse
    // rather than miscompile.
    let src = "def go() -> int:\n    d: dict[int, int] = {1: 10, 2: 20}\n    acc: int = 0\n    for k, v in d.items():\n        acc = acc * 31 + k * 100 + v\n    return acc\n";
    let err = emit(src).expect_err("d.items() paired-iteration must refuse in the WASM lane");
    assert!(
        !err.is_empty(),
        "d.items() refusal should carry a message, got empty"
    );
}

#[test]
fn enumerate_over_nonname_source_refuses_honestly() {
    // `enumerate(sorted(xs))` — the iterable is a computed list expr, not a
    // name; the lane needs a declared `list[scalar]` to recover the element
    // type. Refuse rather than miscompile.
    let src = "def go() -> int:\n    xs: list[int] = [3,1,2]\n    acc: int = 0\n    for i, x in enumerate(sorted(xs)):\n        acc = acc * 31 + i * 100 + x\n    return acc\n";
    let err = emit(src).expect_err("enumerate over a non-name source must refuse");
    assert!(
        err.contains("enumerate"),
        "non-name enumerate refusal should name enumerate, got: {err}"
    );
}
