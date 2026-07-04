//! PMAT-1257 — EXECUTED FULL-PIPELINE (Python source → meta-HIR → WASM)
//! witness for the native-WASM allocating LIST-op family
//! (`sorted` / `reversed` / `a + b` concat / `xs[lo:hi]` slice) and set
//! ALGEBRA (`|` / `&` / `-` / `^`), plus the honest-refusal boundary.
//!
//! ## Why this witness exists (the PMAT-1244 guard)
//!
//! Every other list/set witness in this crate constructs the meta-HIR
//! [`Module`] *by hand* and drives [`emit_module`] directly. That proves the
//! EMIT lane is correct, but it does NOT prove the op is *reachable from
//! Python* — the exact gap PMAT-1244 shipped: the set-ordering emit arm was
//! green while the Python frontend produced a DIFFERENT `Expr` shape, so
//! `s1 <= s2` still errored at `emit_expr`'s catch-all. A hand-built-HIR
//! witness cannot catch that class of regression.
//!
//! This witness closes the loop: it lowers REAL Python source through the
//! production [`PythonFrontend`] with the SAME lowering profile the CLI uses
//! for `--target wasm` (`AliasSemantics::Reference` + `runtime_abort`), emits
//! WASM through the production [`emit_module`], assembles + runs it in WABT,
//! and asserts the executed result VALUE-MATCHES CPython. If a future change
//! keeps the emit arm but breaks the frontend→codegen shape wiring, THIS test
//! fails even though the hand-built-HIR witnesses stay green.
//!
//! ## Fingerprint technique
//!
//! `wasm-interp --run-all-exports` only invokes zero-arg exports, and a
//! single export returns one scalar — but a list op's correctness lives in
//! the WHOLE result list. So each probe folds the result into one
//! order-sensitive fingerprint (`acc = acc*31 + v` for ints, `acc = acc*7 + v`
//! for floats) via a `for v in ys` loop — itself a shipped list op. A single
//! matching scalar therefore certifies the entire ordered result, not just
//! one element. The reference values below were computed by CPython running
//! the byte-identical program (see the doc-comment on each constant).
//!
//! Gated on [`wasm_runtime_available`] — a clean skip (still asserting the
//! full pipeline LOWERS + EMITS) on a host without WABT, so free CI stays
//! green.

use std::path::Path;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The lowering profile the CLI uses for `--target wasm`
/// (`crates/xpile/src/main.rs::lowering_profile_for(Target::Wasm)`): a binding
/// copy IS Python's object sharing (`Reference`), and WASM can express a
/// runtime abort (`unreachable`) for the empty-iterable loop-var-leak guard.
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
/// error collapses to `Err` — so a refusal test does not care WHICH stage
/// refused, only that the pipeline never silently emits a miscompile.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// Assemble the real-emitted WAT + run its zero-arg `go` export in WABT,
/// returning the printed export line's numeric payload as a string.
fn run_go(wat: &str, tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-list-e2e-{}-{}", std::process::id(), tag));
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
    // `go() => i64:<n>` / `go() => f64:<n>` — the single executed result.
    let line = stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=>"))
        .unwrap_or_else(|| panic!("no `go` export in interp output for {tag}:\n{stdout}"));
    line.rsplit(':').next().unwrap().trim().to_string()
}

/// Run a `go() -> int` probe and return the result as a SIGNED i64.
/// wasm-interp prints i64 as UNSIGNED decimal, so parse `u64` then reinterpret
/// the two's-complement bit pattern — this is how a negative fold result
/// (e.g. `sorted([...,-9])`) round-trips.
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

/// Build a zero-arg `go() -> int` program: a list `setup`, then an
/// order-sensitive fold over the result `ys`.
fn int_probe(setup: &str) -> String {
    format!(
        "def go() -> int:\n{setup}\n    acc: int = 0\n    for v in ys:\n        acc = acc * 31 + v\n    return acc\n"
    )
}

/// Build a zero-arg `go() -> float` program (float fold).
fn float_probe(setup: &str) -> String {
    format!(
        "def go() -> float:\n{setup}\n    acc: float = 0.0\n    for v in ys:\n        acc = acc * 7.0 + v\n    return acc\n"
    )
}

// ---------------------------------------------------------------------------
// EXECUTED int list ops — value-matched against CPython.
// ---------------------------------------------------------------------------

#[test]
fn int_list_ops_execute_and_match_cpython() {
    // (tag, list setup, CPython fingerprint of the identical program).
    // CPython refs computed by `acc=0; for v in <result>: acc=acc*31+v`.
    let cases: &[(&str, &str, i64)] = &[
        // sorted with duplicates AND negatives — exercises full comparator +
        // stable ties; the negative final fold value round-trips through the
        // unsigned i64 print. CPython: sorted([3,-1,3,-1,0,7,-9]) fold.
        (
            "sorted_dups_negs",
            "    xs: list[int] = [3,-1,3,-1,0,7,-9]\n    ys: list[int] = sorted(xs)",
            -8017082818,
        ),
        // reversed of 6 elements. CPython: reversed([1..6]) fold.
        (
            "reversed_six",
            "    xs: list[int] = [1,2,3,4,5,6]\n    ys: list[int] = list(reversed(xs))",
            176514621,
        ),
        // concat with an EMPTY rhs — the empty-operand allocator edge.
        (
            "concat_empty_rhs",
            "    a: list[int] = [1,2,3]\n    b: list[int] = []\n    ys: list[int] = a + b",
            1026,
        ),
        // concat with an EMPTY lhs.
        (
            "concat_empty_lhs",
            "    a: list[int] = []\n    b: list[int] = [4,5]\n    ys: list[int] = a + b",
            129,
        ),
        // slice with BOTH bounds negative — `+= n` normalisation on lo AND hi.
        (
            "slice_neg_both",
            "    xs: list[int] = [1,2,3,4,5,6]\n    ys: list[int] = xs[-4:-1]",
            3012,
        ),
        // slice with an OUT-OF-RANGE hi — clamps to n, never traps.
        (
            "slice_oor_hi",
            "    xs: list[int] = [10,20,30]\n    ys: list[int] = xs[0:100]",
            10260,
        ),
        // EMPTY slice (`hi < lo`) — count 0, never a negative length; fold==0.
        (
            "slice_empty",
            "    xs: list[int] = [10,20,30]\n    ys: list[int] = xs[3:1]",
            0,
        ),
        // full `xs[:]` copy — both bounds omitted (0 / i64::MAX defaults).
        (
            "slice_full",
            "    xs: list[int] = [7,8,9]\n    ys: list[int] = xs[:]",
            6984,
        ),
    ];

    // The pipeline must ALWAYS lower + emit (proves frontend reachability),
    // even on a host without WABT.
    for (tag, setup, _) in cases {
        let src = int_probe(setup);
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit int probe {tag}"
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1257: WABT absent — emit-only int check passed, execution skipped");
        return;
    }

    for (tag, setup, expect) in cases {
        let src = int_probe(setup);
        let got = run_i64(&src, tag);
        assert_eq!(
            got, *expect,
            "int list op {tag}: wasm={got} cpython={expect}"
        );
    }
}

// ---------------------------------------------------------------------------
// EXECUTED float list ops — f64 bit patterns survive the word-move.
// ---------------------------------------------------------------------------

#[test]
fn float_list_ops_execute_and_match_cpython() {
    let cases: &[(&str, &str, f64)] = &[
        // sorted floats with a negative + duplicate + zero.
        (
            "f_sorted",
            "    xs: list[float] = [3.5,-1.25,3.5,0.0,7.0,-9.5]\n    ys: list[float] = sorted(xs)",
            -162464.75,
        ),
        // `xs[::-1]` — the reverse-slice idiom (lowers via Reversed, NOT Slice).
        (
            "f_slice_revstep",
            "    xs: list[float] = [1.0,2.5,3.5,4.0]\n    ys: list[float] = xs[::-1]",
            1562.0,
        ),
        // float concat.
        (
            "f_concat",
            "    a: list[float] = [1.5,2.5]\n    b: list[float] = [-3.25,4.0,0.5]\n    ys: list[float] = a + b",
            4328.25,
        ),
    ];

    for (tag, setup, _) in cases {
        let src = float_probe(setup);
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit float probe {tag}"
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1257: WABT absent — emit-only float check passed, execution skipped");
        return;
    }

    for (tag, setup, expect) in cases {
        let src = float_probe(setup);
        let got = run_f64(&src, tag);
        assert!(
            (got - *expect).abs() < 1e-9,
            "float list op {tag}: wasm={got} cpython={expect}"
        );
    }
}

// ---------------------------------------------------------------------------
// PMAT-1259 — EXECUTED witness for GENERALIZED list-concat operands. A concat
// operand is now any list-VALUED expr (chained `a + b + c`, a list LITERAL,
// and the other allocating list ops `sorted`/`reversed`/slice), not only a
// bare name. The `chain_mixed` case exercises `sorted(a) + b[1:] + c` — proof
// the sorted AND slice gate-walkers recurse into concat operands (else their
// helper is undeclared → a hard wat2wasm failure), and that the operand-stack
// discipline survives operands that themselves bump-allocate.
// ---------------------------------------------------------------------------

#[test]
fn generalized_concat_operands_execute_and_match_cpython() {
    // (tag, setup defining `ys`, CPython fingerprint `acc=0; acc=acc*31+v`).
    let cases: &[(&str, &str, i64)] = &[
        // chained `a + b + c` = `(a + b) + c` — a nested ListConcat operand.
        (
            "chain_abc",
            "    a: list[int] = [1,2]\n    b: list[int] = [3,4]\n    c: list[int] = [5,6]\n    ys: list[int] = a + b + c",
            30569571,
        ),
        // a bare name + a list LITERAL operand (the literal bump-allocates).
        (
            "name_plus_lit",
            "    a: list[int] = [7,8,9]\n    ys: list[int] = a + [3,1,2]",
            208063260,
        ),
        // `sorted(a) + b` — a same-kind sort nested in a concat operand.
        (
            "sorted_plus_name",
            "    a: list[int] = [5,3,9,1]\n    b: list[int] = [100]\n    ys: list[int] = sorted(a) + b",
            1018078,
        ),
        // `list(reversed(a)) + b` — a reverse nested in a concat operand.
        (
            "reversed_plus_name",
            "    a: list[int] = [1,2,3]\n    b: list[int] = [9]\n    ys: list[int] = list(reversed(a)) + b",
            91335,
        ),
        // `a[1:4] + b` — a slice nested in a concat operand.
        (
            "slice_plus_name",
            "    a: list[int] = [10,20,30,40,50]\n    b: list[int] = [99]\n    ys: list[int] = a[1:4] + b",
            625989,
        ),
        // `sorted(a) + b[1:] + c` — sorted + slice both nested in a chain.
        (
            "chain_mixed",
            "    a: list[int] = [3,1,2]\n    b: list[int] = [10,20,30]\n    c: list[int] = [7]\n    ys: list[int] = sorted(a) + b[1:] + c",
            30585723,
        ),
        // `a + a + a` — the SAME record aliased three times in a chain (no
        // operand-stack aliasing hazard: each operand's pointer is stacked
        // before the next, and reads never mutate).
        (
            "self_chain",
            "    a: list[int] = [4,5]\n    ys: list[int] = a + a + a",
            119258307,
        ),
    ];

    for (tag, setup, _) in cases {
        let src = int_probe(setup);
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit generalized-concat probe {tag}"
        );
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1259: WABT absent — emit-only concat-operand check passed, execution skipped"
        );
        return;
    }

    for (tag, setup, expect) in cases {
        let src = int_probe(setup);
        let got = run_i64(&src, tag);
        assert_eq!(
            got, *expect,
            "generalized concat {tag}: wasm={got} cpython={expect}"
        );
    }
}

#[test]
fn generalized_concat_float_operands_execute_and_match_cpython() {
    let cases: &[(&str, &str, f64)] = &[
        // float chained concat.
        (
            "f_chain",
            "    a: list[float] = [1.5,2.5]\n    b: list[float] = [3.5]\n    c: list[float] = [4.5,5.5]\n    ys: list[float] = a + b + c",
            4667.5,
        ),
        // `sorted(a) + b` over floats.
        (
            "f_sorted_plus",
            "    a: list[float] = [3.5,1.5,2.5]\n    b: list[float] = [9.5]\n    ys: list[float] = sorted(a) + b",
            671.0,
        ),
    ];

    for (tag, setup, _) in cases {
        let src = float_probe(setup);
        assert!(
            emit(&src).is_ok(),
            "pipeline failed to lower+emit float generalized-concat probe {tag}"
        );
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1259: WABT absent — float concat-operand emit-only check passed, execution skipped");
        return;
    }

    for (tag, setup, expect) in cases {
        let src = float_probe(setup);
        let got = run_f64(&src, tag);
        assert!(
            (got - *expect).abs() < 1e-9,
            "float generalized concat {tag}: wasm={got} cpython={expect}"
        );
    }
}

// ---------------------------------------------------------------------------
// EXECUTED set ALGEBRA — union/intersection/difference/symmetric-difference.
// ---------------------------------------------------------------------------

#[test]
fn set_algebra_executes_and_matches_cpython() {
    // len(a|b)*1000 + len(a&b)*100 + len(a-b)*10 + len(a^b) over
    // a={1,2,3,4}, b={3,4,5,6}: union=6, inter=2, diff=2, symdiff=4 → 6224.
    let src = "def go() -> int:\n    \
        a: set[int] = {1, 2, 3, 4}\n    \
        b: set[int] = {3, 4, 5, 6}\n    \
        u: set[int] = a | b\n    \
        i: set[int] = a & b\n    \
        d: set[int] = a - b\n    \
        x: set[int] = a ^ b\n    \
        return len(u) * 1000 + len(i) * 100 + len(d) * 10 + len(x)\n";

    assert!(
        emit(src).is_ok(),
        "pipeline failed to lower+emit set algebra"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-1257: WABT absent — set-algebra emit-only check passed, execution skipped");
        return;
    }

    let got = run_i64(src, "set_algebra");
    assert_eq!(got, 6224, "set algebra: wasm={got} cpython=6224");
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — the pipeline must REFUSE (hard Err), never silently
// miscompile. A composed op over a temporary (not a named list) and the
// unsupported slice shapes are the boundary.
// ---------------------------------------------------------------------------

#[test]
fn unsupported_shapes_refuse_not_miscompile() {
    // Each of these has a well-defined CPython value, but the WASM lane
    // deliberately requires a NAMED list operand / rejects the shape. The
    // contract is a hard refusal, NOT a wrong answer.
    let refused: &[(&str, &str)] = &[
        // sorted over a concat TEMPORARY (a+b is not a name).
        (
            "sorted_of_concat",
            "def go() -> int:\n    a: list[int] = [3,1]\n    b: list[int] = [2,5,4]\n    c: list[int] = sorted(a + b)\n    return c[0]\n",
        ),
        // reversed over a slice TEMPORARY.
        (
            "reversed_of_slice",
            "def go() -> int:\n    xs: list[int] = [10,20,30,40,50]\n    ys: list[int] = list(reversed(xs[1:4]))\n    return ys[0]\n",
        ),
        // indexing a slice TEMPORARY directly.
        (
            "index_of_slice_temp",
            "def go() -> int:\n    xs: list[int] = [10,20,30,40,50]\n    return xs[1:4][0]\n",
        ),
        // list[bool] slice — 4-byte i32 stride ≠ the 8-byte-word helper.
        (
            "bool_list_slice",
            "def go() -> int:\n    xs: list[bool] = [True, False, True, True]\n    ys: list[bool] = xs[1:3]\n    return 1\n",
        ),
        // STEPPED slice (step 2) — not the verbatim word-range move.
        (
            "stepped_slice",
            "def go() -> int:\n    xs: list[int] = [1,2,3,4,5,6]\n    ys: list[int] = xs[0:6:2]\n    return ys[0]\n",
        ),
    ];

    for (tag, src) in refused {
        assert!(
            emit(src).is_err(),
            "unsupported shape {tag} was NOT refused — the pipeline emitted (possible silent miscompile)"
        );
    }
}
