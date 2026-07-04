//! PMAT-1262 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for native-WASM `x in xs` / `x not in xs` over a named `list[int]` /
//! `list[float]` — the FIRST list-MEMBERSHIP op the WASM lane lowers.
//!
//! ## Why this witness exists (the PMAT-1244 guard, membership edition)
//!
//! `x in xs` reaches the codegen as an [`xpile_meta_hir::Expr::ListContains`]
//! (and `x not in xs` as that node under a `UnOp::Not`). A hand-built-HIR witness
//! would prove the emit handles that node but NOT that the production
//! `PythonFrontend` actually emits it with the fields the emit reads
//! (`list`/`elem`) from real `in` / `not in` source. This witness lowers REAL
//! Python through the same profile the CLI uses for `--target wasm`, emits,
//! assembles + runs in WABT, and asserts the executed scalar VALUE-MATCHES
//! CPython running the byte-identical program.
//!
//! ## What each probe certifies
//!
//! Each `go()` folds several membership tests (present / absent / `not in`, at
//! the first / middle / last / no position, and over negatives) into one scalar,
//! so a single matching result certifies the linear scan finds a hit exactly when
//! CPython's `in` does and yields `False` (0) on the empty and no-match cases.
//! The `nested_sum` probe (`sum(ys) in xs`, `max(ys) in xs`) is the load-bearing
//! GATE-HOLE guard: the needle nests a `sum`/`max` reduction, so the module must
//! declare `$__wasm_list_sum_i64` AND `$__wasm_list_minmax_i64` AND
//! `$__wasm_list_contains_i64` — a missed gate-walker recursion would leave a
//! helper undeclared and `wat2wasm` would hard-fail here.
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
    let dir =
        std::env::temp_dir().join(format!("xpile-contains-e2e-{}-{}", std::process::id(), tag));
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
// EXECUTED int membership — present / absent / not-in / positions / negatives,
// each value-matched to CPython running the identical program.
// ---------------------------------------------------------------------------

#[test]
fn int_membership_executes_and_matches_cpython() {
    // (tag, full go() body, CPython fingerprint of the identical program).
    let cases: &[(&str, &str, i64)] = &[
        // a present hit (+1), an absent `in` (+0), and an absent `not in` (+100).
        (
            "basic_in",
            "    xs: list[int] = [3, 7, 11, 15]\n    acc: int = 0\n    if 11 in xs:\n        acc = acc + 1\n    if 8 in xs:\n        acc = acc + 10\n    if 8 not in xs:\n        acc = acc + 100\n    return acc",
            101,
        ),
        // hit at the FIRST, MIDDLE, LAST position + a miss — proves the scan
        // covers the whole payload, not just an endpoint.
        (
            "all_positions",
            "    xs: list[int] = [5, 6, 7, 8, 9]\n    acc: int = 0\n    if 5 in xs:\n        acc = acc + 1\n    if 7 in xs:\n        acc = acc + 10\n    if 9 in xs:\n        acc = acc + 100\n    if 100 in xs:\n        acc = acc + 1000\n    return acc",
            111,
        ),
        // NEGATIVE and ZERO elements — the i64 compare is signed.
        (
            "negatives",
            "    xs: list[int] = [-3, 0, 4]\n    acc: int = 0\n    if -3 in xs:\n        acc = acc + 1\n    if 0 in xs:\n        acc = acc + 10\n    if -3 not in xs:\n        acc = acc + 100\n    return acc",
            11,
        ),
        // GATE-HOLE guard: the needle nests `sum`/`max` reductions, so the module
        // declares `$__wasm_list_sum_i64` + `$__wasm_list_minmax_i64` +
        // `$__wasm_list_contains_i64` — a missed gate-walker recursion into the
        // needle would leave one undeclared and `wat2wasm` would hard-fail.
        (
            "nested_sum",
            "    xs: list[int] = [6, 10, 15]\n    ys: list[int] = [1, 2, 3]\n    acc: int = 0\n    if sum(ys) in xs:\n        acc = acc + 1\n    if max(ys) in xs:\n        acc = acc + 10\n    return acc",
            1,
        ),
    ];

    // The pipeline must ALWAYS lower + emit (frontend reachability), even
    // without WABT.
    for (tag, body, _) in cases {
        let src = format!("def go() -> int:\n{body}\n");
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit membership probe {tag}: {:?}",
            emit(&src)
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1262: WABT absent — emit-only int check passed, execution skipped");
        return;
    }

    for (tag, body, expect) in cases {
        let src = format!("def go() -> int:\n{body}\n");
        let got = run_i64(&src, tag);
        assert_eq!(
            got, *expect,
            "int membership {tag}: wasm={got} cpython={expect}"
        );
    }
}

// ---------------------------------------------------------------------------
// EXECUTED membership over a list PARAM — the common shape (a helper that tests
// `v in xs` for its list argument), not just a local literal.
// ---------------------------------------------------------------------------

#[test]
fn param_membership_executes_and_matches_cpython() {
    // `probe(xs, v)` returns 1 iff v in xs; go() folds a hit (×10) and a miss.
    let src = "def probe(xs: list[int], v: int) -> int:\n    if v in xs:\n        return 1\n    return 0\ndef go() -> int:\n    xs: list[int] = [7, 8, 9]\n    return probe(xs, 8) * 10 + probe(xs, 100)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit param membership: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1262: WABT absent — emit-only param check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "param");
    assert_eq!(got, 10, "param membership: wasm={got} cpython=10");
}

// ---------------------------------------------------------------------------
// EXECUTED float membership — the f64 element load + `f64.eq` compare twin.
// ---------------------------------------------------------------------------

#[test]
fn float_membership_executes_and_matches_cpython() {
    let body = "    xs: list[float] = [1.5, 2.5, 3.5]\n    acc: float = 0.0\n    if 2.5 in xs:\n        acc = acc + 1.0\n    if 9.9 not in xs:\n        acc = acc + 10.0\n    if 1.5 in xs:\n        acc = acc + 100.0\n    return acc";
    let src = format!("def go() -> float:\n{body}\n");

    assert!(
        emit(&src).is_ok(),
        "pipeline failed to lower+emit float membership: {:?}",
        emit(&src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1262: WABT absent — emit-only float check passed, execution skipped");
        return;
    }

    let got = run_f64(&src, "float_in");
    assert!(
        (got - 111.0).abs() < 1e-9,
        "float membership: wasm={got} cpython=111.0"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED empty-list membership — `x in []` is False (0), the scan-exhausted
// boundary. Uses a param so the empty list is a real length-0 header.
// ---------------------------------------------------------------------------

#[test]
fn empty_list_membership_is_false() {
    let src = "def probe(xs: list[int]) -> int:\n    if 5 in xs:\n        return 1\n    return 0\ndef go() -> int:\n    xs: list[int] = []\n    return probe(xs)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit empty membership: {:?}",
        emit(src)
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1262: WABT absent — emit-only empty check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "empty");
    assert_eq!(got, 0, "empty-list membership must be False (0), got {got}");
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — the membership shapes NOT yet in the lane must error, not
// silently miscompile.
// ---------------------------------------------------------------------------

#[test]
fn non_name_list_membership_refuses_honestly() {
    // `x in [1, 2, 3]` — a list LITERAL, not a name; the lane needs a declared
    // `list[scalar]` base-pointer. Refuse rather than miscompile.
    let src = "def go() -> int:\n    if 2 in [1, 2, 3]:\n        return 1\n    return 0\n";
    let err = emit(src).expect_err("membership in a list literal must refuse");
    assert!(
        err.contains("non-name list"),
        "non-name membership refusal should name the shape, got: {err}"
    );
}

#[test]
fn bool_list_membership_refuses_honestly() {
    // `True in bs` over a `list[bool]` — the elements load as i32 (0/1), not the
    // i64/f64 the membership helpers compare. Refuse rather than miscompile.
    let src =
        "def go(bs: list[bool]) -> int:\n    if True in bs:\n        return 1\n    return 0\n";
    let err = emit(src).expect_err("list[bool] membership must refuse");
    assert!(
        err.contains("i32") || err.contains("element kind"),
        "list[bool] membership refusal should name the element kind, got: {err}"
    );
}
