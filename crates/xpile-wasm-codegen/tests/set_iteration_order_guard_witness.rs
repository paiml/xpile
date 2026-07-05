//! PMAT-1292 — ADVERSARIAL-VERIFY regression witness for the set-iteration
//! ORDER-DEPENDENCE guard. Runs on the bump-heap set runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! PMAT-1290 shipped native-WASM `for x in s` over a set on the claim that
//! "every legitimate observation is a COMMUTATIVE fold (sum / max / count /
//! membership), for which storage order is irrelevant", and the codegen comment
//! asserted an order-DEPENDENT set observation "is refused upstream". A skeptic
//! differential pass REFUTED that: the desugar emitted a storage-order `while`
//! loop for ANY body, so an order-DEPENDENT scalar fold silently diverged —
//! e.g. `for x in {7,3,5,1,9}: r = r*10 + x` returned `73519` (bump-heap storage
//! order) where CPython returns `13579` (hash-table order). The "first element"
//! flag idiom diverged the same way. Nothing was refused.
//!
//! A set has NO defined iteration order (CPython walks its hash table; the
//! bump-heap walks storage order, and `discard` swaps the last entry into the
//! hole), so xpile CANNOT match CPython's order without replicating the exact
//! hash table. PMAT-1292 makes the claim TRUE by construction: a set-iteration
//! body is emitted ONLY when it is a provably order-INDEPENDENT reduction
//! (`set_iteration_body_order_safe`); an order-DEPENDENT body refuses with a
//! precise `sorted(s)`-pointing message rather than emitting a wrong result.
//!
//! Pins, against live `python3`:
//!   * the order-DEPENDENT positional fold + "first element" flag + an
//!     accumulator-observing guard all REFUSE (no WABT needed);
//!   * the commutative folds PMAT-1290 shipped (sum / count / conditional-count /
//!     `if x>m: m=x` max) STILL emit + execute + match CPython;
//!   * `sorted(s)` is the order-defining escape hatch: `xs = sorted(s)` then an
//!     order-dependent fold over `xs` executes and matches CPython.
//!
//! Executed probes gate on `wasm_runtime_available()` (a clean skip without
//! WABT); the refusal probes hold with or without WABT.

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

/// Assemble + run the real-emitted WAT's zero-arg `go` export in WABT and
/// return the printed value as a SIGNED i64 (wasm-interp prints i64 unsigned).
fn run_i64(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let dir = std::env::temp_dir().join(format!("xpile-setguard-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("go.wat");
    let wasm_path = dir.join("go.wasm");
    std::fs::write(&wat_path, &wat).expect("write wat");

    let asm = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        asm.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&asm.stderr)
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
        .unwrap_or_else(|| panic!("no `go` export for {tag}:\n{stdout}"));
    line.rsplit(':')
        .next()
        .unwrap()
        .trim()
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("parse i64 for {tag}")) as i64
}

/// Run the SAME source under live `python3` and return `go()` — the differential
/// ground truth. Skips (returns `None`) if `python3` is unavailable.
fn cpython(src: &str) -> Option<i64> {
    let prog = format!("{src}\nprint(go())\n");
    let out = Command::new("python3").arg("-c").arg(&prog).output().ok()?;
    if !out.status.success() {
        panic!("python3 failed:\n{}", String::from_utf8_lossy(&out.stderr));
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<i64>()
            .expect("python3 go() int"),
    )
}

/// Assert the WASM emit VALUE-MATCHES live CPython for an order-INDEPENDENT
/// set-iteration body.
fn assert_matches_cpython(src: &str, tag: &str) {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1292: skipping EXECUTED probe {tag} — WABT absent");
        return;
    }
    let wasm = run_i64(src, tag);
    if let Some(cpy) = cpython(src) {
        assert_eq!(wasm, cpy, "{tag}: wasm={wasm} != cpython={cpy}");
    }
}

// ------------------------------------------------------------------------
// The REFUSALS — the miscompile class PMAT-1292 closes. Hold without WABT.
// ------------------------------------------------------------------------

/// `r = r*10 + x` — the positional/base-building fold that observes the set's
/// order. THE refuted PMAT-1290 miscompile (`73519` vs CPython `13579`).
#[test]
fn order_dependent_positional_fold_refuses() {
    let src = "\
def go() -> int:
    s = {7, 3, 5, 1, 9}
    r = 0
    for x in s:
        r = r * 10 + x
    return r
";
    let err = emit(src).expect_err("order-dependent positional fold must refuse");
    assert!(
        err.contains("order-dependent") && err.contains("commutative"),
        "refusal must name the order-dependence + point at commutative folds: {err}"
    );
    assert!(
        err.contains("sorted(s)"),
        "refusal must point at the `sorted(s)` escape hatch: {err}"
    );
}

/// The "first element" flag — order-dependent (it captures whichever element the
/// iteration visits first). Matched CPython only by luck before the guard.
#[test]
fn first_element_flag_refuses() {
    let src = "\
def go() -> int:
    s = {50, 10, 30, 20, 40}
    r = 0
    first = 1
    for x in s:
        if first == 1:
            r = x
            first = 0
    return r
";
    let err = emit(src).expect_err("first-element capture must refuse");
    assert!(
        err.contains("order-dependent"),
        "first-element flag must refuse as order-dependent: {err}"
    );
}

/// A guard that OBSERVES an accumulator (`if total < 100: total += x`) makes even
/// a commutative-looking accumulation order-dependent — which element pushes
/// `total` past the threshold depends on order. Must refuse.
#[test]
fn accumulator_observing_guard_refuses() {
    let src = "\
def go() -> int:
    s = {30, 40, 50, 60}
    total = 0
    for x in s:
        if total < 100:
            total = total + x
    return total
";
    let err = emit(src).expect_err("accumulator-observing guard must refuse");
    assert!(
        err.contains("order-dependent"),
        "guard reading an accumulator must refuse: {err}"
    );
}

/// Subtraction is NOT in the commutative-monoid whitelist even though `acc-x1-x2`
/// happens to be order-invariant — the conservative under-approximation refuses
/// it honestly rather than special-casing. (Documents the intended posture.)
#[test]
fn non_whitelisted_op_refuses_conservatively() {
    let src = "\
def go() -> int:
    s = {1, 2, 3}
    r = 100
    for x in s:
        r = r - x
    return r
";
    let err = emit(src).expect_err("subtraction accumulation is refused conservatively");
    assert!(
        err.contains("order-dependent"),
        "conservative refusal: {err}"
    );
}

// ------------------------------------------------------------------------
// The COMMUTATIVE folds PMAT-1290 shipped — STILL emit + execute + match.
// ------------------------------------------------------------------------

#[test]
fn commutative_sum_still_matches_cpython() {
    // sum {5,3,9,1,7} = 25, order-invariant.
    assert_matches_cpython(
        "\
def go() -> int:
    s = {5, 3, 9, 1, 7}
    total = 0
    for x in s:
        total = total + x
    return total
",
        "sum",
    );
}

#[test]
fn commutative_count_still_matches_cpython() {
    // distinct {1,5,9,13} -> 4.
    assert_matches_cpython(
        "\
def go() -> int:
    s = {1, 5, 9, 13, 5, 1}
    n = 0
    for x in s:
        n = n + 1
    return n
",
        "count",
    );
}

#[test]
fn extremum_idiom_max_still_matches_cpython() {
    // `if x > m: m = x` — the max fold, order-invariant.
    assert_matches_cpython(
        "\
def go() -> int:
    s = {3, 99, 17, 42}
    m = 0
    for x in s:
        if x > m:
            m = x
    return m
",
        "max",
    );
}

#[test]
fn conditional_count_still_matches_cpython() {
    // element-only guard + commutative accumulate: count of elements > 10.
    assert_matches_cpython(
        "\
def go() -> int:
    s = {5, 20, 8, 30, 15}
    n = 0
    for x in s:
        if x > 10:
            n = n + 1
    return n
",
        "cond_count",
    );
}

#[test]
fn xor_fold_still_matches_cpython() {
    // `acc = acc ^ x` — a commutative-monoid op beyond the witness set.
    assert_matches_cpython(
        "\
def go() -> int:
    s = {1, 2, 4, 8, 16}
    acc = 0
    for x in s:
        acc = acc ^ x
    return acc
",
        "xor",
    );
}

// ------------------------------------------------------------------------
// `sorted(s)` — the ORDER-DEFINING escape hatch. An order-dependent fold over
// the SORTED materialisation is allowed and CPython-exact.
// ------------------------------------------------------------------------

#[test]
fn sorted_binding_allows_order_dependent_fold() {
    // `xs = sorted(s)` then a positional fold: order is DEFINED, so it matches.
    assert_matches_cpython(
        "\
def go() -> int:
    s = {7, 3, 5, 1, 9}
    xs = sorted(s)
    r = 0
    for x in xs:
        r = r * 10 + x
    return r
",
        "sorted_escape",
    );
}
