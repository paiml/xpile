//! PMAT-1258 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM)
//! witness for the native-WASM `any(xs)` / `all(xs)` reduction over a
//! `list[bool]` (`Expr::BoolReduce`, PMAT-1251), plus the honest-refusal
//! boundary.
//!
//! ## Why this witness exists (two genuine gaps)
//!
//! The PMAT-1251 `any`/`all` witness in `src/tests.rs` builds the meta-HIR
//! BY HAND and drives ONE happy-path input; the PMAT-1257 full-pipeline fuzz
//! (`list_e2e_frontend_witness.rs`) deliberately scoped itself to the
//! *allocating* list-op family (`sorted`/`reversed`/`a + b`/`xs[lo:hi]`), so
//! `any`/`all` — a NON-allocating bool fold — is covered by NEITHER. That
//! leaves the family exposed on two axes this witness closes:
//!
//!   1. **Frontend reachability (the PMAT-1244 guard).** Every hand-built-HIR
//!      witness proves the EMIT arm is correct but NOT that the op is
//!      *reachable from Python* — the exact class of regression PMAT-1244
//!      shipped (an emit arm green while the frontend produced a different
//!      `Expr` shape, so the construct still hit `emit_expr`'s catch-all
//!      refusal). This witness lowers REAL Python through the production
//!      [`PythonFrontend`] with the CLI's `--target wasm` profile, so a future
//!      change that keeps the emit arm but breaks the `any(xs)`/`all(xs)` →
//!      `Expr::BoolReduce` frontend→codegen wiring fails HERE even though the
//!      hand-built witness stays green.
//!
//!   2. **The empty-list identity edge.** `all([]) == True` and
//!      `any([]) == False` are the single most classic Python divergence — a
//!      naive fold that seeds the accumulator from `xs[0]`, or reports the
//!      wrong identity on an exhausted loop, gets them backwards. The
//!      happy-path witness (a distinct-element non-empty list) cannot exercise
//!      them; a short-circuit-position sweep (decisive element FIRST / MID /
//!      LAST / never) cannot be read off a single fixed input either. This
//!      witness pins all of it against CPython (values verified via python3).
//!
//! ## Technique
//!
//! `wasm-interp --run-all-exports` invokes each zero-arg export and prints its
//! scalar result. An `any`/`all` result IS a single scalar bool, so — unlike
//! the list-VALUED ops in `list_e2e_frontend_witness.rs`, which fold the whole
//! result list into one fingerprint — each probe returns its bool directly as
//! `go() => i32:{0,1}`. The reference value for each program was computed by
//! CPython running the byte-identical `any`/`all` call.
//!
//! Gated on [`wasm_runtime_available`] — a clean skip on a host without WABT.
//! Even when skipped, `bool_reduce_pipeline_lowers_and_emits` still asserts the
//! full Python→WASM pipeline LOWERS + EMITS the helper, so the PMAT-1244
//! frontend-reachability guard holds on free CI regardless of WABT.
//!
//! Contracts: C-COMPILE-RUST-TO-WASM (the emit lane under test) + C-WASM-HEAP.
//! The bool FOLD itself allocates nothing — `$__wasm_list_bool_reduce` reads the
//! `list[bool]` payload in place — but a self-contained probe binds the input as
//! a list LITERAL, and building that literal bump-allocates a fresh record
//! (`call $__alloc`), so the emitted module carries the bump heap and cites
//! C-WASM-HEAP (the emitted WAT's own `xpile-contract: C-WASM-HEAP` line).

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The lowering profile the CLI uses for `--target wasm`
/// (`crates/xpile/src/main.rs::lowering_profile_for(Target::Wasm)`): a binding
/// copy IS Python's object sharing (`Reference`), and WASM can express a
/// runtime abort (`unreachable`) for the empty-iterable guard.
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

/// The FULL pipeline: Python source → meta-HIR → WAT text. Either stage's
/// error collapses to `Err`, so a refusal test does not care WHICH stage
/// refused, only that the pipeline never silently emits a miscompile.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// A zero-arg `go() -> bool` program: bind a `list[bool]` literal, then reduce
/// it with `op` (`all` / `any`). `elems` is the comma-separated bool literal
/// body (`""` for the empty list — the load-bearing identity edge).
fn bool_probe(elems: &str, op: &str) -> String {
    format!("def go() -> bool:\n    xs: list[bool] = [{elems}]\n    return {op}(xs)\n")
}

/// Assemble the real-emitted WAT + run its zero-arg `go` export in WABT,
/// returning the printed `i32` payload as a 0/1 bool.
fn run_bool(src: &str, tag: &str) -> bool {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let dir = std::env::temp_dir().join(format!("xpile-boolreduce-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("go.wat");
    let wasm_path = dir.join("go.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");

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
    // `go() => i32:0` / `go() => i32:1` — the single executed bool result.
    let line = stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=> i32:"))
        .unwrap_or_else(|| panic!("no `go` i32 export in interp output for {tag}:\n{stdout}"));
    match line.rsplit(':').next().unwrap().trim() {
        "0" => false,
        "1" => true,
        other => panic!("`{tag}` bool result is not 0/1: {other:?}\n{stdout}"),
    }
}

// ---------------------------------------------------------------------------
// Always-run pipeline guard (no WABT needed) — the PMAT-1244 reachability lock.
// ---------------------------------------------------------------------------

/// The FULL Python→WASM pipeline LOWERS + EMITS `any`/`all` over a `list[bool]`
/// — asserted with no WABT so free CI exercises the frontend→codegen shape
/// wiring even when the executed witnesses skip. The bool FOLD reads the payload
/// in place (allocates nothing), but the self-contained probe's list LITERAL is
/// bump-allocated, so the module carries the heap it cites (C-WASM-HEAP).
#[test]
fn bool_reduce_pipeline_lowers_and_emits() {
    for (op, tag) in [("all", "all"), ("any", "any")] {
        let wat = emit(&bool_probe("True, False, True", op))
            .unwrap_or_else(|e| panic!("pipeline must lower+emit {tag}: {e}"));
        assert!(
            wat.contains("$__wasm_list_bool_reduce")
                && wat.contains("call $__wasm_list_bool_reduce"),
            "{tag}: module declares AND calls the bool-reduce helper:\n{wat}"
        );
        // is_all selector pushed as an i32 immediate: 1 for all, 0 for any.
        let want = if op == "all" {
            "i32.const 1"
        } else {
            "i32.const 0"
        };
        assert!(
            wat.contains(want),
            "{tag}: is_all selector `{want}` pushed at the call site:\n{wat}"
        );
    }
    // The empty-list program must also lower+emit (the identity edge is emitted
    // by the SAME helper — no separate empty-list codepath to miss).
    let empty = emit(&bool_probe("", "all")).expect("empty-list all lowers+emits");
    assert!(empty.contains("call $__wasm_list_bool_reduce"));
}

// ---------------------------------------------------------------------------
// EXECUTED adversarial edges — value-matched against CPython (via python3).
// ---------------------------------------------------------------------------

#[test]
fn any_all_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        eprintln!("SKIP any_all_execute_and_match_cpython: WABT not installed");
        return;
    }
    // (tag, list-literal body, op, CPython result). The decisive element sweeps
    // FIRST / MID / LAST / never, and the empty-list identities are pinned side
    // by side (`all([]) == True`, `any([]) == False`).
    let cases: &[(&str, &str, &str, bool)] = &[
        // ---- the empty-list IDENTITIES (the classic divergence) ----
        ("all_empty", "", "all", true),  // all([]) == True
        ("any_empty", "", "any", false), // any([]) == False
        // ---- `all`: short-circuits FALSE on the first falsey element ----
        ("all_falsey_first", "False, True, True", "all", false), // decisive @ pos 0
        ("all_falsey_mid", "True, True, False, True", "all", false), // decisive mid
        ("all_falsey_last", "True, True, True, False", "all", false), // decisive @ last
        ("all_all_true", "True, True, True", "all", true),       // never decisive → loop exhausts
        // ---- `any`: short-circuits TRUE on the first truthy element ----
        ("any_truthy_first", "True, False, False", "any", true), // decisive @ pos 0
        ("any_truthy_mid", "False, False, True, False", "any", true), // decisive mid
        ("any_truthy_last", "False, False, False, True", "any", true), // decisive @ last
        ("any_all_false", "False, False, False", "any", false),  // never decisive → loop exhausts
        // ---- single-element (n == 1) both directions, both values ----
        ("all_single_true", "True", "all", true),
        ("all_single_false", "False", "all", false),
        ("any_single_true", "True", "any", true),
        ("any_single_false", "False", "any", false),
    ];
    for &(tag, elems, op, expected) in cases {
        let got = run_bool(&bool_probe(elems, op), tag);
        assert_eq!(
            got, expected,
            "{op}([{elems}]) executed {got}, expected (CPython) {expected}"
        );
    }
    eprintln!(
        "=== PMAT-1258: {} any/all edge cases execute CPython-exact \
         (empty-list identities + short-circuit FIRST/MID/LAST/never) ===",
        cases.len()
    );
}

// ---------------------------------------------------------------------------
// Honest-refusal boundary — the pipeline REFUSES (never silently miscompiles).
// ---------------------------------------------------------------------------

/// The three unsupported `any`/`all` shapes each collapse the pipeline to a
/// hard `Err`, never an emit-but-wrong module. Verified against a LIVE
/// `xpile transpile --target wasm` earlier; pinned here so a future frontend
/// or codegen change that begins ACCEPTING one of them (without a real
/// implementation) is caught.
#[test]
fn any_all_unsupported_shapes_refuse() {
    // 1. `list[int]` per-element truthiness — the frontend wraps the list in an
    //    `Expr::Map` (a non-name list), which the WASM subset refuses.
    let int_truthiness = "def f() -> bool:\n    xs: list[int] = [1, 0, 2]\n    return all(xs)\n";
    assert!(
        emit(int_truthiness).is_err(),
        "all(xs) over a list[int] (truthiness map) must refuse in the WASM subset"
    );

    // 2. A list LITERAL argument (a temporary, not a name).
    let literal = "def f() -> bool:\n    return all([True, False])\n";
    assert!(
        emit(literal).is_err(),
        "all([...]) over a list literal must refuse (bind it to a name first)"
    );

    // 3. The LAZY short-circuiting GENERATOR form (a per-element predicate
    //    lambda) — the frontend tags it `short_circuit`, which the subset defers.
    let generator = "def f(xs: list[int]) -> bool:\n    return any(x > 0 for x in xs)\n";
    assert!(
        emit(generator).is_err(),
        "any(<generator>) (lazy predicate) must refuse in the WASM subset"
    );
}
