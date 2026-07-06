//! PMAT-1332 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM) witness
//! for the native-WASM `any(xs)` / `all(xs)` reduction over a `list[int]` /
//! `list[float]` — the SCALAR truthiness twins of the PMAT-1251 `list[bool]`
//! fold — plus the honest-refusal boundary.
//!
//! ## What this closes
//!
//! Python applies `any`/`all` truthiness PER ELEMENT: an `int`/`float` counts as
//! True iff it is NONZERO. The `depyler-frontend` lowers this to a per-element
//! truthiness map wrapped in a reduce — `BoolReduce { list: Map { list: xs,
//! lambda: __x != 0 (int) / __x != 0.0 (float) } }`. Before PMAT-1332 the WASM
//! subset refused that `Expr::Map` (a non-name list); the PMAT-1258 refusal
//! witness even PINNED `all(list[int])` as refused. PMAT-1332 recognises that
//! exact identity-truthiness map over a bare list NAME
//! ([`list_scalar_truthy_target`]) and folds the raw i64/f64 payload by nonzero
//! via a stride-matched helper — no general `Map` lowering needed.
//!
//! ## Why the executed edges matter (four genuine divergence classes)
//!
//!   1. **The empty-list IDENTITIES.** `all([]) == True`, `any([]) == False` —
//!      the classic Python `any`/`all` divergence. The helper reports the
//!      identity by falling out of the loop with `local.get $is_all`; a wrong
//!      seed/identity gets them backwards.
//!   2. **Short-circuit POSITION.** `all` breaks on the first falsey (zero)
//!      element, `any` on the first truthy (nonzero) one — swept FIRST / MID /
//!      LAST / never so a mis-placed break or a fold that reads past the decisive
//!      element is caught.
//!   3. **NONZERO ≠ ==1 (the int-vs-bool distinction).** A `list[bool]` element
//!      is 0/1, so the bool helper's truthiness is trivial; an `int` element is
//!      any i64, so a NEGATIVE or a LARGE value must still read truthy. `any([-1])
//!      == True` and `all([-3, 7]) == True` pin that the fold tests `!= 0`, not
//!      `> 0` or `== 1`.
//!   4. **IEEE float truthiness.** `bool(-0.0)` is FALSE and `bool(nan)` is TRUE
//!      — both fall out of a single `f64.ne 0.0` with no special case
//!      (`-0.0 != 0.0` is false, `nan != 0.0` is true). `any([-0.0, 0.0]) ==
//!      False` is the load-bearing case (a naive bit-test of the sign bit would
//!      wrongly read `-0.0` as truthy).
//!
//! ## Technique
//!
//! `wasm-interp --run-all-exports` invokes each zero-arg export and prints its
//! scalar result; an `any`/`all` result IS a single i32 bool, read directly as
//! `go() => i32:{0,1}`. Each reference value was computed by CPython running the
//! byte-identical `any`/`all` call. Gated on [`wasm_runtime_available`] — a clean
//! skip without WABT; the always-run guard still asserts the full pipeline
//! LOWERS + EMITS + CALLS the right helper so the frontend→codegen wiring is
//! locked on free CI regardless.
//!
//! Contracts: C-COMPILE-RUST-TO-WASM (the emit lane under test) + C-WASM-HEAP
//! (the self-contained probe binds a list LITERAL, whose construction
//! bump-allocates a record — `call $__alloc` — so the module carries the heap it
//! cites; the fold itself reads the payload in place and allocates nothing).

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// A unique-per-call counter so parallel libtest threads never race on a shared
/// `go.wat` path (the multi-execution-path WABT witness gotcha — pid alone is
/// not enough when one process runs many cases concurrently).
static SEQ: AtomicU64 = AtomicU64::new(0);

/// The lowering profile the CLI uses for `--target wasm`
/// (`crates/xpile/src/main.rs::lowering_profile_for(Target::Wasm)`).
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

/// The FULL pipeline: Python source → meta-HIR → WAT text.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// A zero-arg `go() -> bool` program: bind a `list[<ty>]` literal, then reduce it
/// with `op` (`all` / `any`). `elems` is the comma-separated literal body (`""`
/// for the empty list — the load-bearing identity edge).
fn probe(ty: &str, elems: &str, op: &str) -> String {
    format!("def go() -> bool:\n    xs: list[{ty}] = [{elems}]\n    return {op}(xs)\n")
}

/// Assemble the real-emitted WAT + run its zero-arg `go` export in WABT,
/// returning the printed `i32` payload as a 0/1 bool.
fn run_bool(src: &str, tag: &str) -> bool {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "xpile-scalar-truthy-{}-{}-{}",
        std::process::id(),
        seq,
        tag
    ));
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
// Always-run pipeline guard (no WABT needed) — reachability + helper routing.
// ---------------------------------------------------------------------------

/// The FULL Python→WASM pipeline LOWERS + EMITS + CALLS the correct
/// stride-matched helper for each scalar element kind (a frontend or codegen
/// change that breaks the `any(xs)`/`all(xs)` → truthiness-map recognition fails
/// HERE even when the executed witnesses skip).
#[test]
fn scalar_truthy_pipeline_routes_by_element_kind() {
    // int → the i64-stride nonzero fold.
    let int_wat = emit(&probe("int", "1, 0, 2", "all")).expect("int truthiness lowers+emits");
    assert!(
        int_wat.contains("$__wasm_list_int_truthy_reduce")
            && int_wat.contains("call $__wasm_list_int_truthy_reduce"),
        "list[int] any/all declares AND calls the int truthiness helper:\n{int_wat}"
    );
    // float → the f64-stride nonzero fold.
    let flt_wat = emit(&probe("float", "1.0, 0.0", "any")).expect("float truthiness lowers+emits");
    assert!(
        flt_wat.contains("$__wasm_list_float_truthy_reduce")
            && flt_wat.contains("call $__wasm_list_float_truthy_reduce"),
        "list[float] any/all declares AND calls the float truthiness helper:\n{flt_wat}"
    );
    // The int module calls the INT helper, not the float one (correct routing).
    assert!(
        !int_wat.contains("call $__wasm_list_float_truthy_reduce"),
        "list[int] any/all must NOT call the float helper:\n{int_wat}"
    );
    // is_all selector pushed as an i32 immediate at the call site.
    assert!(
        int_wat.contains("i32.const 1"),
        "all → is_all=1 pushed at the int call site:\n{int_wat}"
    );
    assert!(
        flt_wat.contains("i32.const 0"),
        "any → is_all=0 pushed at the float call site:\n{flt_wat}"
    );
}

// ---------------------------------------------------------------------------
// EXECUTED adversarial edges — value-matched against CPython (via python3).
// ---------------------------------------------------------------------------

#[test]
fn int_any_all_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        eprintln!("SKIP int_any_all_execute_and_match_cpython: WABT not installed");
        return;
    }
    // (tag, list-literal body, op, CPython result).
    let cases: &[(&str, &str, &str, bool)] = &[
        // ---- empty-list IDENTITIES ----
        ("i_all_empty", "", "all", true),  // all([]) == True
        ("i_any_empty", "", "any", false), // any([]) == False
        // ---- `all`: short-circuits FALSE on the first zero, swept ----
        ("i_all_zero_first", "0, 1, 2", "all", false),
        ("i_all_zero_mid", "1, 2, 0, 3", "all", false),
        ("i_all_zero_last", "1, 2, 3, 0", "all", false),
        ("i_all_nonzero", "1, 2, 3", "all", true), // loop exhausts
        // ---- `any`: short-circuits TRUE on the first nonzero, swept ----
        ("i_any_nz_first", "5, 0, 0", "any", true),
        ("i_any_nz_mid", "0, 0, 7, 0", "any", true),
        ("i_any_nz_last", "0, 0, 0, 9", "any", true),
        ("i_any_all_zero", "0, 0, 0", "any", false), // loop exhausts
        // ---- NONZERO ≠ ==1: negatives and large values are truthy ----
        ("i_any_neg", "-1", "any", true),    // bool(-1) is True
        ("i_all_neg", "-3, 7", "all", true), // both nonzero
        ("i_all_neg_zero", "-3, 0", "all", false),
        ("i_any_big", "1000000", "any", true),
        // ---- single element (n == 1), both directions/values ----
        ("i_all_single_nz", "4", "all", true),
        ("i_all_single_zero", "0", "all", false),
        ("i_any_single_nz", "4", "any", true),
        ("i_any_single_zero", "0", "any", false),
    ];
    for &(tag, elems, op, expected) in cases {
        let got = run_bool(&probe("int", elems, op), tag);
        assert_eq!(
            got, expected,
            "{op}(list[int] [{elems}]) executed {got}, expected (CPython) {expected}"
        );
    }
    eprintln!(
        "=== PMAT-1332: {} list[int] any/all edges execute CPython-exact ===",
        cases.len()
    );
}

#[test]
fn float_any_all_execute_and_match_cpython() {
    if !wasm_runtime_available() {
        eprintln!("SKIP float_any_all_execute_and_match_cpython: WABT not installed");
        return;
    }
    let cases: &[(&str, &str, &str, bool)] = &[
        // ---- empty-list IDENTITIES ----
        ("f_all_empty", "", "all", true),
        ("f_any_empty", "", "any", false),
        // ---- short-circuit position ----
        ("f_all_zero_first", "0.0, 1.5, 2.5", "all", false),
        ("f_all_zero_mid", "1.5, 2.5, 0.0, 3.5", "all", false),
        ("f_all_nonzero", "1.5, 2.5, 3.5", "all", true),
        ("f_any_nz_first", "1.5, 0.0, 0.0", "any", true),
        ("f_any_nz_last", "0.0, 0.0, 9.5", "any", true),
        ("f_any_all_zero", "0.0, 0.0", "any", false),
        // ---- IEEE: bool(-0.0) is FALSE (the load-bearing case) ----
        ("f_any_neg_zero", "-0.0, 0.0", "any", false), // both falsey
        ("f_all_neg_zero", "-0.0", "all", false),      // bool(-0.0) is False
        // ---- negatives/fractions are truthy (nonzero) ----
        ("f_any_neg", "-1.5", "any", true),
        ("f_all_neg", "-2.5, 3.5", "all", true),
        ("f_all_tiny", "0.0001", "all", true), // a small nonzero is truthy
        // ---- single element ----
        ("f_all_single_nz", "4.0", "all", true),
        ("f_all_single_zero", "0.0", "all", false),
        ("f_any_single_zero", "0.0", "any", false),
    ];
    for &(tag, elems, op, expected) in cases {
        let got = run_bool(&probe("float", elems, op), tag);
        assert_eq!(
            got, expected,
            "{op}(list[float] [{elems}]) executed {got}, expected (CPython) {expected}"
        );
    }
    eprintln!(
        "=== PMAT-1332: {} list[float] any/all edges execute CPython-exact \
         (incl. bool(-0.0) == False) ===",
        cases.len()
    );
}

// ---------------------------------------------------------------------------
// Honest-refusal boundary — the still-unsupported truthiness shapes refuse.
// ---------------------------------------------------------------------------

/// The scalar int/float maps now EMIT, but the NON-scalar / non-name truthiness
/// shapes still collapse the pipeline to a hard `Err` (never an emit-but-wrong
/// module). Pinned so a future change that begins ACCEPTING one of them without a
/// real implementation is caught.
#[test]
fn scalar_truthy_unsupported_shapes_refuse() {
    // 1. `list[str]` truthiness — a `len(__x) != 0` map, NOT a `!= 0` scalar map,
    //    so `list_scalar_truthy_target` does not match and the list[str] element
    //    kind is refused (the string payload fold is deferred).
    let str_truthiness =
        "def f() -> bool:\n    words: list[str] = [\"a\", \"\"]\n    return all(words)\n";
    assert!(
        emit(str_truthiness).is_err(),
        "all(list[str]) (len-truthiness map) must refuse in the WASM subset"
    );

    // 2. A dict-sourced `any(d)` over an INT-keyed dict EMITS (PMAT-1334 folds the
    //    keys); the STR-keyed form now EMITS TOO (PMAT-1336 folds `len(k) != 0`
    //    DIRECTLY out of the region's str-pointer keys via
    //    `$__wasm_hash_strkey_truthy_reduce` — see `str_truthy_reduce_witness.rs`).
    //    Pin that it lowers cleanly (no longer a refusal boundary here).
    let dict_str_truthiness =
        "def f() -> bool:\n    d: dict[str, int] = {\"a\": 10, \"\": 20}\n    return any(d)\n";
    assert!(
        emit(dict_str_truthiness).is_ok(),
        "any(str-keyed dict) now folds via the fused str-key helper (PMAT-1336)"
    );

    // 3. A list LITERAL argument (a temporary, not a name).
    let literal = "def f() -> bool:\n    return all([1, 0, 2])\n";
    assert!(
        emit(literal).is_err(),
        "all([...]) over a list literal must refuse (bind it to a name first)"
    );

    // 4. The LAZY short-circuiting GENERATOR form (a per-element predicate
    //    lambda) — the frontend tags it `short_circuit`, which the subset defers.
    let generator = "def f(xs: list[int]) -> bool:\n    return any(x > 0 for x in xs)\n";
    assert!(
        emit(generator).is_err(),
        "any(<generator>) (lazy predicate) must refuse in the WASM subset"
    );
}
