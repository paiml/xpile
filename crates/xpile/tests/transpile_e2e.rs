//! End-to-end Python → Rust transpilation tests.
//!
//! Drives the `xpile transpile` binary as a subprocess against
//! fixture .py files and verifies the emitted Rust:
//!   * is shaped as expected (string matchers), AND
//!   * actually type-checks via rustc as `--edition 2021`.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    // Set by Cargo when running integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run_xpile(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("spawn xpile")
}

#[test]
fn transpile_add_py_emits_rust_fn() {
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "xpile failed: stderr={stderr} stdout={stdout}"
    );
    assert!(
        stdout.contains("pub fn add(a: i64, b: i64) -> i64"),
        "missing fn signature in:\n{stdout}"
    );
    // Post PMAT-002: addition lowers to checked_add + .expect(...).
    assert!(
        stdout.contains("checked_add"),
        "expected checked_add in body:\n{stdout}"
    );
    assert!(
        stdout.contains("C-PY-INT-ARITH"),
        "expected contract reference:\n{stdout}"
    );
}

#[test]
fn transpile_cmp_py_emits_bool_return() {
    let py = fixture("cmp.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn le(a: i64, b: i64) -> bool"));
    assert!(stdout.contains("(a <= b)"));
}

#[test]
fn transpile_pick_py_emits_ternary_if_expr() {
    let py = fixture("pick.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn pick(a: i64, b: i64) -> i64"));
    assert!(
        stdout.contains("if (a <= b) { a } else { b }"),
        "expected ternary as if-expr in:\n{stdout}"
    );
}

#[test]
fn transpile_pick_py_to_ruchy_emits_fun_with_if() {
    let py = fixture("pick.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fun pick(a: i64, b: i64) -> i64"));
    assert!(stdout.contains("if (a <= b) { a } else { b }"));
}

#[test]
fn transpile_add_py_to_ruchy_target() {
    // Same Python source through a different backend — proves the
    // dispatch architecture: one Frontend, multiple Backends.
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("fun add(a: i64, b: i64) -> i64"),
        "expected Ruchy `fun` signature in:\n{stdout}"
    );
    assert!(
        !stdout.contains("pub fn"),
        "Ruchy target must not emit Rust `pub fn`"
    );
    // Post PMAT-002: Ruchy compiles to Rust → shares checked semantics.
    assert!(
        stdout.contains("checked_add"),
        "expected checked_add in Ruchy emission:\n{stdout}"
    );
}

#[test]
fn unknown_extension_errors() {
    let unknown = fixture("../fixtures/add.unknownext");
    let out = run_xpile(&["transpile", unknown.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "should fail on unknown extension");
    assert!(
        stderr.contains("no frontend") || stderr.contains("reading"),
        "expected frontend-not-found / read error, got: {stderr}"
    );
}

#[test]
fn info_lists_default_session_dispatch_tables() {
    let out = run_xpile(&["info"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(stdout.contains("Code lane"));
    assert!(stdout.contains("python"));
    assert!(stdout.contains("rust"));
    assert!(stdout.contains("Proof lane"));
    assert!(stdout.contains("latex"));
}

// ─── rustc round-trip ─────────────────────────────────────────────
//
// The shape-matching tests above prove that the emitted Rust *looks*
// right. These tests prove it *type-checks* — by piping the output
// through `rustc --crate-type=lib --emit=metadata`. If the emitter
// regresses to a syntactically-plausible-but-ill-typed form (e.g.,
// wrong return type after some refactor), these tests catch it.
//
// Skipped if `rustc` is not on PATH (treated as test infrastructure
// missing rather than a feature failure).

fn rust_target_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("xpile-e2e-rustc").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Stronger than `assert_rustc_accepts`: actually compiles + runs the
/// emitted Rust, with the supplied `driver_main` appended (which calls
/// the transpiled functions and `assert!`s the expected results).
/// Failure = the binary exiting non-zero (assertions tripped) or rustc
/// rejecting the merged source.
fn assert_rustc_runs(name: &str, transpiled: &str, driver_main: &str) {
    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("warning: rustc not on PATH; skipping runtime check for {name}");
        return;
    }
    let dir = rust_target_dir(name);
    let file = dir.join(format!("{name}.rs"));
    let merged = format!("{transpiled}\n\n{driver_main}\n");
    std::fs::write(&file, &merged).expect("write merged rust");
    let bin = dir.join(name);
    let compile = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(&bin)
        .arg(&file)
        .output()
        .expect("spawn rustc");
    assert!(
        compile.status.success(),
        "rustc failed to build {name}:\n=== source ===\n{merged}\n=== stderr ===\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&bin).output().expect("spawn binary");
    assert!(
        run.status.success(),
        "binary {name} exited non-zero (assertion tripped?):\n=== source ===\n{merged}\n=== stderr ===\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

fn assert_rustc_accepts(name: &str, source: &str) {
    if Command::new("rustc").arg("--version").output().is_err() {
        eprintln!("warning: rustc not on PATH; skipping round-trip check for {name}");
        return;
    }
    let dir = rust_target_dir(name);
    let file = dir.join(format!("{name}.rs"));
    std::fs::write(&file, source).expect("write rust source");
    let out = Command::new("rustc")
        .arg("--edition=2021")
        .arg("--crate-type=lib")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&dir)
        .arg(&file)
        .output()
        .expect("spawn rustc");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "rustc rejected emitted Rust for {name}:\n=== source ===\n{source}\n=== rustc stderr ===\n{stderr}"
    );
}

fn xpile_transpile_to_rust(fixture_name: &str) -> String {
    let py = fixture(fixture_name);
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "xpile failed on {fixture_name}: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout is UTF-8")
}

/// PMAT-479 (R10): C early returns (guard clauses) — a non-final
/// `return` inside an if-branch lowers to `Stmt::Return`; the function
/// still ends with a trailing return. Recursive `fact` via a guard
/// clause and a 3-way `sign` must compute correctly.
#[test]
fn c_early_return_guard_clauses_roundtrip() {
    let rust = xpile_transpile_to_rust("c_early_return.c");
    assert!(
        rust.contains("return 1i32;"),
        "early return inside a branch should emit `return e;`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(fact(0), 1);
    assert_eq!(fact(5), 120);
    assert_eq!(sign(7), 1);
    assert_eq!(sign(-3), -1);
    assert_eq!(sign(0), 0);
}
"#;
    assert_rustc_runs("c_early_return", &rust, driver);
}

/// PMAT-478 (R9): C if/else statements (`Stmt::If`) — the decy
/// frontend's first statement-level branching (beyond the ternary).
/// Locals reassigned in a branch are inferred `mut`.
#[test]
fn c_if_else_statements_roundtrip() {
    let rust = xpile_transpile_to_rust("c_if.c");
    assert!(
        rust.contains("if b > m {") && rust.contains("} else {"),
        "C if/else should emit Rust if/else statements:\n{rust}"
    );
    assert!(
        rust.contains("let mut m: i32") && rust.contains("let mut r: i32"),
        "locals reassigned in a branch should be `mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(max3(1, 5, 3), 5);
    assert_eq!(max3(9, 2, 4), 9);
    assert_eq!(clamp(15, 0, 10), 10);
    assert_eq!(clamp(-3, 0, 10), 0);
    assert_eq!(clamp(7, 0, 10), 7);
}
"#;
    assert_rustc_runs("c_if", &rust, driver);
}

/// PMAT-477 (R8): Python `float` (f64). Float arithmetic is plain infix
/// (IEEE-754, no `checked_*`); `/` is true division (not floor).
#[test]
fn float_arithmetic_roundtrip() {
    let rust = xpile_transpile_to_rust("float_arith.py");
    assert!(
        rust.contains("-> f64") && rust.contains(": f64"),
        "float params/returns should be f64:\n{rust}"
    );
    // PMAT-581: float `/` guards the zero divisor — the `2f64` divisor now
    // binds to `__fz` (still no `checked_*`; that's the i64 path).
    assert!(
        !rust.contains("checked_") && rust.contains("let __fz: f64 = 2f64"),
        "float arithmetic should be plain infix (no checked_*); `/` is true division:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!((lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-9);
    assert!((lerp(2.0, 4.0, 0.25) - 2.5).abs() < 1e-9);
    assert!((average(3.0, 4.0) - 3.5).abs() < 1e-9);
    assert!((scale(2.5, 4.0) - 10.0).abs() < 1e-9);
}
"#;
    assert_rustc_runs("float_arith", &rust, driver);
}

/// PMAT-474 (R5): keyword arguments `f(x=1, y=2)` reorder to positional
/// at lowering using the callee's declared parameter order.
#[test]
fn keyword_arguments_roundtrip() {
    let rust = xpile_transpile_to_rust("kwargs.py");
    assert!(
        rust.contains("area(1i64, 2i64, 3i64, 4i64)"),
        "`area(1, 2, h=4, w=3)` should reorder to positional w,h order:\n{rust}"
    );
    assert!(
        rust.contains("area(10i64, 20i64, 30i64, 40i64)"),
        "all-keyword call should reorder to declared param order:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(mixed(), 10i64);    // 1+2+3+4
    assert_eq!(all_kw(), 100i64);  // 10+20+30+40
}
"#;
    assert_rustc_runs("kwargs", &rust, driver);
}

/// PMAT-473 (R4): list comprehensions `[elem for var in iter]`
/// materialise to `tmp = []` + a for-append loop, in both return
/// position (hoisted to a temp) and assignment position.
#[test]
fn list_comprehension_roundtrip() {
    let rust = xpile_transpile_to_rust("list_comp.py");
    assert!(
        rust.contains("let mut __xpile_comp: Vec<i64> = vec![];")
            && rust.contains("__xpile_comp.push("),
        "return-position comprehension should hoist to a temp + push loop:\n{rust}"
    );
    assert!(
        rust.contains("let mut ys: Vec<i64> = vec![];") && rust.contains("ys.push("),
        "assignment-position comprehension should build the named accumulator:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(squares(vec![1i64, 2, 3, 4]), vec![1i64, 4, 9, 16]);
    assert_eq!(doubled(vec![5i64, 10]), vec![10i64, 20]);
    assert_eq!(total_sq(vec![1i64, 2, 3]), 14i64);
}
"#;
    assert_rustc_runs("list_comp", &rust, driver);
}

/// PMAT-472 (R3): dict iteration `for k in d:` lowers to
/// `for k in d.keys().cloned()` (the loop var is the key type).
/// Assertions are order-independent — HashMap key order is unspecified.
#[test]
fn dict_iteration_roundtrip() {
    let rust = xpile_transpile_to_rust("dict_iter.py");
    assert!(
        rust.contains("for k in d.keys().cloned()"),
        "`for k in d:` should iterate keys:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(1i64, 10i64);
    d.insert(2i64, 20i64);
    d.insert(3i64, 30i64);
    assert_eq!(sum_keys(d.clone()), 6i64);     // 1+2+3
    assert_eq!(sum_values(d.clone()), 60i64);  // 10+20+30
}
"#;
    assert_rustc_runs("dict_iter", &rust, driver);
}

/// PMAT-471 (R2): cross-function return-type inference. A local bound to
/// a call (`s = make_scores()`) must take the callee's declared return
/// type from the module signature table, not the old hardcoded `i64`
/// fallback (which emitted `let s: i64` and made `s["alice"]` reject).
#[test]
fn cross_function_return_type_inference_roundtrip() {
    let rust = xpile_transpile_to_rust("cross_fn_dict.py");
    assert!(
        rust.contains("let s: std::collections::HashMap<String, i64> = make_scores()"),
        "`s = make_scores()` should type s as the callee's dict return, not i64:\n{rust}"
    );
    assert!(
        !rust.contains("let s: i64 = make_scores()"),
        "the old i64 call-result fallback must not apply to a dict-returning call:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(alice_score(), 10i64);
    assert_eq!(total(), 30i64);
}
"#;
    assert_rustc_runs("cross_fn_dict", &rust, driver);
}

/// PMAT-470 (R1): augmented assignment (`x += i`, `p *= x`, `out += "!"`).
/// Desugars to `x = x <op> e` reusing the BinOp machinery (so overflow
/// checking and str-concat detection apply); must compute correct values.
#[test]
fn augmented_assignment_roundtrip() {
    let rust = xpile_transpile_to_rust("aug_assign.py");
    assert!(
        rust.contains("total = (total).checked_add(i)"),
        "`total += i` should desugar to a checked add:\n{rust}"
    );
    assert!(
        rust.contains("out = format!(\"{}{}\", out,"),
        "`out += \"!\"` should desugar to str concat, not checked_add:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(count_up(5), 10);      // 0+1+2+3+4
    assert_eq!(count_up(100), 4950);
    assert_eq!(product(vec![1i64, 2, 3, 4]), 24i64);
    assert_eq!(shout(String::from("hi")), "hi!!");
}
"#;
    assert_rustc_runs("aug_assign", &rust, driver);
}

/// PMAT-467 (v0.2.0 Track 2.A): the decy C → Rust frontend, xpile's
/// second source language. A stack-only int C module (`add`, recursive
/// `factorial` via ternary, `poly` with local decls) must transpile to
/// Rust with **C arithmetic semantics** — `i32` (not Python's i64) and
/// `wrapping_*` (not Python's `checked_*`) — and compute correct values.
#[test]
fn c_int_arith_transpiles_to_rust_and_runs() {
    let rust = xpile_transpile_to_rust("c_int_arith.c");
    assert!(
        rust.contains("-> i32") && rust.contains(": i32"),
        "C int must lower to i32, not i64:\n{rust}"
    );
    assert!(
        rust.contains("wrapping_add") && rust.contains("wrapping_mul"),
        "C arithmetic must use wrapping_*, not checked_*:\n{rust}"
    );
    assert!(
        !rust.contains("checked_") && !rust.contains("i64"),
        "C emission must not use Python's checked_*/i64 path:\n{rust}"
    );
    assert!(
        rust.contains("C-C-INT-ARITH"),
        "C functions should cite C-C-INT-ARITH:\n{rust}"
    );
    assert!(
        rust.contains("while ") && rust.contains("let mut "),
        "iterative C should emit a while loop with mutable locals:\n{rust}"
    );
    assert!(
        rust.contains("wrapping_div"),
        "C `/` should emit truncating wrapping_div:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(add(2, 3), 5);
    assert_eq!(add(-4, 10), 6);
    assert_eq!(factorial(0), 1);
    assert_eq!(factorial(5), 120);
    assert_eq!(factorial(12), 479001600);
    assert_eq!(poly(3), 16);   // 9 + (6+1)
    // iterative (slice 2): while + reassignment
    assert_eq!(sum_to(5), 15);
    assert_eq!(sum_to(100), 5050);
    // C truncating division (toward zero), not Python floor
    assert_eq!(half(7), 3);
    assert_eq!(half(-7), -3);
}
"#;
    assert_rustc_runs("c_int_arith", &rust, driver);
}

#[test]
fn rust_emission_for_add_compiles_with_rustc() {
    let rust = xpile_transpile_to_rust("add.py");
    assert_rustc_accepts("add", &rust);
}

#[test]
fn rust_emission_for_cmp_compiles_with_rustc() {
    let rust = xpile_transpile_to_rust("cmp.py");
    assert_rustc_accepts("cmp", &rust);
}

#[test]
fn transpile_let_sum_py_emits_lets_and_trailing_return() {
    let py = fixture("let_sum.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn let_sum(a: i64, b: i64) -> i64"));
    // Post PMAT-002: arithmetic uses checked_*; the let bindings now
    // wrap their values, but the let-with-`i64` annotation shape stays.
    assert!(
        stdout.contains("let s: i64 = ") && stdout.contains("checked_add"),
        "expected `let s: i64 = ...checked_add...`:\n{stdout}"
    );
    assert!(
        stdout.contains("let t: i64 = ") && stdout.contains("checked_mul"),
        "expected `let t: i64 = ...checked_mul...`:\n{stdout}"
    );
    // Trailing return is just the ident — no `return` keyword in v0.1.0 emission.
    assert!(stdout.contains("\n    t\n"));
}

#[test]
fn rust_emission_for_let_sum_compiles_with_rustc() {
    let rust = xpile_transpile_to_rust("let_sum.py");
    assert_rustc_accepts("let_sum", &rust);
}

#[test]
fn transpile_call_chain_py_emits_two_fns_and_calls() {
    let py = fixture("call_chain.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn add(a: i64, b: i64) -> i64"));
    assert!(stdout.contains("pub fn quad_add(a: i64, b: i64, c: i64, d: i64) -> i64"));
    assert!(stdout.contains("add(add(a, b), add(c, d))"));
}

#[test]
fn rust_emission_for_call_chain_compiles_with_rustc() {
    let rust = xpile_transpile_to_rust("call_chain.py");
    assert_rustc_accepts("call_chain", &rust);
}

#[test]
fn transpile_in_range_py_uses_logical_and() {
    let py = fixture("in_range.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pub fn in_range(x: i64, lo: i64, hi: i64) -> bool"));
    // `in_range.py` uses explicit `lo <= x and x <= hi` — two SINGLE comparisons
    // joined by `and`, NOT a chained comparison — so it stays the plain
    // conjunction (PMAT-576's temp-binding only applies to chained `a < b < c`).
    assert!(stdout.contains("((lo <= x) && (x <= hi))"));
}

#[test]
fn rust_emission_for_in_range_compiles_with_rustc() {
    let rust = xpile_transpile_to_rust("in_range.py");
    assert_rustc_accepts("in_range", &rust);
}

#[test]
fn transpile_add_py_to_lean_target() {
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("def add (a : Int) (b : Int) : Int :="),
        "expected Lean def signature in:\n{stdout}"
    );
    assert!(stdout.contains("(a + b)"));
    assert!(!stdout.contains("pub fn"));
    assert!(!stdout.contains("fun "));
}

#[test]
fn transpile_typed_py_honors_explicit_annotations() {
    let py = fixture("typed.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Annotated `n: int` and `-> bool` should flow through unchanged.
    assert!(stdout.contains("pub fn is_even(n: i64) -> bool"));
    // PMAT-538: Python `%` lowers to a truncating `checked_rem` plus a floor
    // correction (sign-of-divisor), not `rem_euclid` (which diverges for a
    // negative divisor). The operand binds to `__fa`/`__fb` temps.
    assert!(stdout.contains("__fa.checked_rem(__fb)") && stdout.contains("__r + __fb"));
}

#[test]
fn rust_emission_for_typed_compiles_with_rustc() {
    let rust = xpile_transpile_to_rust("typed.py");
    assert_rustc_accepts("typed", &rust);
}

#[test]
fn transpile_factorial_py_emits_recursive_rust() {
    let py = fixture("factorial.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // PMAT-036: factorial.py annotated `-> BigInt`. PMAT-013's
    // implicit promotion lowers `n: int` to BigInt + all int literals
    // in the body to `xpile_bigint::BigInt::from(<n>i64)`, so the
    // emitted Rust uses BigInt end-to-end. This is also what closes
    // the DIFF-003 documented promotion gaps for this fixture.
    assert!(
        stdout.contains("pub fn factorial(n: xpile_bigint::BigInt) -> xpile_bigint::BigInt"),
        "expected BigInt signature, got:\n{stdout}"
    );
    assert!(
        stdout.contains("xpile_bigint::BigInt::from(1i64)"),
        "expected BigInt-lifted integer literals, got:\n{stdout}"
    );
    // Recursive call back into factorial — proves Expr::Call works
    // for self-reference, BigInt or otherwise.
    assert!(stdout.contains("factorial("));
}

/// Semantic round-trip: emit factorial → compile → run → assert results.
/// Post-PMAT-036 factorial.py is BigInt-mode; the driver uses BigInt
/// constructors. Same expected values (0!=1, 1!=1, …, 10!=3628800).
/// Uses an inline `xpile_bigint` shim so the driver compiles
/// standalone via rustc — mirrors the existing
/// `bigint_implicit_promotion_factorial_emits_bigint_mode` test.
#[test]
fn factorial_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("factorial.py");
    let shim = r#"
mod xpile_bigint {
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BigInt(pub i64);
    impl From<i64> for BigInt {
        fn from(v: i64) -> Self { BigInt(v) }
    }
    impl std::ops::Add for BigInt {
        type Output = BigInt;
        fn add(self, o: BigInt) -> BigInt { BigInt(self.0 + o.0) }
    }
    impl std::ops::Sub for BigInt {
        type Output = BigInt;
        fn sub(self, o: BigInt) -> BigInt { BigInt(self.0 - o.0) }
    }
    impl std::ops::Mul for BigInt {
        type Output = BigInt;
        fn mul(self, o: BigInt) -> BigInt { BigInt(self.0 * o.0) }
    }
}
"#;
    let driver = r#"
fn main() {
    use xpile_bigint::BigInt;
    assert_eq!(factorial(BigInt::from(0i64)), BigInt::from(1i64));
    assert_eq!(factorial(BigInt::from(1i64)), BigInt::from(1i64));
    assert_eq!(factorial(BigInt::from(2i64)), BigInt::from(2i64));
    assert_eq!(factorial(BigInt::from(3i64)), BigInt::from(6i64));
    assert_eq!(factorial(BigInt::from(5i64)), BigInt::from(120i64));
    assert_eq!(factorial(BigInt::from(6i64)), BigInt::from(720i64));
    assert_eq!(factorial(BigInt::from(10i64)), BigInt::from(3628800i64));
}
"#;
    assert_rustc_runs("factorial", &format!("{shim}\n{rust}"), driver);
}

/// PMAT-462 — v0.2.0 Track 1.C foundation: `dict[str, int]` literal.
/// First Track 1.C sub-PR. Rust emits a block expression returning
/// an owned `HashMap<String, i64>` populated via `.insert(...)` calls.
#[test]
fn dict_counts_emitted_rust_returns_hashmap() {
    let rust = xpile_transpile_to_rust("dict_counts.py");
    let driver = r#"
fn main() {
    let m = counts();
    assert_eq!(m.len(), 3);
    assert_eq!(m.get(&String::from("alice")), Some(&1i64));
    assert_eq!(m.get(&String::from("bob")), Some(&2i64));
    assert_eq!(m.get(&String::from("carol")), Some(&3i64));
    assert_eq!(m.get(&String::from("nobody")), None);
}
"#;
    assert_rustc_runs("dict_counts", &rust, driver);
}

/// PMAT-466 — v0.2.0 Track 1.C operations: the full dict read/write
/// API. `histogram` builds a frequency map from an empty annotated
/// literal using `d[k] = d.get(k, 0) + 1` inside a `for` loop;
/// `lookup` reads `d[k]`; `has_key` tests `k in d`; `size` is
/// `len(d)`. The emitted Rust must compile (HashMap insert / get /
/// index / contains_key / len) and compute the correct frequencies.
#[test]
fn histogram_dict_ops_roundtrip() {
    let rust = xpile_transpile_to_rust("histogram.py");
    // Emission shape: empty literal → bare HashMap::new() (no
    // `let mut m` block that would trip clippy unused_mut), insert
    // for the keyed assign, get/cloned/unwrap_or for get-with-default,
    // index+clone for the read, contains_key for membership.
    assert!(
        rust.contains("std::collections::HashMap::new()"),
        "empty dict literal should emit a bare HashMap::new():\n{rust}"
    );
    assert!(
        rust.contains("counts.insert("),
        "d[k] = v should emit HashMap::insert:\n{rust}"
    );
    assert!(
        rust.contains(".cloned().unwrap_or("),
        "d.get(k, default) should emit get/cloned/unwrap_or:\n{rust}"
    );
    assert!(
        rust.contains("(table).get(&(key)).cloned().unwrap_or_else(|| panic!(\"xpile: KeyError: key not found\"))"),
        "d[k] read should emit indexed clone:\n{rust}"
    );
    assert!(
        rust.contains("table.contains_key(&(key))"),
        "k in d should emit contains_key:\n{rust}"
    );
    let driver = r#"
fn main() {
    let h = histogram(vec![1i64, 1, 2, 3, 3, 3]);
    assert_eq!(h.len(), 3);
    assert_eq!(lookup(h.clone(), 1i64), 2i64);
    assert_eq!(lookup(h.clone(), 2i64), 1i64);
    assert_eq!(lookup(h.clone(), 3i64), 3i64);
    assert_eq!(has_key(h.clone(), 2i64), true);
    assert_eq!(has_key(h.clone(), 99i64), false);
    assert_eq!(size(h.clone()), 3i64);
    // Absent-key get-with-default path: a fresh histogram of [5]
    // exercises the unwrap_or(0) branch for first insertion.
    let single = histogram(vec![5i64]);
    assert_eq!(lookup(single, 5i64), 1i64);
}
"#;
    assert_rustc_runs("histogram", &rust, driver);
}

/// PMAT-466 regression (adversarial review #5/#6/#9): string-keyed
/// histogram. The canonical `counts[w] = counts.get(w, 0) + 1` over
/// non-Copy `String` keys must compile — the DictSet emission binds the
/// value to a temp BEFORE moving the key into `.insert`, avoiding the
/// borrow-of-moved-value (E0382) that a naive `insert(w, …w…)` causes.
#[test]
fn word_count_str_keys_roundtrip() {
    let rust = xpile_transpile_to_rust("word_count.py");
    assert!(
        rust.contains("let __xpile_dict_val ="),
        "DictSet must bind the value to a temp before moving the key:\n{rust}"
    );
    let driver = r#"
fn main() {
    let h = word_count(vec![
        String::from("a"), String::from("b"), String::from("a"), String::from("a"),
    ]);
    assert_eq!(h.get(&String::from("a")), Some(&3i64));
    assert_eq!(h.get(&String::from("b")), Some(&1i64));
    assert_eq!(h.len(), 2);
}
"#;
    assert_rustc_runs("word_count", &rust, driver);
}

/// PMAT-466 regression (adversarial review #2/#4/#7): dict reads in
/// call args, relational operands, ternary branches, and `len(...)`
/// args all lower to HashMap keyed access (`d[&(k)].clone()`), never a
/// list `usize` index. The `rewrite_dict_reads` post-pass repairs them
/// in every position, not just the few `lower_expr_in_ctx` recurses.
#[test]
fn dict_reads_in_nested_positions_roundtrip() {
    let rust = xpile_transpile_to_rust("dict_read_positions.py");
    assert!(
        !rust.contains("as usize"),
        "no dict read should lower to a `usize` list index:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(1i64, 5i64);
    assert_eq!(via_call(d.clone(), 1i64), 5i64);
    assert_eq!(is_positive(d.clone(), 1i64), true);
    assert_eq!(pick(d.clone(), 1i64, 3i64), 5i64);
    assert_eq!(pick(d.clone(), 9i64, -1i64), 0i64);
    let mut s = std::collections::HashMap::new();
    s.insert(2i64, String::from("hello"));
    assert_eq!(val_len(s, 2i64), 5i64);
    // Dict read inside an if/else branch (lookup-with-fallback).
    assert_eq!(lookup_or(d.clone(), 1i64), 5i64);
    assert_eq!(lookup_or(d.clone(), -1i64), 0i64);
}
"#;
    assert_rustc_runs("dict_read_positions", &rust, driver);
}

/// PMAT-466 regression (adversarial review #1): a read-only annotated
/// local inside a loop body must emit `let`, not `let mut` — otherwise
/// `cargo clippy -D warnings` (the pre-push gate) rejects `unused_mut`.
#[test]
fn annotated_loop_local_is_not_mut() {
    let rust = xpile_transpile_to_rust("loop_local_readonly.py");
    assert!(
        rust.contains("let tmp: i64 = x;"),
        "read-only annotated loop local should be an immutable binding:\n{rust}"
    );
    assert!(
        !rust.contains("let mut tmp"),
        "read-only annotated loop local must NOT be `mut` (clippy unused_mut):\n{rust}"
    );
    let driver = "fn main() { assert_eq!(f(vec![1i64, 2, 3]), 6i64); }";
    assert_rustc_runs("loop_local_readonly", &rust, driver);
}

/// PMAT-466 regression (2nd adversarial-review round): dict reads in
/// positions the first fix overlooked — a `range()` bound, a
/// `list.append()` argument, and a list indexed-assign target index —
/// plus the str-key increment-then-read-back move. All must compile and
/// compute correctly.
#[test]
fn dict_ops_edge_positions_roundtrip() {
    let rust = xpile_transpile_to_rust("dict_ops_edge.py");
    // Dict reads must lower to keyed access `d[&(k)]`, never a list
    // `usize` index `d[k as usize]`. (The fixture also has *legitimate*
    // list indexing — `xs[0]`, `xs[<dict-read> as usize]` — so we check
    // for the mis-dispatch pattern directly rather than the absence of
    // `as usize`.)
    assert!(
        rust.contains(
            "(d).get(&(k)).cloned().unwrap_or_else(|| panic!(\"xpile: KeyError: key not found\"))"
        ),
        "dict reads should lower to keyed access:\n{rust}"
    );
    assert!(
        !rust.contains("d[k as usize]"),
        "a dict read must not lower to a list usize index:\n{rust}"
    );
    assert!(
        rust.contains(".clone(), __xpile_dict_val)"),
        "DictSet must clone the key so it survives a later read:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(2i64, 4i64);
    assert_eq!(range_bound(d.clone(), 2i64), 6i64); // 0+1+2+3
    assert_eq!(append_val(vec![], d.clone(), 2i64), 1i64);
    let mut t = std::collections::HashMap::new();
    t.insert(5i64, 0i64);
    assert_eq!(index_target(vec![1i64, 2], t, 5i64, 9i64), 9i64);
    let mut s = std::collections::HashMap::new();
    s.insert(String::from("a"), 5i64);
    assert_eq!(readback(s, String::from("a")), 6i64);
}
"#;
    assert_rustc_runs("dict_ops_edge", &rust, driver);
}

/// PMAT-466 regression (adversarial review #10): the Lean backend must
/// REFUSE dict operations with a clear error, never silently emit Lean
/// (the `List (K × V)` model has no keyed lookup). The histogram
/// fixture refuses on `for` first, so this uses a pure dict-read
/// fixture to exercise the dict-op refusal directly.
#[test]
fn dict_reads_refused_by_lean() {
    let py = fixture("dict_read_positions.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(
        !out.status.success(),
        "Lean should refuse dict ops, not emit code"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("dict") || stderr.contains("Std.HashMap"),
        "Lean refusal should name the dict-op gap:\n{stderr}"
    );
}

/// PMAT-502fe (Tranche 2 — correctness): `tuple(<iterable>)` is REJECTED with a
/// clear diagnostic rather than silently emitting an undefined `tuple(...)` call.
/// Rust tuples are fixed-arity, so a variable-length `tuple(xs)` has no Rust
/// counterpart; converting the prior silent miscompile into a clean lowering
/// error upholds the central "transpile-success ⟹ valid Rust" guarantee. (The
/// `(a, b)` literal-tuple path, `Type::Tuple`, is unaffected.)
#[test]
fn tuple_call_is_rejected_not_miscompiled() {
    let py = fixture("tuple_call_rejected.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "tuple(<iterable>) must be refused, not emitted as an undefined call"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("tuple(...)") && stderr.contains("fixed-arity"),
        "the rejection should name the fixed-arity reason:\n{stderr}"
    );
}

/// PMAT-466 regression (adversarial review #11): the Ruchy backend
/// emits the same HashMap pipeline as Rust (Ruchy compiles to Rust),
/// including the temp-let DictSet form.
#[test]
fn dict_ops_ruchy_target_emits_hashmap() {
    let py = fixture("word_count.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(
        out.status.success(),
        "ruchy transpile failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("fun word_count"),
        "ruchy should emit a `fun`:\n{stdout}"
    );
    assert!(
        stdout.contains(".insert(") && stdout.contains("let __xpile_dict_val ="),
        "ruchy dict write should emit the temp-let insert form:\n{stdout}"
    );
}

/// PMAT-462 — v0.2.0 Track 1.B: nested `list[list[int]]`. Verifies
/// the recursive Type::List + Expr::ListLit composition produces a
/// well-typed Rust `Vec<Vec<i64>>` and (transitively) a properly
/// parenthesised Lean `List (List Int)`.
#[test]
fn nested_list_emitted_rust_returns_2d_vec() {
    let rust = xpile_transpile_to_rust("nested_list.py");
    let driver = r#"
fn main() {
    let g = grid();
    assert_eq!(g.len(), 3);
    assert_eq!(g[0], vec![1i64, 2, 3]);
    assert_eq!(g[1], vec![4i64, 5, 6]);
    assert_eq!(g[2], vec![7i64, 8, 9]);
}
"#;
    assert_rustc_runs("nested_list", &rust, driver);
}

/// PMAT-461 — v0.2.0 Track 1.B: indexed assignment `xs[i] = v`.
/// Pairs with PMAT-457's Expr::Index read; together they give the
/// full read/write list-element API. Param mutation flows through
/// the same `mutable` flag as `.append()`.
#[test]
fn set_first_emitted_rust_overwrites_element() {
    let rust = xpile_transpile_to_rust("set_first.py");
    let driver = r#"
fn main() {
    assert_eq!(set_first(vec![10i64, 20, 30], 99i64), 99i64);
    assert_eq!(set_first(vec![1i64], -5i64), -5i64);
}
"#;
    assert_rustc_runs("set_first", &rust, driver);
}

/// PMAT-460 — v0.2.0 Track 1.B: list.append() mutation. Calls
/// `xs.append(...)` twice on a list parameter; the frontend marks
/// the parameter mutable, the Rust backend emits `mut xs: Vec<i64>`
/// and `.push()`. Verifies the post-mutation length via the value
/// returned.
#[test]
fn append_demo_emitted_rust_grows_list() {
    let rust = xpile_transpile_to_rust("append_demo.py");
    let driver = r#"
fn main() {
    // Empty input + 2 appends → length 2
    assert_eq!(double_and_append(vec![], 5i64), 2i64);
    // 3 existing + 2 appends → 5
    assert_eq!(double_and_append(vec![1i64, 2, 3], 10i64), 5i64);
}
"#;
    assert_rustc_runs("append_demo", &rust, driver);
}

/// PMAT-459 — v0.2.0 Track 1.B: builtin `len(xs)` over a list[int].
/// Returns the element count as Python int. Rust emits `xs.len() as i64`.
#[test]
fn len_list_emitted_rust_returns_count() {
    let rust = xpile_transpile_to_rust("len_list.py");
    let driver = r#"
fn main() {
    assert_eq!(count_xs(vec![1i64, 2, 3]), 3i64);
    assert_eq!(count_xs(vec![]), 0i64);
    assert_eq!(count_xs(vec![42i64]), 1i64);
}
"#;
    assert_rustc_runs("len_list", &rust, driver);
}

/// PMAT-459 — v0.2.0 Track 1.B: builtin `len(s)` over a str.
/// Returns the UTF-8 byte length (matching Rust's `String::len`
/// semantics — v0.2.0 first cut, codepoint-count is a Silver-tier
/// refinement in subsequent sub-tracks).
#[test]
fn len_str_emitted_rust_returns_byte_count() {
    let rust = xpile_transpile_to_rust("len_str.py");
    let driver = r#"
fn main() {
    assert_eq!(name_len(String::from("hello")), 5i64);
    assert_eq!(name_len(String::from("")), 0i64);
    assert_eq!(name_len(String::from("xpile")), 5i64);
}
"#;
    assert_rustc_runs("len_str", &rust, driver);
}

/// PMAT-654: `len(tuple)` folds to the tuple's arity (a compile-time constant).
/// Rust tuples have no `.len()` method, so the `Expr::Len` emission (`t.len()`)
/// was E0599. A Python tuple's length is fixed by its type. Cross-checked vs
/// python3; list `len()` still uses `.len()`.
#[test]
fn len_tuple() {
    let rust = xpile_transpile_to_rust("len_tuple.py");
    // tuple len folds to a literal; no `t.len()` / `p.len()` on a tuple
    assert!(
        !rust.contains("t.len()") && !rust.contains("p.len()"),
        "len(tuple) must not emit a .len() call on a tuple:\n{rust}"
    );
    // list len still emits .len()
    assert!(
        rust.contains("xs.len() as i64"),
        "list len() should still use .len():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(len_tuple_param((1, 2, 3)), 3);   // was E0599
    assert_eq!(len_tuple_literal(), 4);
    assert_eq!(len_tuple_mixed((1, String::from("a"))), 2);
    assert_eq!(len_list_regression(vec![5, 6, 7, 8, 9]), 5);
}
"#;
    assert_rustc_runs("len_tuple", &rust, driver);
}

/// PMAT-458 — v0.2.0 Track 1.B: for-each iteration over list[int].
/// Closes the spec §23 ⏳ entry "`for` over non-range iterables".
/// The frontend lowers `for x in xs:` (where xs has Type::List) to
/// Stmt::ForEach; the Rust backend emits `for x in xs.iter().cloned()`.
#[test]
fn sum_list_emitted_rust_returns_total() {
    let rust = xpile_transpile_to_rust("sum_list.py");
    let driver = r#"
fn main() {
    assert_eq!(total(vec![1i64, 2, 3, 4, 5]), 15i64);
    assert_eq!(total(vec![]), 0i64);
    assert_eq!(total(vec![42i64]), 42i64);
    assert_eq!(total(vec![-1i64, 1]), 0i64);
}
"#;
    assert_rustc_runs("sum_list", &rust, driver);
}

/// PMAT-457 — v0.2.0 Track 1.B: list indexed access `xs[0]`. The
/// frontend lowers ast::Expr::Subscript to Expr::Index; backends
/// emit `xs[i as usize].clone()` (Rust) / `xs[i.toNat]!` (Lean).
/// The rustc round-trip confirms the indexed value at runtime.
#[test]
fn list_first_emitted_rust_returns_first_element() {
    let rust = xpile_transpile_to_rust("list_first.py");
    let driver = r#"
fn main() {
    assert_eq!(first(vec![10i64, 20, 30]), 10i64);
    assert_eq!(first(vec![42i64]), 42i64);
    assert_eq!(first(vec![-1i64, 0, 1]), -1i64);
}
"#;
    assert_rustc_runs("list_first", &rust, driver);
}

/// PMAT-456 — v0.2.0 Track 1.B: list[str] literal exercising
/// Type::List(Box<Type::Str>) + Expr::ListLit of LitStr elements.
#[test]
fn names_list_emitted_rust_returns_string_vec() {
    let rust = xpile_transpile_to_rust("names_list.py");
    let driver = r#"
fn main() {
    let result = names();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], String::from("alice"));
    assert_eq!(result[1], String::from("bob"));
    assert_eq!(result[2], String::from("carol"));
}
"#;
    assert_rustc_runs("names_list", &rust, driver);
}

/// PMAT-456 — v0.2.0 Track 1.B: list[bool] literal exercising the
/// new Expr::LitBool variant. True/False lower to Rust `true`/`false`.
#[test]
fn flags_list_emitted_rust_returns_bool_vec() {
    let rust = xpile_transpile_to_rust("flags_list.py");
    let driver = r#"
fn main() {
    let result = flags();
    assert_eq!(result, vec![true, false, true]);
}
"#;
    assert_rustc_runs("flags_list", &rust, driver);
}

/// PMAT-455 — v0.2.0 Track 1.B foundation: list[int] literal.
/// `def squares() -> list[int]: return [1, 4, 9, 16, 25]` transpiles
/// to `pub fn squares() -> Vec<i64> { vec![1i64, 4i64, 9i64, 16i64, 25i64] }`
/// and the rustc round-trip green confirms the values + length.
#[test]
fn squares_list_emitted_rust_returns_int_vec() {
    let rust = xpile_transpile_to_rust("squares_list.py");
    let driver = r#"
fn main() {
    let result = squares();
    assert_eq!(result.len(), 5);
    assert_eq!(result, vec![1i64, 4i64, 9i64, 16i64, 25i64]);
}
"#;
    assert_rustc_runs("squares_list", &rust, driver);
}

/// PMAT-452 — v0.2.0 Track 1.A EXIT CRITERION: f-string lowering.
/// `f"Hello, {name}!"` parses to `JoinedStr { values: [Const, Fmt, Const] }`,
/// the frontend folds it to left-associative `Expr::Concat`, and the
/// Rust backend emits nested `format!("{}{}", ...)`. This is the
/// fixture cited by sub/v0.2.0-depyler-merger.md as the exit
/// criterion for the depyler-merger string lane.
#[test]
fn greet_fstring_emitted_rust_returns_formatted_string() {
    let rust = xpile_transpile_to_rust("greet_fstring.py");
    let driver = r#"
fn main() {
    assert_eq!(greet(String::from("world")), String::from("Hello, world!"));
    assert_eq!(greet(String::from("xpile")), String::from("Hello, xpile!"));
    assert_eq!(greet(String::from("")), String::from("Hello, !"));
}
"#;
    assert_rustc_runs("greet_fstring", &rust, driver);
}

/// PMAT-502am (Tranche 2): f-string format specs → Rust `format!` specs
/// (`.2f`→`.2`, `05d`→`05`, `>8`→`>8`, `4d`→`4`).
#[test]
fn fstring_spec() {
    let rust = xpile_transpile_to_rust("fstring_spec.py");
    assert!(
        // PMAT-659: the float-precision `.2` now NaN-guards (emits `__nf`); the
        // int/width/align specs keep the plain `format!` form.
        rust.contains("format!(\"{:.2}\", __nf)")
            && rust.contains("format!(\"{:05}\", n)")
            && rust.contains("format!(\"{:>8}\", name)")
            && rust.contains("format!(\"{:4}\", n)"),
        "expected translated format specs, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(price(3.14159), String::from("$3.14"));
    assert_eq!(padded(42), String::from("[00042]"));
    assert_eq!(aligned(String::from("hi")), String::from("|      hi|"));
    assert_eq!(width(42), String::from("  42"));
}
"#;
    assert_rustc_runs("fstring_spec", &rust, driver);
}

/// PMAT-451 — v0.2.0 Track 1.A: str + str concatenation via
/// `Expr::Concat`. `"hello, " + name` lowers to `format!("{}{}", ...)`
/// in Rust; verify the rustc round-trip produces `"hello, world"`.
#[test]
fn greet_concat_emitted_rust_returns_concatenated_string() {
    let rust = xpile_transpile_to_rust("greet_concat.py");
    let driver = r#"
fn main() {
    assert_eq!(greet(String::from("world")), String::from("hello, world"));
    assert_eq!(greet(String::from("")), String::from("hello, "));
    assert_eq!(greet(String::from("xpile")), String::from("hello, xpile"));
}
"#;
    assert_rustc_runs("greet_concat", &rust, driver);
}

/// PMAT-492/493b (sprint): Python string methods. `upper/lower/strip`
/// lower to `Expr::StrMethod` (→ Str: `.to_uppercase()` etc.);
/// `startswith/endswith` carry a pattern arg (→ Bool: `.starts_with`/
/// `.ends_with`).
#[test]
fn str_methods_emitted_rust_transforms_strings() {
    let rust = xpile_transpile_to_rust("str_methods.py");
    assert!(
        rust.contains(".to_uppercase()")
            && rust.contains(".to_lowercase()")
            && rust.contains(".trim_matches(|__c: char| __c.is_whitespace()")
            && rust.contains(".starts_with(")
            && rust.contains(".ends_with(")
            && rust.contains(".split(&(")
            && rust.contains(".join(&("),
        "expected str-method emissions in Rust, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(shout(String::from("hi")), String::from("HI"));
    assert_eq!(quiet(String::from("HI")), String::from("hi"));
    assert_eq!(clean(String::from("  hi  ")), String::from("hi"));
    assert!(is_greeting(String::from("hello there")));
    assert!(!is_greeting(String::from("goodbye")));
    assert!(is_question(String::from("ok?")));
    assert!(!is_question(String::from("ok")));
    assert_eq!(words(String::from("a b c")), vec![String::from("a"), String::from("b"), String::from("c")]);
    assert_eq!(joined(vec![String::from("a"), String::from("b"), String::from("c")]), String::from("a b c"));
}
"#;
    assert_rustc_runs("str_methods", &rust, driver);
}

/// PMAT-494 (sprint): tuples — multiple return + `tuple[...]`
/// annotation. `return a, b` → `Expr::TupleLit` (Rust `(e0, e1)`),
/// `tuple[T0, T1]` → `(T0, T1)`. The driver destructures the returned
/// tuples (Python-side unpacking is a follow-up slice).
#[test]
fn tuples_emitted_rust_multiple_return() {
    let rust = xpile_transpile_to_rust("tuples.py");
    assert!(
        rust.contains("-> (i64, i64)") && rust.contains("-> (String, i64)"),
        "expected tuple return types in emitted Rust, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let (q, r) = divmod_pair(17, 5);
    assert_eq!(q, 3);
    assert_eq!(r, 2);
    let (s, c) = tagged(String::from("x"), 9);
    assert_eq!(s, String::from("x"));
    assert_eq!(c, 9);
}
"#;
    assert_rustc_runs("tuples", &rust, driver);
}

/// PMAT-494b (sprint): tuple unpacking `a, b = <expr>` → `Stmt::LetTuple`,
/// emitting Rust `let (x, y) = <value>;`.
#[test]
fn tuple_unpack_emitted_rust_destructures() {
    let rust = xpile_transpile_to_rust("tuple_unpack.py");
    assert!(
        rust.contains("let (x, y) = "),
        "expected a tuple-destructuring let, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(swap_diff(5, 3), -2);
    assert_eq!(sum_pair(4, 9), 13);
}
"#;
    assert_rustc_runs("tuple_unpack", &rust, driver);
}

/// PMAT-711: a single-element tuple-unpack `x, = t` / `(x,) = t` emitted
/// `let (x) = t` (grouping → binds the whole tuple to x) → E0308. Now emits the
/// 1-tuple destructure `let (x,) = t`. Cross-checked vs python3 (HUNT-V9 V9-12).
#[test]
fn single_elem_tuple_unpack() {
    let rust = xpile_transpile_to_rust("single_elem_tuple_unpack.py");
    assert!(
        rust.contains("let (x,) = ") && rust.contains("let (y,) = "),
        "single-element unpack should have the trailing comma:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(unwrap((42,)), 42);
    assert_eq!(unwrap_str(("hi".to_string(),)), "hi");
}
"#;
    assert_rustc_runs("single_elem_tuple_unpack", &rust, driver);
}

/// PMAT-662: a `_` discard in a tuple-unpack must never be `mut`. Repeated
/// discards (`a, _, _ = t`) aggregated the `_` count in the mutability pre-walk,
/// so the 2nd+ `_` was flagged mutable → invalid `mut _`. Cross-checked vs python3.
#[test]
fn lettuple_underscore_mut() {
    let rust = xpile_transpile_to_rust("lettuple_underscore_mut.py");
    assert!(
        !rust.contains("mut _"),
        "a `_` wildcard must never be `mut`:\n{rust}"
    );
    // a real mutated binding still gets `mut`
    assert!(
        rust.contains("let (mut a, _, _)") && rust.contains("let (_, mut x, _)"),
        "named mutated bindings should keep `mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(two_discard((1, 2, 3)), 2);   // was `mut _` E
    assert_eq!(all_discard(), 99);            // was 4x `mut _`
    assert_eq!(mixed((1, 10, 3)), 15);
    assert_eq!(single_discard_regression((9, 5)), 6);
}
"#;
    assert_rustc_runs("lettuple_underscore_mut", &rust, driver);
}

/// PMAT-645/646: starred unpacking `n…, *star, m… = xs` (star at ANY position)
/// over a list. Desugars to prefix `let n_i = xs[i]`, the star `let star =
/// xs[p:len-s]`, and suffix `let m_j = xs[len-(s-j)]` — no new IR (reuses Index,
/// Slice, Len). The source variable is borrowed (not moved). Cross-checked vs
/// python3.
#[test]
fn starred_unpack() {
    let rust = xpile_transpile_to_rust("starred_unpack.py");
    // Prefix names index; the star binds a slice (block form) of the tail.
    assert!(
        rust.contains("let __li = (0i64) as usize") && rust.contains("__sl[__lo..__hi].to_vec()"),
        "starred unpack should index the head and slice the tail:\n{rust}"
    );
    // PMAT-647: a later-mutated starred binding must be `let mut`.
    assert!(
        rust.contains("let mut rest:"),
        "a mutated starred binding should be `let mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(head_tail(vec![1, 2, 3, 4]), 109);  // 1*100 + (2+3+4)
    assert_eq!(two_prefix(vec![10, 20, 30, 40, 50]), 33); // 10+20+3
    assert_eq!(star_only(vec![1, 2, 3]), 6);
    assert_eq!(keeps_source(vec![5, 6, 7, 8]), 12); // 5 + 3 + 4 (xs not moved)
    // PMAT-646: star-first and star-mid.
    assert_eq!(star_first(vec![1, 2, 3, 4]), 406);          // last*100 + sum(init)
    assert_eq!(star_mid(vec![1, 2, 3, 4, 5]), 1509);        // 1*1000 + 5*100 + (2+3+4)
    assert_eq!(two_pre_two_suf(vec![1, 2, 3, 4, 5, 6]), 21); // 1+2+5+6 + (3+4)
    // PMAT-647: a mutated starred binding must be `let mut`.
    assert_eq!(star_then_mutate(vec![5, 3, 1, 2]), 10);     // 5 + 1 + 4
}
"#;
    assert_rustc_runs("starred_unpack", &rust, driver);
}

/// PMAT-700: a plain (no-star) tuple-unpack over a LIST — `a, b = xs` — now
/// transpiles (it was rejected "expected a tuple", though `a, *b = xs` over the
/// same list worked). Python unpacks by position with an exact-length check;
/// xpile emits a length assert + `let a = xs[0]; …`. Cross-checked vs python3
/// (HUNT-V8 item V8-3).
#[test]
fn list_positional_unpack() {
    let rust = xpile_transpile_to_rust("list_positional_unpack.py");
    assert!(
        rust.contains("ValueError: expected 2 values to unpack")
            && rust.contains("let __li = (0i64) as usize")
            && rust.contains("__listunpack"),
        "list unpack should length-assert + positionally index:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(two(vec![10, 7]), 17);
    assert_eq!(three(vec!["x".to_string(), "y".to_string(), "z".to_string()]), "xz");
    assert_eq!(from_list_lit(5), 15); // RHS list literal → temp-bound, unpacked
    // length mismatch panics like Python's ValueError
    let r = std::panic::catch_unwind(|| two(vec![1, 2, 3]));
    assert!(r.is_err(), "len-mismatch should panic (ValueError)");
}
"#;
    assert_rustc_runs("list_positional_unpack", &rust, driver);
}

/// PMAT-496 (sprint): bounded slicing `xs[a:b]`. List → `Vec` via
/// `.to_vec()`, str → `String` via `.to_string()` (byte-indexed).
#[test]
fn slicing_emitted_rust_list_and_str() {
    let rust = xpile_transpile_to_rust("slicing.py");
    // PMAT-539: bounded slices use the resolve+clamp block form. PMAT-567: a
    // str slice collects to `Vec<char>` then back to a String (char-indexed).
    assert!(
        rust.contains("__sl[__lo..__hi].to_vec()")
            && rust.contains("__sl[__lo..__hi].iter().collect::<String>()"),
        "expected slice emissions in Rust, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(middle(vec![10, 20, 30, 40]), vec![20, 30]);
    assert_eq!(prefix(String::from("hello")), String::from("hel"));
}
"#;
    assert_rustc_runs("slicing", &rust, driver);
}

/// PMAT-502c (Tranche 2): `sorted(xs)` → a new sorted list
/// (`{ let mut __xv = xs.clone(); __xv.sort(); __xv }`).
#[test]
fn sorted_builtin_int_and_str() {
    let rust = xpile_transpile_to_rust("sorted_builtin.py");
    assert!(
        rust.contains(".clone(); __xv.sort();"),
        "expected sort block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(order(vec![3, 1, 2]), vec![1, 2, 3]);
    assert_eq!(
        order_str(vec![String::from("c"), String::from("a"), String::from("b")]),
        vec![String::from("a"), String::from("b"), String::from("c")]
    );
}
"#;
    assert_rustc_runs("sorted_builtin", &rust, driver);
}

/// PMAT-502z (Tranche 2): `sorted(xs, key=lambda p: e)` → `sort_by_key`
/// (first lambda/closure support, bounded to the `key=` position).
#[test]
fn sorted_key_lambda() {
    let rust = xpile_transpile_to_rust("sorted_key.py");
    assert!(
        rust.contains("sort_by_key(|__k| { let w = __k.clone(); w.chars().count() as i64 }"),
        "expected sort_by_key emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(
        by_len(vec![String::from("ccc"), String::from("a"), String::from("bb")]),
        vec![String::from("a"), String::from("bb"), String::from("ccc")]
    );
    assert_eq!(by_neg(vec![1, 3, 2]), vec![3, 2, 1]);
    assert_eq!(
        by_len_desc(vec![String::from("a"), String::from("ccc"), String::from("bb")]),
        vec![String::from("ccc"), String::from("bb"), String::from("a")]
    );
}
"#;
    assert_rustc_runs("sorted_key", &rust, driver);
}

/// PMAT-502f (Tranche 2): `sorted(xs, reverse=True)` → descending order
/// (`__xv.sort(); __xv.reverse();`); `reverse=False` stays ascending.
#[test]
fn sorted_reverse_flag() {
    let rust = xpile_transpile_to_rust("sorted_reverse.py");
    assert!(
        rust.contains("__xv.sort(); __xv.reverse();"),
        "expected sort-then-reverse block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(order_desc(vec![3, 1, 2]), vec![3, 2, 1]);
    assert_eq!(order_asc(vec![3, 1, 2]), vec![1, 2, 3]);
}
"#;
    assert_rustc_runs("sorted_reverse", &rust, driver);
}

/// PMAT-555 (Tranche 2): in-place `xs.sort(reverse=True)` → a descending sort
/// via a reversed comparator (`.sort_by(|a, b| b.cmp(a))` for ints,
/// `b.partial_cmp(a).unwrap()` for floats); `reverse=False` stays a plain
/// `.sort()`. Cross-checked vs python3 (9, 1, 3.5, 54).
#[test]
fn sort_inplace_reverse() {
    let rust = xpile_transpile_to_rust("sort_inplace_reverse.py");
    assert!(
        rust.contains("sort_by(|a, b| b.cmp(a))"),
        "expected a reversed-comparator descending sort, got:\n{rust}"
    );
    assert!(
        // PMAT-616: NaN-safe comparator (Python doesn't raise on NaN).
        rust.contains("b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)"),
        "expected a float descending sort, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(top_int(vec![3, 1, 4, 1, 5, 9, 2, 6]), 9);
    assert_eq!(bottom_int(vec![3, 1, 4, 1, 5]), 1);
    assert_eq!(top_float(vec![1.5, 3.5, 2.5]), 3.5);
    assert_eq!(desc_concat(vec![3, 1, 4, 1, 5]), 54);
}
"#;
    assert_rustc_runs("sort_inplace_reverse", &rust, driver);
}

/// PMAT-561 (Tranche 2): in-place keyed sort `xs.sort(key=lambda v: e)` (and
/// `key=` + `reverse=`). Desugars to `xs = sorted(xs, key=…, reverse=…)`,
/// reusing the `Expr::Sorted` / `SortKey` machinery (`.sort_by_key(|__k| { let
/// p = __k.clone(); … })`). The receiver is already `mut` (the pre-walk keys on
/// the `sort` method). Cross-checked vs python3 (9, 1, 1, 8).
#[test]
fn sort_inplace_key() {
    let rust = xpile_transpile_to_rust("sort_inplace_key.py");
    assert!(
        rust.contains("sort_by_key(|__k|"),
        "in-place keyed sort should reuse the sorted() key machinery:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sort_desc_key(vec![3, 1, 4, 1, 5, 9, 2, 6]), 9);
    assert_eq!(sort_by_square(vec![3, 1, 4, 1, 5]), 1);
    assert_eq!(sort_pairs_by_second(vec![(1, 5), (2, 1), (3, 3)]), 1);
    assert_eq!(sort_key_reverse(vec![23, 17, 45, 8]), 8);
}
"#;
    assert_rustc_runs("sort_inplace_key", &rust, driver);
}

/// PMAT-502d (Tranche 2): `reversed(xs)` (and `list(reversed(xs))`) →
/// a new reversed list (`{ let mut __xv = xs.clone(); __xv.reverse(); __xv }`).
#[test]
fn reversed_builtin_int_and_str() {
    let rust = xpile_transpile_to_rust("reversed_builtin.py");
    assert!(
        rust.contains(".clone(); __xv.reverse();"),
        "expected reverse block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(flip(vec![1, 2, 3]), vec![3, 2, 1]);
    assert_eq!(
        flip_str(vec![String::from("a"), String::from("b"), String::from("c")]),
        vec![String::from("c"), String::from("b"), String::from("a")]
    );
}
"#;
    assert_rustc_runs("reversed_builtin", &rust, driver);
}

/// PMAT-596: `reversed(s)` over a `str` reverses its characters (Python yields
/// an iterator of 1-char strings → `list[str]`). Lowered to
/// `Reversed(StrChars(s))` so the textbook idioms compose. Previously
/// `reversed(str)` fell through to a bare `reversed(...)` call (rustc E0425).
/// Cross-checked vs python3: `reverse_string("hello")=="olleh"`,
/// `first_reversed("hello")=="o"`, `reversed_len("hello")==5`.
#[test]
fn reversed_str() {
    let rust = xpile_transpile_to_rust("reversed_str.py");
    // The str path materializes chars then reverses (no bare `reversed()` call,
    // which would be E0425 — the rustc round-trip below is the real guard).
    assert!(
        rust.contains(".chars().map(|__c| __c.to_string()).collect::<Vec<String>>()")
            && rust.contains(".reverse();"),
        "reversed(str) must materialize reversed chars:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(reverse_string("hello".to_string()), "olleh");
    assert_eq!(first_reversed("hello".to_string()), "o");
    assert_eq!(reversed_len("hello".to_string()), 5);
}
"#;
    assert_rustc_runs("reversed_str", &rust, driver);
}

/// PMAT-502ab (Tranche 2): `filter(lambda p: pred, xs)` → an order-preserving
/// materialized list of elements where the Bool predicate holds.
#[test]
fn filter_lambda() {
    let rust = xpile_transpile_to_rust("filter_lambda.py");
    assert!(
        rust.contains(".iter().cloned().filter(|__k| { let x = __k.clone();")
            && rust.contains(").collect::<Vec<_>>()"),
        "expected filter emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(positives(vec![-1, 2, -3, 4, 0]), vec![2, 4]);
    assert_eq!(evens(vec![1, 2, 3, 4, 5, 6]), vec![2, 4, 6]);
    assert_eq!(
        nonempty(vec![String::from("a"), String::from(""), String::from("bb")]),
        vec![String::from("a"), String::from("bb")]
    );
}
"#;
    assert_rustc_runs("filter_lambda", &rust, driver);
}

/// PMAT-502ac (Tranche 2): `map(lambda p: e, xs)` → a materialized list of
/// transformed elements; result element type = the body's type.
#[test]
fn map_lambda() {
    let rust = xpile_transpile_to_rust("map_lambda.py");
    assert!(
        rust.contains(".iter().cloned().map(|__k| { let x = __k.clone();")
            && rust.contains(").collect::<Vec<_>>()"),
        "expected map emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(doubled(vec![1, 2, 3]), vec![2, 4, 6]);
    assert_eq!(
        lengths(vec![String::from("a"), String::from("bbb"), String::from("cc")]),
        vec![1, 3, 2]
    );
    assert_eq!(to_floats(vec![1, 2, 3]), vec![1.0, 2.0, 3.0]);
}
"#;
    assert_rustc_runs("map_lambda", &rust, driver);
}

/// PMAT-706: `map`/`filter` with a bare callable NAME — `list(map(len, xs))`,
/// `list(filter(bool, xs))`, `list(filter(None, xs))`, `list(map(myfunc, xs))` —
/// were rejected ("produces I64"); only the lambda form worked. Synthesizes
/// `name(__x)` (filter needs a Bool predicate; `filter(None)` keeps truthy).
/// Cross-checked vs python3 (HUNT-V8 item V8-6).
#[test]
fn map_filter_named() {
    let rust = xpile_transpile_to_rust("map_filter_named.py");
    let driver = r#"
fn main() {
    assert_eq!(lens(vec!["a".into(), "bb".into(), "ccc".into()]), vec![1, 2, 3]);
    assert_eq!(strs(vec![1, 2, 3]), vec!["1".to_string(), "2".to_string(), "3".to_string()]);
    assert_eq!(keep_truthy(vec![0, 1, 2, 0, 3]), vec![1, 2, 3]);
    assert_eq!(
        keep_none(vec!["a".into(), "".into(), "b".into(), "".into()]),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(keep_pos(vec![-1, 2, -3, 4]), vec![2, 4]); // user predicate
}
"#;
    assert_rustc_runs("map_filter_named", &rust, driver);
}

/// PMAT-502ai (Tranche 2): standalone `enumerate(xs)` / `zip(a, b)` →
/// materialized `Vec`s of tuples (compose with `for`-pair loops and `len`).
#[test]
fn enumerate_zip_standalone() {
    let rust = xpile_transpile_to_rust("enumerate_zip_standalone.py");
    assert!(
        rust.contains(".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e))")
            && rust.contains(".iter().cloned().zip("),
        "expected enumerate/zip emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    // idx_sum: sum(i*x) over enumerate -> 0*10 + 1*20 + 2*30 = 80
    assert_eq!(idx_sum(vec![10, 20, 30]), 80);
    // dot: 1*4 + 2*5 + 3*6 = 32
    assert_eq!(dot(vec![1, 2, 3], vec![4, 5, 6]), 32);
    // zip truncates to shorter
    assert_eq!(n_pairs(vec![1, 2, 3], vec![9, 9]), 2);
}
"#;
    assert_rustc_runs("enumerate_zip_standalone", &rust, driver);
}

/// PMAT-707: 3-way `zip(a, b, c)` in EXPRESSION position — `list(zip(a, b, c))` —
/// was rejected/mis-typed as I64 (only 2-way worked in expr position; 3-way only
/// in a for-loop). Flattened via a nested `Zip` + a re-tupling `Map` → a flat
/// list of 3-tuples. Cross-checked vs python3 (HUNT-V8 item V8-12).
#[test]
fn zip3_expr() {
    let rust = xpile_transpile_to_rust("zip3_expr.py");
    assert!(
        rust.contains("Vec<(i64, String, i64)>"),
        "3-way zip should produce a flat 3-tuple list type:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(z3_len(vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]), 3);
    assert_eq!(z3_first(vec![1, 2], vec!["x".into(), "y".into()], vec![3, 4]), "x");
    assert_eq!(z2_regression(vec![1, 2], vec![3, 4]), 2); // 2-way regression
}
"#;
    assert_rustc_runs("zip3_expr", &rust, driver);
}

/// PMAT-502e (Tranche 2): 1-arg `min(xs)`/`max(xs)` over an int list →
/// `xs.iter().cloned().min().unwrap()` (or `.max()`). (PMAT-502er switched the
/// non-float reduction from `.copied()` to `.cloned()` so `String` works too;
/// `i64` is `Clone`, so this is semantically identical.)
#[test]
fn list_minmax_builtin() {
    let rust = xpile_transpile_to_rust("list_minmax_builtin.py");
    assert!(
        rust.contains(".iter().cloned().min().unwrap()")
            && rust.contains(".iter().cloned().max().unwrap()"),
        "expected min/max reduction emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(smallest(vec![3, 1, 2]), 1);
    assert_eq!(largest(vec![3, 1, 2]), 3);
    assert_eq!(span(vec![3, 1, 2, 9, 4]), 8);
}
"#;
    assert_rustc_runs("list_minmax_builtin", &rust, driver);
}

/// PMAT-502aa (Tranche 2): `min(xs, key=lambda)` / `max(xs, key=lambda)` →
/// `min_by_key`/`max_by_key` (returns the element; any element type).
#[test]
fn minmax_key_lambda() {
    let rust = xpile_transpile_to_rust("minmax_key.py");
    assert!(
        rust.contains(
            ".iter().cloned().rev().max_by_key(|__k| { let w = __k.clone(); w.chars().count() as i64 })"
        ) && rust.contains(".iter().cloned().min_by_key("),
        "expected min/max_by_key emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(
        longest(vec![String::from("a"), String::from("ccc"), String::from("bb")]),
        String::from("ccc")
    );
    assert_eq!(
        shortest(vec![String::from("ccc"), String::from("a"), String::from("bb")]),
        String::from("a")
    );
    assert_eq!(closest_to_zero(vec![5, -2, 8, -1, 3]), -1);
}
"#;
    assert_rustc_runs("minmax_key", &rust, driver);
}

/// PMAT-653: `max(xs, key=…)` / `min(xs, key=…)` with a FLOAT-returning key.
/// The compared values are `f64` (no `Ord`), so the `max_by_key`/`min_by_key`
/// emission was E0277; a float key now emits `max_by`/`min_by` with
/// `partial_cmp` (and `max` still reverses so ties resolve to the FIRST
/// element). An int key keeps the `max_by_key` path. Cross-checked vs python3.
#[test]
fn minmax_float_key() {
    let rust = xpile_transpile_to_rust("minmax_float_key.py");
    assert!(
        rust.contains(".iter().cloned().rev().max_by(|__a, __b|")
            && rust.contains(".partial_cmp(&{ let x = __b.clone();"),
        "float key should emit max_by/min_by with partial_cmp:\n{rust}"
    );
    // the int-key function must still use the max_by_key path
    assert!(
        rust.contains(".rev().max_by_key(|__k|"),
        "int key should keep max_by_key:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(max_by_ratio(vec![1, 5, 3, 2]), 5);  // was E0277
    assert_eq!(min_by_ratio(vec![1, 5, 3, 2]), 1);
    assert_eq!(max_tie_first(vec![7, 8, 9]), 7);     // first element on a tie
    assert_eq!(max_int_key(vec![1, 5, 3, 2]), 1);    // int-key regression
}
"#;
    assert_rustc_runs("minmax_float_key", &rust, driver);
}

/// PMAT-502j (Tranche 2): `all(xs)`/`any(xs)` over a `list[bool]` →
/// `xs.iter().all(|&__b| __b)` / `.any(|&__b| __b)`.
#[test]
fn bool_reduce_all_any() {
    let rust = xpile_transpile_to_rust("bool_reduce.py");
    assert!(
        rust.contains(".iter().all(|&__b| __b)") && rust.contains(".iter().any(|&__b| __b)"),
        "expected all/any reduction emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(all_true(vec![true, true, true]), true);
    assert_eq!(all_true(vec![true, false, true]), false);
    assert_eq!(any_true(vec![false, false, true]), true);
    assert_eq!(any_true(vec![false, false, false]), false);
    assert_eq!(all_of_literals(), false);
}
"#;
    assert_rustc_runs("bool_reduce", &rust, driver);
}

/// PMAT-665: `any()`/`all()` over a list[int]/[float]/[str] apply Python
/// per-element truthiness. These emitted a bare `any(xs)` (E0425); each element
/// is now mapped to a bool (int → `!= 0`, float → `!= 0.0`, str → non-empty)
/// before the reduce. A list[bool] reduces directly. Cross-checked vs python3.
#[test]
fn any_all_truthy() {
    let rust = xpile_transpile_to_rust("any_all_truthy.py");
    assert!(
        !rust.contains("(any(xs)") && !rust.contains("(all(xs)"),
        "any/all over a non-bool list must not emit a bare free call:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(any_int(vec![0, 0, 3]), 1);
    assert_eq!(any_int(vec![0, 0]), 0);
    assert_eq!(all_int(vec![1, 2, 3]), 1);
    assert_eq!(all_int(vec![1, 0]), 0);
    assert_eq!(any_str(vec![String::new(), "x".to_string()]), 1);
    assert_eq!(any_str(vec![String::new(), String::new()]), 0);
    assert_eq!(all_float(vec![1.0, 2.0]), 1);
    assert_eq!(all_float(vec![1.0, 0.0]), 0);
    assert_eq!(any_bool_regression(vec![false, true]), 1);
}
"#;
    assert_rustc_runs("any_all_truthy", &rust, driver);
}

/// PMAT-572 (Tranche 2): **correctness** — tuple-unpack that REASSIGNS
/// already-bound names (`a, b = b, a % b` / `a, b = b, a + b`) inside a loop/if
/// body must reassign, not emit a fresh `let (mut a, mut b)` (which only shadows
/// within the nested block → outer vars never change → **Euclid GCD infinite
/// loop, iterative Fibonacci all-zeros**). The shared unpack helper evaluates
/// all RHS into temps first (swap-safe) then `Assign`s each. Found by the
/// differential hunt. Cross-checked vs python3 (6, 55, 24, 21).
#[test]
fn tuple_reassign_loop() {
    let rust = xpile_transpile_to_rust("tuple_reassign_loop.py");
    let driver = r#"
fn main() {
    assert_eq!(gcd(48, 18), 6);
    assert_eq!(fib(10), 55);
    assert_eq!(max_product(vec![-2, 3, -4]), 24);
    assert_eq!(swap_top(1, 2), 21);
}
"#;
    assert_rustc_runs("tuple_reassign_loop", &rust, driver);
}

/// PMAT-571 (Tranche 2): 3-arg `pow(base, exp, mod)` — modular exponentiation.
/// Was a bare `pow(...)` call → undefined Rust fn (E0425, transpile-success →
/// invalid Rust). Now an inline square-and-multiply that reduces mod m each step
/// with i128 products (no overflow even near i64::MAX); zero modulus / negative
/// exponent panic. Found by the differential hunt. Cross-checked vs python3
/// (24, 9, 7269837747581970906, 1, 0).
#[test]
fn pow_mod() {
    let rust = xpile_transpile_to_rust("pow_mod.py");
    assert!(
        // PMAT-619: modexp runs on the magnitude |m| in i128.
        rust.contains("(__pmm as i128).abs()") && rust.contains("__pmr * __pmb"),
        "3-arg pow should emit i128 modular exponentiation:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(modpow(2, 10, 1000), 24);
    assert_eq!(modpow(7, 256, 13), 9);
    assert_eq!(big_mod(123456789, 1000), 7269837747581970906);
    assert_eq!(neg_base(3), 1);
    assert_eq!(mod_one(5, 3), 0);
}
"#;
    assert_rustc_runs("pow_mod", &rust, driver);
}

/// PMAT-605 + PMAT-619: 3-arg `pow(a, b, m)` with a NEGATIVE modulus. Python's
/// result takes the sign of the modulus (range `(m, 0]`). PMAT-605 re-signed the
/// residue, but the base-normalization (`if __t < 0 { __t + __pmm }`) and the
/// `% __pmm` reductions still assumed a positive modulus, so a NEGATIVE base with
/// a negative modulus (`pow(-2, 3, -5)`) gave a wrong value (hunt #7 H7-18).
/// PMAT-619 reworks the whole modexp on the magnitude `|m|` (i128) then
/// sign-corrects. Cross-checked vs python3 incl. negative bases.
#[test]
fn pow_negative_modulus() {
    let rust = xpile_transpile_to_rust("pow_negative_modulus.py");
    assert!(
        // PMAT-619: sign-correct by subtracting the magnitude (= `+ m`).
        rust.contains("if __pmm < 0 && __pmr != 0 { __pmr -= __pma; }"),
        "negative modulus must re-sign the modpow result:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(mp(10, 2, -3), -2);
    assert_eq!(mp(2, 5, -100), -68);
    assert_eq!(mp(7, 3, -5), -2);
    assert_eq!(mp(5, 3, -7), -1);
    // PMAT-619: NEGATIVE base with a negative modulus (the H7-18 bug).
    assert_eq!(mp(-2, 3, -5), -3);
    assert_eq!(mp(-3, 2, -7), -5);
    assert_eq!(mp(-5, 3, -11), -4);
    // negative base with a POSITIVE modulus.
    assert_eq!(mp(-2, 3, 5), 2);
    assert_eq!(mp(-5, 3, 11), 7);
    // positive modulus is unchanged.
    assert_eq!(mp(10, 2, 3), 1);
    assert_eq!(mp(2, 5, 100), 32);
}
"#;
    assert_rustc_runs("pow_negative_modulus", &rust, driver);
}

/// PMAT-607: `pow()` with a bool base (Python bool is an int subtype,
/// `pow(True, n) == pow(1, n)`). The pow builtin only handled int/float bases
/// and fell through to a bare `pow(...)` call (E0425); the bool operand is now
/// coerced to i64. Cross-checked vs python3: bp2(True,5)=1, bp2(False,3)=0,
/// bp3(True,5,7)=1, bp3(False,0,7)=1.
#[test]
fn pow_bool_base() {
    let rust = xpile_transpile_to_rust("pow_bool_base.py");
    assert!(
        rust.contains("(((flag) as i64)).checked_pow(")
            && rust.contains("let __pmb0 = (((flag) as i64))"),
        "bool base must coerce to i64 in both pow forms:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(bp2(true, 5), 1);
    assert_eq!(bp2(false, 3), 0);
    assert_eq!(bp3(true, 5, 7), 1);
    assert_eq!(bp3(false, 0, 7), 1);
}
"#;
    assert_rustc_runs("pow_bool_base", &rust, driver);
}

/// PMAT-570 (Tranche 2): **correctness** — negative-literal `xs.pop(-k)` /
/// `del xs[-k]` remove from the end. Both emitted `remove((-k) as usize)` →
/// `usize::MAX` → panic; now resolve to `len(xs) - k` with the index bound to a
/// temp before `remove` (E0502). Positive indices unchanged. Found by the
/// differential hunt. Cross-checked vs python3 (3, 3, 10, 7, 6, 9).
#[test]
fn neg_pop_del() {
    let rust = xpile_transpile_to_rust("neg_pop_del.py");
    assert!(
        rust.contains("let __pi =") && rust.contains("let __di ="),
        "negative pop/del should bind the resolved index to a temp:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pop_last(vec![1, 2, 3]), 3);
    assert_eq!(pop_2nd_last(vec![1, 2, 3, 4]), 3);
    assert_eq!(pop_front(vec![10, 20, 30]), 10);
    assert_eq!(pop_noarg(vec![5, 6, 7]), 7);
    assert_eq!(del_last(vec![1, 2, 3, 4]), 6);
    assert_eq!(del_first(vec![1, 2, 3, 4]), 9);
}
"#;
    assert_rustc_runs("neg_pop_del", &rust, driver);
}

/// PMAT-639: **correctness** — a runtime-negative list index wraps like Python
/// (`xs[-1]` is the last element) instead of `i as usize` underflowing to
/// `usize::MAX` and panicking. A variable/computed index gets the wrap; a
/// non-negative literal index keeps the bare fast path; `Expr::Index` is
/// list-only (str/dict have their own paths). Found by a differential hunt;
/// cross-checked vs python3.
#[test]
fn neg_runtime_list_index() {
    let rust = xpile_transpile_to_rust("neg_runtime_list_index.py");
    // Variable index gets the negative-wrap; literal index stays bare.
    assert!(
        rust.contains("if __li < 0 { __lc.len() as i64 + __li }"),
        "runtime index negative-wrap:\n{rust}"
    );
    assert!(
        rust.contains("let __li = (0i64) as usize"),
        "non-negative literal index is bounds-checked (PMAT-764):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(at(vec![10, 20, 30], -1), 30);
    assert_eq!(at(vec![10, 20, 30], -2), 20);
    assert_eq!(at(vec![10, 20, 30], 1), 20);
    assert_eq!(last_and_first(vec![5, 6, 7]), 12); // 7 + 5
    assert_eq!(neg_nested(vec![vec![1, 2], vec![3, 4]], -1), 3);
    assert_eq!(literal(vec![10, 20, 30]), 40);
}
"#;
    assert_rustc_runs("neg_runtime_list_index", &rust, driver);
}

/// PMAT-569 (Tranche 2): **correctness** — list-of-list repeat `[[0]] * n`.
/// A list repeat used slice `repeat`, which needs `T: Copy`, so `Vec<Vec<_>>`
/// failed to compile (E0277). Now a list repeat clones its elements
/// (`(0..k).flat_map(|_| __rep.iter().cloned()).collect::<Vec<_>>()`); str
/// repeat keeps `String::repeat`; int-element list repeat unchanged in behavior.
/// Found by the differential hunt. Cross-checked vs python3 (3, 8, 14, "ababab").
#[test]
fn list_of_list_repeat() {
    let rust = xpile_transpile_to_rust("list_of_list_repeat.py");
    assert!(
        rust.contains(".flat_map(|_| __rep.iter().cloned()).collect::<Vec<_>>()"),
        "list repeat should clone-repeat:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(grid(3), 3);
    assert_eq!(grid_cells(4), 8);
    assert_eq!(int_repeat(5), 14);
    assert_eq!(str_repeat(3), "ababab");
}
"#;
    assert_rustc_runs("list_of_list_repeat", &rust, driver);
}

/// PMAT-502k (Tranche 2): sequence repetition `seq * n` / `n * seq` →
/// str → `String::repeat`; list → clone-repeat (PMAT-569).
#[test]
fn seq_repeat() {
    let rust = xpile_transpile_to_rust("seq_repeat.py");
    assert!(
        rust.contains(").repeat(((") && rust.contains(").max(0)) as usize)"),
        "expected repeat emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(bar(3), String::from("==="));
    assert_eq!(left_mul(2), String::from("abab"));
    assert_eq!(zeros(3), vec![0, 0, 0]);
    assert_eq!(repeat_pair(2), vec![1, 2, 1, 2]);
    assert_eq!(clamp_negative(), String::from(""));
}
"#;
    assert_rustc_runs("seq_repeat", &rust, driver);
}

/// PMAT-502l (Tranche 2): more string methods — `.lstrip()`/`.rstrip()` (Str)
/// and `.find(sub)`/`.count(sub)` (Int).
#[test]
fn str_methods_more() {
    let rust = xpile_transpile_to_rust("str_methods_more.py");
    assert!(
        rust.contains(".trim_start_matches(|__c: char| __c.is_whitespace()")
            && rust.contains(".trim_end_matches(|__c: char| __c.is_whitespace()")
            && rust.contains(".map(|__b| __s[..__b].chars().count() as i64).unwrap_or(-1)")
            && rust.contains(".matches(&(")
            && rust.contains(".count() as i64"),
        "expected lstrip/rstrip/find/count emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(trim_left(String::from("  hi  ")), String::from("hi  "));
    assert_eq!(trim_right(String::from("  hi  ")), String::from("  hi"));
    assert_eq!(index_of(String::from("hello"), String::from("ll")), 2);
    assert_eq!(index_of(String::from("hello"), String::from("z")), -1);
    assert_eq!(occurrences(String::from("banana"), String::from("a")), 3);
    assert_eq!(occurrences(String::from("banana"), String::from("na")), 2);
}
"#;
    assert_rustc_runs("str_methods_more", &rust, driver);
}

/// PMAT-502ag (Tranche 2): string classification predicates
/// `.isdigit()`/`.isalpha()`/`.isspace()` → `Bool` (empty → False).
#[test]
fn str_predicates() {
    let rust = xpile_transpile_to_rust("str_predicates.py");
    assert!(
        rust.contains(".chars().all(|__c| __c.is_ascii_digit())")
            && rust.contains("is_alphabetic()")
            && rust.contains("is_whitespace()")
            && rust.contains(".is_empty() &&"),
        "expected predicate emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(all_digits(String::from("123")));
    assert!(!all_digits(String::from("12a")));
    assert!(!all_digits(String::from("")));
    assert!(all_alpha(String::from("abc")));
    assert!(!all_alpha(String::from("ab1")));
    assert!(all_space(String::from("  \t")));
    assert!(!all_space(String::from(" x ")));
}
"#;
    assert_rustc_runs("str_predicates", &rust, driver);
}

/// PMAT-643: `str.isnumeric()` — True iff non-empty and every char is numeric
/// (Unicode Number categories, via Rust `char::is_numeric()`), broader than
/// `isdigit` (e.g. `½`, `²` are numeric but not digits). Cross-checked vs python3.
#[test]
fn str_isnumeric() {
    let rust = xpile_transpile_to_rust("str_isnumeric.py");
    assert!(
        rust.contains(".chars().all(|__c| __c.is_numeric())"),
        "isnumeric should use char::is_numeric():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(is_num(String::from("123")), 1);
    assert_eq!(is_num(String::from("")), 0);
    assert_eq!(is_num(String::from("12a")), 0);
    assert_eq!(is_num(String::from("\u{00BD}")), 1); // ½ is numeric
    assert_eq!(is_num(String::from("\u{00B2}")), 1); // ² is numeric
    // isdigit stays distinct: a fraction is numeric but not a digit.
    assert_eq!(is_dig(String::from("123")), 1);
    assert_eq!(is_dig(String::from("\u{00BD}")), 0);
}
"#;
    assert_rustc_runs("str_isnumeric", &rust, driver);
}

/// PMAT-695: `str.isascii()` → `(s).is_ascii()` (Bool, 0 args). True iff every
/// char is ASCII (U+0000..=U+007F); the empty string is True in both Python and
/// Rust, so — unlike the isdigit-family predicates — no empty guard. Cross-checked
/// vs python3.
#[test]
fn str_isascii() {
    let rust = xpile_transpile_to_rust("str_isascii.py");
    assert!(
        rust.contains("(s).is_ascii()"),
        "isascii should emit `(s).is_ascii()`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(asc("abc".to_string()), true);
    assert_eq!(asc("café".to_string()), false); // é is non-ASCII
    assert_eq!(asc("123!".to_string()), true);
    assert_eq!(asc_empty(), true); // empty is ASCII
    // isalnum (empty-guarded predicate) stays distinct.
    assert_eq!(still_alnum("ab12".to_string()), true);
    assert_eq!(still_alnum("a b".to_string()), false);
}
"#;
    assert_rustc_runs("str_isascii", &rust, driver);
}

/// PMAT-600: Python treats the C0 information separators FS/GS/RS/US
/// (U+001C..U+001F) as whitespace for `isspace()` and `strip`/`lstrip`/`rstrip`;
/// Rust's `char::is_whitespace()` / `trim()` excludes them. The predicate now
/// augments the Rust whitespace set with that range. Cross-checked vs python3.
#[test]
fn c0_whitespace() {
    let rust = xpile_transpile_to_rust("c0_whitespace.py");
    assert!(
        rust.contains("matches!(__c, '\\u{1c}'..='\\u{1f}')"),
        "C0-separator whitespace predicate:\n{rust}"
    );
    let driver = r#"
fn main() {
    // C0 separators FS/GS/RS/US are Python whitespace.
    assert!(is_ws("\u{1c}\u{1d}\u{1e}\u{1f}".to_string()));
    assert_eq!(stripped("\u{1c}abc\u{1f}".to_string()), "abc");
    assert_eq!(lstripped("\u{1c}ab".to_string()), "ab");
    assert_eq!(rstripped("ab\u{1f}".to_string()), "ab");
    // normal whitespace still works; non-whitespace is not isspace.
    assert!(is_ws("   ".to_string()));
    assert_eq!(stripped("  x  ".to_string()), "x");
    assert!(!is_ws("a b".to_string()));
}
"#;
    assert_rustc_runs("c0_whitespace", &rust, driver);
}

/// PMAT-502ah (Tranche 2): `s.capitalize()` → first char upper, rest lower
/// (empty → ""), matching Python.
#[test]
fn str_capitalize() {
    let rust = xpile_transpile_to_rust("str_capitalize.py");
    // PMAT-701: the lead is titlecased via the uppercase expansion (no
    // `char::to_titlecase` in std), so `let __ue: String = __f.to_uppercase()
    // .collect()` then keep the first char + lowercase the rest.
    assert!(
        rust.contains("let __ue: String = __f.to_uppercase().collect()"),
        "expected capitalize titlecase-via-expansion block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(cap(String::from("hELLO")), String::from("Hello"));
    assert_eq!(cap(String::from("world")), String::from("World"));
    assert_eq!(cap(String::from("a")), String::from("A"));
    assert_eq!(cap(String::from("")), String::from(""));
}
"#;
    assert_rustc_runs("str_capitalize", &rust, driver);
}

/// PMAT-502aj (Tranche 2): `s.title()` → title-case each word, matching
/// Python's exact word-boundary semantics.
#[test]
fn str_title() {
    let rust = xpile_transpile_to_rust("str_title.py");
    assert!(
        rust.contains("__c.is_alphabetic()") && rust.contains("__pa"),
        "expected title-case fold, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(t(String::from("hello world")), String::from("Hello World"));
    assert_eq!(t(String::from("HELLO")), String::from("Hello"));
    assert_eq!(t(String::from("123abc")), String::from("123Abc"));
    assert_eq!(t(String::from("it's")), String::from("It'S"));
    assert_eq!(t(String::from("")), String::from(""));
}
"#;
    assert_rustc_runs("str_title", &rust, driver);
}

/// PMAT-701: capitalize()/title() titlecase the lead char (not uppercase), so a
/// titlecase-EXPANDING scalar matches Python — `"ßeta".capitalize()` == "Sseta"
/// (ß→"Ss"), `"ﬂy".title()` == "Fly" (ﬂ→"Fl"). Derived from the uppercase
/// expansion (std has no `char::to_titlecase`). Cross-checked vs python3
/// (HUNT-V8 item V8-11).
#[test]
fn title_titlecase() {
    let rust = xpile_transpile_to_rust("title_titlecase.py");
    let driver = r#"
fn main() {
    assert_eq!(cap("ßeta".to_string()), "Sseta");   // ß titlecase = "Ss", not "SS"
    assert_eq!(cap("hello".to_string()), "Hello");
    assert_eq!(cap("ABC".to_string()), "Abc");
    assert_eq!(cap("".to_string()), "");
    assert_eq!(titl("ﬂy away".to_string()), "Fly Away"); // ﬂ titlecase = "Fl"
    assert_eq!(titl("hello world".to_string()), "Hello World");
    assert_eq!(titl("it's".to_string()), "It'S"); // regression: word-boundary
}
"#;
    assert_rustc_runs("title_titlecase", &rust, driver);
}

/// PMAT-502m (Tranche 2): numeric conversions `int(x)` / `float(x)` →
/// `((x) as i64)` (truncate toward zero) / `((x) as f64)`.
#[test]
fn num_cast_int_float() {
    let rust = xpile_transpile_to_rust("num_cast.py");
    // PMAT-586: `float(int)` stays `((n) as f64)`; `int(float)` now guards a
    // non-finite source (`__ic as i64` inside the guard block).
    assert!(
        rust.contains(") as f64)") && rust.contains("__ic as i64"),
        "expected numeric cast emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(to_float(7), 7.0);
    assert_eq!(to_int(2.7), 2);
    assert_eq!(to_int(-2.7), -2);
    assert_eq!(half(5), 2.5);
}
"#;
    assert_rustc_runs("num_cast", &rust, driver);
}

/// PMAT-502ak (Tranche 2): `round(x)` over a float → nearest int via
/// banker's rounding (`round_ties_even`), matching Python exactly.
#[test]
fn round_builtin() {
    let rust = xpile_transpile_to_rust("round_builtin.py");
    assert!(
        // PMAT-664: the emit now guards inf/nan + i64 range before the cast.
        rust.contains(".round_ties_even();") && rust.contains("__rti as i64"),
        "expected guarded round_ties_even emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    // banker's rounding: ties go to the even neighbor (matches Python)
    assert_eq!(r(2.5), 2);
    assert_eq!(r(3.5), 4);
    assert_eq!(r(0.5), 0);
    assert_eq!(r(1.5), 2);
    assert_eq!(r(-1.5), -2);
    assert_eq!(r(2.4), 2);
    assert_eq!(r(2.6), 3);
    // round(int) is the identity
    assert_eq!(r_int(7), 7);
}
"#;
    assert_rustc_runs("round_builtin", &rust, driver);
}

/// PMAT-664: `round(x)` of a non-finite float raises in Python (OverflowError
/// on inf, ValueError on nan) and returns a bigint for a huge magnitude; a bare
/// `as i64` saturated/garbage-cast silently. The emit now guards finiteness +
/// i64 range (mirrors the int()/math.floor guards), failing loud. Verified vs
/// python3 (which raises on inf/nan); the guard is asserted in the emitted Rust.
#[test]
fn round_nonfinite_guard() {
    let rust = xpile_transpile_to_rust("round_nonfinite.py");
    assert!(
        rust.contains("round() of a non-finite float") && rust.contains("round() out of i64 range"),
        "round should guard non-finite + out-of-range:\n{rust}"
    );
    // the normal-rounding path still compiles and runs
    let driver = r#"
fn main() {
    assert_eq!(round_normal(2.5), 2);
    assert_eq!(round_normal(3.5), 4);
    assert_eq!(round_normal(-2.5), -2);
}
"#;
    assert_rustc_runs("round_nonfinite", &rust, driver);
}

/// PMAT-502al (Tranche 2): `round(x, n)` → float rounded to n decimals via
/// banker's rounding after `10^n` scaling, matching Python's float-repr.
#[test]
fn round_digits() {
    let rust = xpile_transpile_to_rust("round_digits.py");
    assert!(
        rust.contains("format!(\"{:.1$}\", __rx, __rn as usize).parse::<f64>().unwrap()"),
        "expected round-to-digits block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(r2(3.14159, 2), 3.14);
    assert_eq!(r2(2.5, 0), 2.0);  // banker's: 2.5 -> 2.0
    assert_eq!(r2(1234.5, -1), 1230.0);  // negative ndigits
    assert_eq!(half_cent(3.14159), 3.14);
    // Python float-repr edge: 2.675 isn't exactly representable -> 2.67
    assert_eq!(half_cent(2.675), 2.67);
}
"#;
    assert_rustc_runs("round_digits", &rust, driver);
}

/// PMAT-612: `round(int, n)` → int. Previously emitted a bare `round(x, n)`
/// call → E0425 (cannot find function `round`). `n >= 0` is the identity (an
/// int has no fractional part); `n < 0` rounds to the nearest `10^(-n)` with
/// round-half-to-even (banker's rounding), done in `i128`. Cross-checked vs
/// python3: round(12345,-2)=12300, round(12350,-2)=12400 (tie→even),
/// round(12250,-2)=12200 (tie→even), round(-12350,-2)=-12400, round(7,-1)=10,
/// round(x,2)=x.
#[test]
fn round_int_digits() {
    let rust = xpile_transpile_to_rust("round_int_digits.py");
    // n < 0 / non-literal n → the i128 banker's-rounding block.
    assert!(
        rust.contains("__rv.div_euclid(__rp)") && rust.contains("as i128"),
        "expected round-int-to-digits block, got:\n{rust}"
    );
    // a non-negative literal ndigits is the identity: `keep` returns `x`
    // directly with no rounding block (and no bare `round(...)` → no E0425).
    assert!(
        rust.contains("pub fn keep(x: i64) -> i64 {\n    x\n}"),
        "round(int, n>=0) must be the identity (`keep` returns `x`):\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(to_hundred(12345), 12300);
    assert_eq!(to_hundred(12350), 12400); // tie -> even
    assert_eq!(to_hundred(12250), 12200); // tie -> even
    assert_eq!(keep(12345), 12345);       // n >= 0 identity
    assert_eq!(by_var(12345, -2), 12300);
    assert_eq!(by_var(-12350, -2), -12400);
    assert_eq!(by_var(98765, -3), 99000);
    assert_eq!(by_var(7, -1), 10);
    assert_eq!(by_var(42, 5), 42);        // runtime n >= 0 identity
}
"#;
    assert_rustc_runs("round_int_digits", &rust, driver);
}

/// PMAT-626: `str(list)`/`str(tuple)` and `print(list)`/`print(tuple)` were
/// rejected (str fell through → I64 mismatch; print declined). Both now reuse the
/// `build_list_repr`/`build_tuple_repr` desugar from f-string interpolation
/// (PMAT-623/624) → the Python repr. Found by hunt #8 (H8-3). Cross-checked vs
/// python3.
#[test]
fn str_print_list_tuple() {
    let rust = xpile_transpile_to_rust("str_print_list_tuple.py");
    assert!(
        rust.contains(".join(&(String::from(\", \"))[..])") && rust.contains("let __tp"),
        "str/print of a list & tuple should desugar to the Python repr:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(s_list(vec![1, 2, 3]), "[1, 2, 3]");
    assert_eq!(s_tuple((1, "a".to_string())), "(1, 'a')");
    assert_eq!(s_nested(vec![vec![1, 2], vec![3]]), "[[1, 2], [3]]");
    p_list(vec![1, 2, 3]); // prints [1, 2, 3]
    p_tuple((4, 5));       // prints (4, 5)
}
"#;
    assert_rustc_runs("str_print_list_tuple", &rust, driver);
}

/// PMAT-502ad (Tranche 2): `str(x)` over an int → `format!("{}", x)`
/// (unblocks `"prefix" + str(n)` concatenation).
#[test]
fn str_of_int() {
    let rust = xpile_transpile_to_rust("str_of_int.py");
    assert!(
        rust.contains("format!(\"{}\", n)"),
        "expected str(int) emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(show(42), String::from("count: 42"));
    assert_eq!(num_str(7), String::from("7"));
    assert_eq!(neg_str(5), String::from("-5"));
}
"#;
    assert_rustc_runs("str_of_int", &rust, driver);
}

/// PMAT-502ae (Tranche 2): `str(b)` over a bool → Python's `"True"`/`"False"`
/// via a desugar to `"True" if b else "False"`.
#[test]
fn str_of_bool() {
    let rust = xpile_transpile_to_rust("str_of_bool.py");
    assert!(
        rust.contains("String::from(\"True\")") && rust.contains("String::from(\"False\")"),
        "expected True/False string branches, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(flag_str(true), String::from("True"));
    assert_eq!(flag_str(false), String::from("False"));
    assert_eq!(cmp_str(1, 2), String::from("True"));
    assert_eq!(cmp_str(5, 2), String::from("False"));
    assert_eq!(labeled(true), String::from("flag=True"));
}
"#;
    assert_rustc_runs("str_of_bool", &rust, driver);
}

/// PMAT-502af (Tranche 2): `str(x)` over a float → Python-matching string
/// (whole numbers get a `.0` suffix; `nan`/`inf` handled).
#[test]
fn str_of_float() {
    let rust = xpile_transpile_to_rust("str_of_float.py");
    assert!(
        rust.contains("__sf.fract() == 0.0") && rust.contains("format!(\"{}.0\", __sf)"),
        "expected float str format block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(f_str(2.0), String::from("2.0"));
    assert_eq!(f_str(2.5), String::from("2.5"));
    assert_eq!(f_str(-1.5), String::from("-1.5"));
    assert_eq!(half_str(5), String::from("2.5"));
    assert_eq!(half_str(4), String::from("2.0"));
}
"#;
    assert_rustc_runs("str_of_float", &rust, driver);
}

/// PMAT-502n (Tranche 2): `divmod(a, b)` → the tuple `(a // b, a % b)`,
/// reusing the contract-checked floor-div + mod ops.
#[test]
fn divmod_builtin() {
    let rust = xpile_transpile_to_rust("divmod_builtin.py");
    // PMAT-538: floor-div + floor-mod (truncating op + sign correction), not
    // the euclidean ops (which diverge from Python for a negative divisor).
    assert!(
        rust.contains("checked_div") && rust.contains("__q - 1") && rust.contains("__r + __fb"),
        "expected floor-div + mod tuple emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(split_div(17, 5), (3, 2));
    assert_eq!(combine(17, 5), 302);
}
"#;
    assert_rustc_runs("divmod_builtin", &rust, driver);
}

/// PMAT-657: `divmod(float, float)` → `(a // b, a % b)` over the float ops.
/// The int path inlined the tuple, but a float arg fell through to an undefined
/// free `divmod(...)` call (E0425). The float floor-div/mod ops already match
/// CPython (sign follows the divisor). Cross-checked vs python3.
#[test]
fn divmod_float() {
    let rust = xpile_transpile_to_rust("divmod_float.py");
    assert!(
        !rust.contains("divmod("),
        "divmod(float) must not emit a bare divmod() call:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(dm_pos(), 301.5);          // (3.0, 1.5) — was E0425
    assert_eq!(dm_neg_dividend(), -399.5); // (-4.0, 0.5)
    assert_eq!(dm_neg_divisor(), -400.5);  // (-4.0, -0.5)
}
"#;
    assert_rustc_runs("divmod_float", &rust, driver);
}

/// PMAT-502o (Tranche 2): substring containment `sub in s` (str) →
/// `(s).contains(&(sub)[..])`; `not in` wraps it in `!`.
#[test]
fn str_contains_substring() {
    let rust = xpile_transpile_to_rust("str_contains.py");
    assert!(
        rust.contains(".contains(&(") && rust.contains(")[..])"),
        "expected substring-contains emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(has(String::from("hello"), String::from("ell")));
    assert!(!has(String::from("hello"), String::from("z")));
    assert!(lacks(String::from("hello"), String::from("z")));
    assert!(!lacks(String::from("hello"), String::from("ell")));
    assert!(has_literal(String::from("hello")));
    assert!(!has_literal(String::from("hi")));
}
"#;
    assert_rustc_runs("str_contains", &rust, driver);
}

/// PMAT-502an (Tranche 2): list membership `x in xs` / `x not in xs` →
/// `(xs).contains(&(x))` (and `!` for `not in`).
#[test]
fn list_membership() {
    let rust = xpile_transpile_to_rust("list_membership.py");
    assert!(
        rust.contains("(xs).contains(&(x))") && rust.contains("(!(xs).contains(&(x)))"),
        "expected list contains emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(has(vec![1, 2, 3], 2));
    assert!(!has(vec![1, 2, 3], 9));
    assert!(lacks(vec![1, 2, 3], 9));
    assert!(!lacks(vec![1, 2, 3], 2));
    assert!(has_str(vec![String::from("a"), String::from("b")], String::from("b")));
    assert!(!has_str(vec![String::from("a"), String::from("b")], String::from("z")));
}
"#;
    assert_rustc_runs("list_membership", &rust, driver);
}

/// PMAT-502p (Tranche 2): chained comparison `a OP b OP c` →
/// `(a OP b) && (b OP c)`.
#[test]
fn chained_compare() {
    let rust = xpile_transpile_to_rust("chained_compare.py");
    // PMAT-672: chained comparison evaluates each operand exactly once AND
    // short-circuits like Python — a right-nested form binds each shared operand
    // to a temp the moment it is reached, inside the `if` of the prior compare,
    // so the trailing operand is only evaluated when the earlier compare holds.
    assert!(
        rust.contains("if (lo <= __t1) { (__t1 <= hi) } else { false }"),
        "expected right-nested short-circuiting chained comparison, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(in_range(0, 5, 10));
    assert!(!in_range(0, 15, 10));
    assert!(in_range(0, 0, 10));
    assert!(in_range(0, 10, 10));
    assert!(strictly_increasing(1, 2, 3));
    assert!(!strictly_increasing(1, 2, 2));
    assert!(triple_eq(7, 7, 7));
    assert!(!triple_eq(7, 7, 8));
}
"#;
    assert_rustc_runs("chained_compare", &rust, driver);
}

/// PMAT-502q (Tranche 2): tuple constant-indexing `t[N]` → `(t).N.clone()`
/// (Rust tuple field access, not `[]` indexing).
#[test]
fn tuple_index() {
    let rust = xpile_transpile_to_rust("tuple_index.py");
    assert!(
        rust.contains("(t).0.clone()") && rust.contains("(t).1.clone()"),
        "expected tuple field-access emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(first((10, 20)), 10);
    assert_eq!(second((10, 20)), 20);
    assert_eq!(from_local(3, 4), 7);
}
"#;
    assert_rustc_runs("tuple_index", &rust, driver);
}

/// PMAT-671: `x in t` / `x not in t` over a fixed-arity tuple → a chained-OR of
/// equalities (`x == t.0 || …`). Was rejected ("unsupported comparison operator:
/// In") — Rust tuples have no `.contains`. Cross-checked vs python3.
#[test]
fn tuple_membership() {
    let rust = xpile_transpile_to_rust("tuple_membership.py");
    assert!(
        rust.contains("(x == (t).0.clone()) || (x == (t).1.clone())"),
        "x in tuple should be a chained-OR of equalities:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(contains((1, 2, 3), 2), 1);   // was rejected
    assert_eq!(contains((1, 2, 3), 9), 0);
    assert_eq!(not_contains((1, 2, 3), 9), 1);
    assert_eq!(not_contains((1, 2, 3), 2), 0);
    assert_eq!(contains_str(("a".to_string(), "b".to_string()), "b".to_string()), 1);
    assert_eq!(contains_str(("a".to_string(), "b".to_string()), "z".to_string()), 0);
    assert_eq!(list_in_regression(vec![1, 2], 2), 1);
}
"#;
    assert_rustc_runs("tuple_membership", &rust, driver);
}

/// PMAT-673: `a + b` over two tuples is CONCATENATION (`(1,2)+(3,4) == (1,2,3,4)`),
/// not numeric addition — Rust tuples have no `+`, so the old path fell to the
/// i64-arith lowering and mis-typed the result as I64 (the body-vs-annotation
/// mismatch rejected it). Lowered to a fresh `TupleLit` reading each field of the
/// operands via `TupleIndex`. Works for str tuples and chained `a + b + c`.
/// Cross-checked vs python3.
#[test]
fn tuple_concat() {
    let rust = xpile_transpile_to_rust("tuple_concat.py");
    assert!(
        rust.contains("((a).0.clone(), (a).1.clone(), (b).0.clone(), (b).1.clone())"),
        "tuple `+` should build a fresh tuple of all fields:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(concat((1, 2), (3, 4)), (1, 2, 3, 4));
    assert_eq!(
        concat_str(("a".to_string(), "b".to_string()), ("c".to_string(),)),
        ("a".to_string(), "b".to_string(), "c".to_string())
    );
    assert_eq!(concat_three((1, 2), (3,), (4, 5)), (1, 2, 3, 4, 5));
    assert_eq!(concat_local(10, 20), (10, 20, 30));
}
"#;
    assert_rustc_runs("tuple_concat", &rust, driver);
}

/// PMAT-670: a NEGATIVE constant tuple index `t[-k]` resolves at compile time to
/// the field access `t.(arity - k)` (Python from-the-end). The old path fell to
/// the list-style runtime wrap (`__lc.len()...`) → E0599 (a tuple has no `.len()`).
/// Works for heterogeneous tuples too. Cross-checked vs python3.
#[test]
fn tuple_negative_index() {
    let rust = xpile_transpile_to_rust("tuple_negative_index.py");
    assert!(
        // t[-1] of a 3-tuple → (t).2; t[-2] → (t).1; no `.len()` on a tuple
        rust.contains("(t).2.clone()")
            && rust.contains("(t).1.clone()")
            && !rust.contains("__lc.len()"),
        "negative tuple index should be a constant field access:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(last((10, 20, 30)), 30);       // was E0599
    assert_eq!(secondlast((10, 20, 30)), 20);
    assert_eq!(first_regression((10, 20, 30)), 10);
    assert_eq!(mixed_neg((7, "hi".to_string())), "hi");
}
"#;
    assert_rustc_runs("tuple_negative_index", &rust, driver);
}

/// PMAT-502s (Tranche 2): negative list index `xs[-k]` → `xs[len(xs) - k]`
/// (Python from-the-end indexing).
#[test]
fn neg_index() {
    let rust = xpile_transpile_to_rust("neg_index.py");
    assert!(
        rust.contains("xs.len() as i64)).checked_sub("),
        "expected len-relative negative index, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(last(vec![10, 20, 30]), 30);
    assert_eq!(second_last(vec![10, 20, 30]), 20);
    assert_eq!(sum_ends(vec![10, 20, 30]), 40);
}
"#;
    assert_rustc_runs("neg_index", &rust, driver);
}

/// PMAT-502t (Tranche 2): the reverse idiom `xs[::-1]` over a list →
/// a new reversed list (reuses `Expr::Reversed`).
#[test]
fn slice_reverse() {
    let rust = xpile_transpile_to_rust("slice_reverse.py");
    assert!(
        rust.contains(".clone(); __xv.reverse(); __xv }"),
        "expected reversed-list block, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(rev(vec![1, 2, 3]), vec![3, 2, 1]);
    assert_eq!(
        rev_strs(vec![String::from("a"), String::from("b"), String::from("c")]),
        vec![String::from("c"), String::from("b"), String::from("a")]
    );
}
"#;
    assert_rustc_runs("slice_reverse", &rust, driver);
}

/// PMAT-502u (Tranche 2): list query methods `xs.count(x)` / `xs.index(x)`
/// over an int list → `.iter().filter(…).count()` / `.iter().position(…)`.
#[test]
fn list_query() {
    let rust = xpile_transpile_to_rust("list_query.py");
    assert!(
        rust.contains(".iter().filter(|&&__e| __e ==")
            && rust.contains(".iter().position(|&__e| __e =="),
        "expected count/index emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(how_many(vec![1, 2, 2, 3, 2], 2), 3);
    assert_eq!(how_many(vec![1, 2, 3], 9), 0);
    assert_eq!(first_at(vec![10, 20, 30], 20), 1);
    assert_eq!(first_at(vec![10, 20, 30], 10), 0);
}
"#;
    assert_rustc_runs("list_query", &rust, driver);
}

/// PMAT-502v (Tranche 2): dict views `d.keys()` / `d.values()` →
/// `.keys()/.values().cloned().collect::<Vec<_>>()` (compose w/ sorted/sum).
#[test]
fn dict_views() {
    let rust = xpile_transpile_to_rust("dict_views.py");
    assert!(
        rust.contains(".keys().cloned().collect::<Vec<_>>()")
            && rust.contains(".values().cloned().collect::<Vec<_>>()"),
        "expected dict view emission, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    let d: HashMap<i64, i64> = [(1, 10), (2, 20), (3, 30)].into_iter().collect();
    assert_eq!(sorted_keys(d.clone()), vec![1, 2, 3]);
    assert_eq!(sorted_values(d.clone()), vec![10, 20, 30]);
    assert_eq!(total_values(d.clone()), 60);
}
"#;
    assert_rustc_runs("dict_views", &rust, driver);
}

/// PMAT-502w (Tranche 2): ctx-aware `len(x)` over context-dependent
/// expressions (dict views, sorted) — previously a hard error.
#[test]
fn len_ctx() {
    let rust = xpile_transpile_to_rust("len_ctx.py");
    assert!(
        rust.contains(".cloned().collect::<Vec<_>>().len() as i64"),
        "expected len over a dict view, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    let d: HashMap<i64, i64> = [(1, 10), (2, 20), (3, 30)].into_iter().collect();
    assert_eq!(num_keys(d.clone()), 3);
    assert_eq!(num_values(d.clone()), 3);
    assert_eq!(len_sorted(vec![5, 1, 3, 2]), 4);
}
"#;
    assert_rustc_runs("len_ctx", &rust, driver);
}

/// PMAT-502x (Tranche 2): `d.items()` → a `Vec` of `(k, v)` tuples
/// (composes with `sorted`/`len`).
#[test]
fn dict_items() {
    let rust = xpile_transpile_to_rust("dict_items.py");
    assert!(
        rust.contains(".iter().map(|(__k, __v)| (__k.clone(), __v.clone())).collect::<Vec<_>>()"),
        "expected dict items emission, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    let d: HashMap<i64, i64> = [(3, 30), (1, 10), (2, 20)].into_iter().collect();
    assert_eq!(sorted_items(d.clone()), vec![(1, 10), (2, 20), (3, 30)]);
    assert_eq!(num_items(d.clone()), 3);
}
"#;
    assert_rustc_runs("dict_items", &rust, driver);
}

/// PMAT-502y (Tranche 2): `for k, v in d.items()` — iterate dict pairs,
/// destructuring each `(k, v)` (`PairIterKind::Pairs`).
#[test]
fn for_items() {
    let rust = xpile_transpile_to_rust("for_items.py");
    assert!(
        rust.contains("for (k, v) in "),
        "expected destructuring pair loop, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    let d: HashMap<i64, i64> = [(1, 10), (2, 20), (3, 30)].into_iter().collect();
    assert_eq!(sum_kv(d.clone()), 66);
    assert_eq!(sum_values(d.clone()), 60);
}
"#;
    assert_rustc_runs("for_items", &rust, driver);
}

/// PMAT-502r (Tranche 2): open-ended slices `xs[a:]` / `xs[:b]` / `xs[:]`
/// (list + str) → half-open / full Rust ranges.
#[test]
fn open_slice() {
    let rust = xpile_transpile_to_rust("open_slice.py");
    // PMAT-539: open-ended slices now use the resolve+clamp block form. An
    // absent low bound defaults to 0, an absent high bound to `__n` (the len);
    // present bounds clamp. The runtime values are checked by the driver below.
    assert!(
        rust.contains("let __lo_i = 0;")
            && rust.contains("let __hi_i = __n;")
            && rust.contains("__sl[__lo..__hi].to_vec()"),
        "expected open-ended slice emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(head(vec![1, 2, 3, 4], 2), vec![1, 2]);
    assert_eq!(tail(vec![1, 2, 3, 4], 2), vec![3, 4]);
    assert_eq!(copy_all(vec![1, 2, 3]), vec![1, 2, 3]);
    assert_eq!(str_prefix(String::from("hello"), 3), String::from("hel"));
    assert_eq!(str_suffix(String::from("hello"), 3), String::from("lo"));
}
"#;
    assert_rustc_runs("open_slice", &rust, driver);
}

/// PMAT-502h (Tranche 2): 1-arg `min(xs)`/`max(xs)` over a `list[float]` →
/// a fold (`fold(f64::INFINITY, f64::min)` / `fold(f64::NEG_INFINITY, f64::max)`).
#[test]
fn list_minmax_float() {
    let rust = xpile_transpile_to_rust("list_minmax_float.py");
    // PMAT-608: float min/max use a first-arg-wins reduce (empty → ValueError).
    assert!(
        rust.contains(".reduce(|__a, __b| if __b < __a { __b } else { __a })")
            && rust.contains(".reduce(|__a, __b| if __b > __a { __b } else { __a })")
            && rust.contains("of an empty sequence"),
        "expected float min/max first-arg-wins reduce, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(lowest(vec![3.5, 1.5, 2.5]), 1.5);
    assert_eq!(highest(vec![3.5, 1.5, 2.5]), 3.5);
}
"#;
    assert_rustc_runs("list_minmax_float", &rust, driver);
}

/// PMAT-608: max()/min() over a float sequence that is EMPTY must raise
/// ValueError (Python), not return the old ±∞ fold sentinel. The float reduce
/// now yields Option → an empty sequence panics (≈ ValueError) instead of
/// silently returning ±inf. Cross-checked vs python3: non-empty computes the
/// extremum (3.0 / 1.0); empty fails loud (caught via catch_unwind).
#[test]
fn max_empty_float_gen() {
    let rust = xpile_transpile_to_rust("max_empty_float_gen.py");
    assert!(
        !rust.contains("f64::NEG_INFINITY")
            && !rust.contains("f64::INFINITY")
            && rust.contains("of an empty sequence"),
        "empty float max/min must fail loud, not return ±inf:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(max_pos_ratio(vec![2, 4, 6]), 3.0);
    assert_eq!(min_pos_ratio(vec![2, 4, 6]), 1.0);
    // all filtered out -> empty -> ValueError (panic), not -inf/+inf.
    assert!(std::panic::catch_unwind(|| max_pos_ratio(vec![-1, -2])).is_err());
    assert!(std::panic::catch_unwind(|| min_pos_ratio(vec![-1, -2])).is_err());
}
"#;
    assert_rustc_runs("max_empty_float_gen", &rust, driver);
}

/// PMAT-503a (Tranche 2, exceptions sub-slice 1): a `raise Exc("msg")`
/// guard clause → `panic!("{}", <message>)`. The non-raising path returns
/// normally; the raising path panics (caught via `catch_unwind`).
#[test]
fn raise_guard_panics() {
    let rust = xpile_transpile_to_rust("raise_guard.py");
    assert!(
        rust.contains("panic!(\"{}\","),
        "expected panic emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    // Silence the default panic hook so the expected panics stay quiet.
    std::panic::set_hook(Box::new(|_| {}));
    // Non-raising paths return normally.
    assert_eq!(checked_div(10, 2), 5);
    assert_eq!(must_be_positive(7), 7);
    // Guard conditions fire → panic, caught here.
    assert!(std::panic::catch_unwind(|| checked_div(1, 0)).is_err());
    assert!(std::panic::catch_unwind(|| must_be_positive(0)).is_err());
}
"#;
    assert_rustc_runs("raise_guard", &rust, driver);
}

/// PMAT-631 (typed-exceptions sub-slice 1): `raise E(msg)` now emits a typed
/// panic payload `xpile: <Type>: <msg>` (matching the builtin convention), so
/// distinct exception types are distinguishable (was an identical `"msg"`
/// payload). Groundwork for typed `except` matching. Found by hunt #9 (H9-5..10).
#[test]
fn raise_typed_payload() {
    let rust = xpile_transpile_to_rust("raise_typed_payload.py");
    assert!(
        rust.contains("xpile: ValueError: ") && rust.contains("xpile: KeyError: "),
        "raise should prefix the panic payload with the exception type:\n{rust}"
    );
    let driver = r#"
fn payload(r: std::thread::Result<i64>) -> String {
    match r {
        Ok(_) => String::from("ok"),
        Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_else(|| String::from("?")),
    }
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    // raised exceptions carry their type in the payload, and differ by type.
    assert_eq!(payload(std::panic::catch_unwind(|| raise_value(-1))), "xpile: ValueError: neg");
    assert_eq!(payload(std::panic::catch_unwind(|| raise_key(-1))), "xpile: KeyError: missing");
    // the non-raising path returns normally.
    assert_eq!(raise_value(5), 5);
}
"#;
    assert_rustc_runs("raise_typed_payload", &rust, driver);
}

/// PMAT-502ao (Tranche 2): `assert cond, msg` → `assert!(cond, "{}", msg)`;
/// the bare `assert cond` form is unchanged.
#[test]
fn assert_msg() {
    let rust = xpile_transpile_to_rust("assert_msg.py");
    assert!(
        rust.contains("assert!((x > 0i64), \"{}\", String::from(\"x must be positive\"))")
            && rust.contains("assert!((x > 0i64));"),
        "expected assert with + without message, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    // Passing paths return normally.
    assert_eq!(checked(5), 5);
    assert_eq!(bare(5), 5);
    // Failing assert (with message) panics, caught here.
    assert!(std::panic::catch_unwind(|| checked(0)).is_err());
    assert!(std::panic::catch_unwind(|| bare(-1)).is_err());
}
"#;
    assert_rustc_runs("assert_msg", &rust, driver);
}

/// PMAT-502ap (Tranche 2): in-place list mutators `xs.sort()` /
/// `xs.reverse()` / `xs.clear()` → the matching `Vec` method. A float
/// list sorts via `.sort_by(partial_cmp)` (no `Ord` on `f64`).
#[test]
fn list_mutate() {
    let rust = xpile_transpile_to_rust("list_mutate.py");
    assert!(rust.contains("xs.sort();"), "int sort:\n{rust}");
    assert!(rust.contains("xs.reverse();"), "reverse:\n{rust}");
    assert!(
        // PMAT-616: NaN-safe comparator (Python doesn't raise on NaN).
        rust.contains("xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));"),
        "float sort:\n{rust}"
    );
    assert!(rust.contains("xs.clear();"), "clear:\n{rust}");
    // Receivers must be `mut`.
    assert!(
        rust.contains("first_sorted(mut xs: Vec<i64>)"),
        "mut receiver:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(first_sorted(vec![3, 1, 2]), 1);
    assert_eq!(first_reversed(vec![3, 1, 2]), 2);
    assert_eq!(first_fsorted(vec![3.0, 1.5, 2.0]), 1.5);
    assert_eq!(cleared_len(vec![1, 2, 3]), 0);
}
"#;
    assert_rustc_runs("list_mutate", &rust, driver);
}

/// PMAT-502aq (Tranche 2): in-place list concatenation `xs.extend(ys)` →
/// `xs.extend((<ys>).iter().cloned());`.
#[test]
fn list_extend() {
    let rust = xpile_transpile_to_rust("list_extend.py");
    assert!(
        rust.contains("xs.extend((ys).iter().cloned());"),
        "extend(name):\n{rust}"
    );
    assert!(
        rust.contains("xs.extend((vec![4i64, 5i64]).iter().cloned());"),
        "extend(literal):\n{rust}"
    );
    assert!(
        rust.contains("grow(mut xs: Vec<i64>"),
        "mut receiver:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(grow(vec![1, 2], vec![3, 4, 5]), 5);
    assert_eq!(grow_lit(vec![1, 2, 3]), 4);
    assert_eq!(sum_after(vec![1, 2], vec![3, 4]), 10);
}
"#;
    assert_rustc_runs("list_extend", &rust, driver);
}

/// PMAT-660: `list.extend()` accepts any iterable. extend(range(n)) emitted an
/// undefined `range(...)` (E0425), extend((a,b,c)) called `.iter()` on a tuple
/// (E0599), and extend(xs) (self) hit a borrow conflict (E0502). The arg now
/// materializes (range→Vec, set→list, tuple→list) and self-extend clones first.
/// Cross-checked vs python3.
#[test]
fn list_extend_iterables() {
    let rust = xpile_transpile_to_rust("list_extend_iterables.py");
    assert!(
        !rust.contains("(range(") && !rust.contains("((7i64, 8i64, 9i64)).iter()"),
        "extend arg should be materialized, not a bare range/tuple:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(extend_range(vec![1, 2]), 6);   // was E0425
    assert_eq!(extend_tuple(vec![1, 2]), 27);  // was E0599
    assert_eq!(extend_self(vec![1, 2, 3]), 12); // was E0502
    let s: std::collections::HashSet<i64> = [10, 20].into_iter().collect();
    assert_eq!(extend_set(vec![1, 2], s), 33);  // bonus: set extend
    assert_eq!(extend_list_regression(vec![1, 2], vec![3, 4]), 10);
}
"#;
    assert_rustc_runs("list_extend_iterables", &rust, driver);
}

/// PMAT-502ar (Tranche 2): positional list insertion `xs.insert(i, x)` →
/// a CPython-clamping insert block (PMAT-590).
#[test]
fn list_insert() {
    let rust = xpile_transpile_to_rust("list_insert.py");
    // PMAT-590: insert now emits a clamp block (CPython `ins1` parity).
    assert!(
        rust.contains("let mut __i = (1i64);") && rust.contains("xs.insert(__i as usize, x);"),
        "insert(var):\n{rust}"
    );
    assert!(
        rust.contains("let mut __i = (0i64);") && rust.contains("xs.insert(__i as usize, 99i64);"),
        "insert(front):\n{rust}"
    );
    assert!(
        rust.contains("ins_mid(mut xs: Vec<i64>"),
        "mut receiver:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(ins_mid(vec![10, 20, 30], 99), 99);
    assert_eq!(ins_front(vec![1, 2, 3]), 99);
    assert_eq!(ins_grows(vec![1, 2, 3]), 4);
}
"#;
    assert_rustc_runs("list_insert", &rust, driver);
}

/// PMAT-590: `list.insert(i, x)` must clamp out-of-range / negative indices
/// to CPython `ins1` semantics instead of panicking like raw `Vec::insert`.
/// Cross-checked vs python3:
///   xs=[1,2,3]; insert(100,88) -> [1,2,3,88]      xs[3]==88
///   xs=[1,2,3]; insert(-1,77)  -> [1,2,77,3]      xs[2]==77
///   xs=[1,2,3]; insert(-100,5) -> [5,1,2,3]       xs[0]==5
///   xs=[1,2,3]; insert(3,9)    -> [1,2,3,9]       xs[3]==9
#[test]
fn list_insert_clamp() {
    let rust = xpile_transpile_to_rust("list_insert_clamp.py");
    // The clamp block normalizes negatives and caps high indices at len.
    assert!(
        rust.contains(
            "if __i < 0 { __i += __n; if __i < 0 { __i = 0; } } if __i > __n { __i = __n; }"
        ),
        "clamp block:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(ins_oob(vec![1, 2, 3]), 88);
    assert_eq!(ins_neg(vec![1, 2, 3]), 77);
    assert_eq!(ins_neg_far(vec![1, 2, 3]), 5);
    assert_eq!(ins_at_len(vec![1, 2, 3]), 9);
}
"#;
    assert_rustc_runs("list_insert_clamp", &rust, driver);
}

/// PMAT-502as (Tranche 2): list pop (expression form) `xs.pop()` →
/// `(xs).pop().unwrap()` and `xs.pop(i)` → `(xs).remove((i) as usize)`.
/// Covers a param receiver, a local receiver (mutability pre-pass), and
/// pop inside arithmetic.
#[test]
fn list_pop() {
    let rust = xpile_transpile_to_rust("list_pop.py");
    assert!(
        rust.contains("(xs).pop().expect(\"xpile: IndexError: pop from empty list\")"),
        "pop last:\n{rust}"
    );
    assert!(
        rust.contains("(xs).remove((0i64) as usize)"),
        "pop at index:\n{rust}"
    );
    // Param receiver marked mut.
    assert!(
        rust.contains("take_last(mut xs: Vec<i64>"),
        "mut param:\n{rust}"
    );
    // Local receiver marked mut by the count_pop_receivers pre-pass.
    assert!(
        rust.contains("let mut xs: Vec<i64> = vec!"),
        "mut local:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(take_last(vec![1, 2, 3]), 3);
    assert_eq!(take_at(vec![10, 20, 30]), 10);
    assert_eq!(local_pop(), 5);
    assert_eq!(sum_two(vec![1, 2, 3, 4]), 7);
}
"#;
    assert_rustc_runs("list_pop", &rust, driver);
}

/// PMAT-715: `xs[i].pop()` must mutate the inner container IN PLACE — the read
/// path cloned `xs[i]` and popped the clone, so `xs[i]` kept its length
/// (silent-wrong: the popped value was right, the side-effect was lost). Now an
/// l-value pop over a `mut` base, with negative-index wrapping. Cross-checked vs
/// python3 (HUNT-V9 V9-2).
#[test]
fn nested_subscript_pop() {
    let rust = xpile_transpile_to_rust("nested_subscript_pop.py");
    assert!(
        rust.contains("pop_inner(mut xs:")
            && rust.contains(
                "xs[__pi as usize].pop().expect(\"xpile: IndexError: pop from empty list\")"
            ),
        "nested pop should be an l-value over a mut base:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pop_inner(vec![vec![1, 2, 3], vec![4]]), 32); // pop 3; 3*10 + len 2
    assert_eq!(pop_neg(vec![vec![1], vec![5, 6, 7]]), 9);    // pop 7; 7 + len 2
    assert_eq!(plain_pop(vec![10, 20, 30]), 32);             // pop 30; 30 + len 2
}
"#;
    assert_rustc_runs("nested_subscript_pop", &rust, driver);
}

/// PMAT-609: `xs.pop(i)` with a RUNTIME negative index removes from the end
/// (Python `i + len`); a bare `(i) as usize` wrapped a negative i to usize::MAX
/// → panic. The runtime index is now normalized (`if __pidx < 0 { len + __pidx }`).
/// Literal pops keep their existing lowering. Cross-checked vs python3:
/// pop_at([10,20,30,40], -1)=40, -2=30, 0=10, 2=30.
#[test]
fn pop_runtime_negative() {
    let rust = xpile_transpile_to_rust("pop_runtime_negative.py");
    assert!(
        rust.contains("let __pidx: i64 = i;") && rust.contains("if (__pidx < 0i64)"),
        "runtime pop index must be normalized:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pop_at(vec![10, 20, 30, 40], -1), 40);
    assert_eq!(pop_at(vec![10, 20, 30, 40], -2), 30);
    assert_eq!(pop_at(vec![10, 20, 30, 40], 0), 10);
    assert_eq!(pop_at(vec![10, 20, 30, 40], 2), 30);
}
"#;
    assert_rustc_runs("pop_runtime_negative", &rust, driver);
}

/// PMAT-502at (Tranche 2): item deletion `del coll[key]` — list →
/// `coll.remove((k) as usize);`, dict → `coll.remove(&(k));`.
#[test]
fn del_item() {
    let rust = xpile_transpile_to_rust("del_item.py");
    // PMAT-712: a plain-index list del now normalizes a runtime-negative index
    // (bind + `if __di < 0 { len + __di }`) before `remove`.
    assert!(
        rust.contains("let __di = (i) as i64;")
            && rust.contains("if __di < 0")
            && rust.contains("xs.remove(__di as usize);"),
        "list del (var) normalized:\n{rust}"
    );
    assert!(
        rust.contains("let __di = (0i64) as i64;"),
        "list del (literal) normalized:\n{rust}"
    );
    // PMAT-709: dict del now asserts the key was present (KeyError parity).
    assert!(
        rust.contains("assert!(d.remove(&(k)).is_some(), \"xpile: KeyError"),
        "dict del KeyError assert:\n{rust}"
    );
    assert!(
        rust.contains("drop_at(mut xs: Vec<i64>"),
        "mut list param:\n{rust}"
    );
    assert!(
        rust.contains("mut d: std::collections::HashMap"),
        "mut dict param:\n{rust}"
    );
    // Local receiver marked mut by the walk_counts Delete arm.
    assert!(
        rust.contains("let mut xs: Vec<i64> = vec!"),
        "mut local:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(drop_at(vec![1, 2, 3], 1), 2);
    assert_eq!(drop_first(vec![10, 20, 30]), 20);
    let mut d = std::collections::HashMap::new();
    d.insert("a".to_string(), 1);
    d.insert("b".to_string(), 2);
    assert_eq!(drop_key(d, "a".to_string()), 1);
    assert_eq!(drop_local(), 3);
}
"#;
    assert_rustc_runs("del_item", &rust, driver);
}

/// PMAT-712: `del xs[i]` with a runtime-negative index emitted `xs.remove((i) as
/// usize)` → the negative underflowed to a huge index → panic. It now wraps like
/// Python (`del xs[-1]` removes the last element). Cross-checked vs python3
/// (HUNT-V9 V9-11).
#[test]
fn del_list_runtime_negidx() {
    let rust = xpile_transpile_to_rust("del_list_runtime_negidx.py");
    let driver = r#"
fn main() {
    assert_eq!(del_idx(vec![10, 20, 30], -1), 30); // removes 30 -> [10,20] -> 10+20
    assert_eq!(del_idx(vec![10, 20, 30], -2), 40); // removes 20 -> [10,30] -> 10+30
    assert_eq!(del_pos(vec![10, 20, 30], 1), 2);   // positive index unchanged
    assert_eq!(del_pos(vec![10, 20, 30], 0), 2);
}
"#;
    assert_rustc_runs("del_list_runtime_negidx", &rust, driver);
}

/// PMAT-709: `del d[k]` on an absent key raises KeyError in Python — xpile's bare
/// `d.remove(&k)` discarded the `Option` and silently succeeded (silent-wrong).
/// Now it asserts the key was present. Cross-checked vs python3 (HUNT-V9 V9-1).
#[test]
fn del_dict_keyerror() {
    let rust = xpile_transpile_to_rust("del_dict_keyerror.py");
    let driver = r#"
fn main() {
    let mut a = std::collections::HashMap::new();
    a.insert("a".to_string(), 1);
    a.insert("b".to_string(), 2);
    assert_eq!(drop_present(a), 1);
    let mut b = std::collections::HashMap::new();
    b.insert("x".to_string(), 1);
    b.insert("y".to_string(), 2);
    assert_eq!(drop_var(b, "x".to_string()), 1);
    // missing key -> Python KeyError -> panic
    let mut c = std::collections::HashMap::new();
    c.insert("a".to_string(), 1);
    assert!(std::panic::catch_unwind(move || drop_var(c, "z".to_string())).is_err());
}
"#;
    assert_rustc_runs("del_dict_keyerror", &rust, driver);
}

/// PMAT-502au (Tranche 2): dict pop (expression form) `d.pop(k)` →
/// `(d).remove(&(k)).unwrap()` and `d.pop(k, default)` →
/// `(d).remove(&(k)).unwrap_or(default)`. Covers param + local receivers.
#[test]
fn dict_pop() {
    let rust = xpile_transpile_to_rust("dict_pop.py");
    assert!(
        rust.contains(
            "(d).remove(&(k)).unwrap_or_else(|| panic!(\"xpile: KeyError: key not found\"))"
        ),
        "pop (no default):\n{rust}"
    );
    assert!(
        rust.contains("(d).remove(&(k)).unwrap_or(0i64)"),
        "pop (default):\n{rust}"
    );
    assert!(
        rust.contains("take(mut d: std::collections::HashMap"),
        "mut param:\n{rust}"
    );
    // Local receiver marked mut by the count_pop_receivers pre-pass.
    assert!(
        rust.contains("let mut d: std::collections::HashMap"),
        "mut local:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert("a".to_string(), 5);
    assert_eq!(take(d, "a".to_string()), 5);
    let d2 = std::collections::HashMap::new();
    assert_eq!(take_or(d2, "missing".to_string()), 0);
    assert_eq!(take_local(), 2);
}
"#;
    assert_rustc_runs("dict_pop", &rust, driver);
}

/// PMAT-615: augmented set assignment `s -= / |= / &= / ^= other`. Previously
/// fell through to a numeric/bitwise BinOp — `HashSet::checked_sub` (E0599) for
/// `-=` and owned-value `|`/`&`/`^` on `HashSet` (E0369) for the others:
/// transpile succeeded but emitted invalid Rust. Now reuses the binop `SetOp`
/// path. Cross-checked vs python3 ({1,2,3} op {3,4,5}).
#[test]
fn augmented_set_ops() {
    let rust = xpile_transpile_to_rust("augmented_set_ops.py");
    // the augmented forms now reassign via the set-op methods, not BinOp.
    assert!(
        rust.contains(".difference(") && rust.contains(".symmetric_difference("),
        "augmented set ops should reuse the SetOp path:\n{rust}"
    );
    assert!(
        !rust.contains("checked_sub") && !rust.contains("(s | "),
        "augmented set ops must not emit int/bitwise BinOps:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(aug_sub(vec![1, 2, 3], vec![3, 4, 5]), vec![1, 2]);
    assert_eq!(aug_or(vec![1, 2, 3], vec![3, 4, 5]), vec![1, 2, 3, 4, 5]);
    assert_eq!(aug_and(vec![1, 2, 3], vec![3, 4, 5]), vec![3]);
    assert_eq!(aug_xor(vec![1, 2, 3], vec![3, 4, 5]), vec![1, 2, 4, 5]);
}
"#;
    assert_rustc_runs("augmented_set_ops", &rust, driver);
}

/// PMAT-502av (Tranche 2): set element removal `s.remove(x)` →
/// `assert!(s.remove(&(x)), "…");` (KeyError if absent) and
/// `s.discard(x)` → `s.remove(&(x));` (silent no-op).
#[test]
fn set_remove() {
    let rust = xpile_transpile_to_rust("set_remove.py");
    assert!(
        rust.contains("assert!(s.remove(&(x)), \"xpile: KeyError: set.remove(x): x not in set\");"),
        "remove (KeyError):\n{rust}"
    );
    assert!(rust.contains("s.remove(&(x));"), "discard (no-op):\n{rust}");
    assert!(
        rust.contains("drop(mut s: std::collections::HashSet"),
        "mut param:\n{rust}"
    );
    assert!(
        rust.contains("let mut s: std::collections::HashSet"),
        "mut local:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let mut s = std::collections::HashSet::new();
    s.insert(1); s.insert(2); s.insert(3);
    assert_eq!(drop(s, 2), 2);
    let mut s2 = std::collections::HashSet::new();
    s2.insert(1);
    assert_eq!(disc(s2, 99), 1);
    assert_eq!(drop_local(), 2);
    // remove of an absent element panics (KeyError).
    let mut s3 = std::collections::HashSet::new();
    s3.insert(1);
    assert!(std::panic::catch_unwind(move || drop(s3, 99)).is_err());
}
"#;
    assert_rustc_runs("set_remove", &rust, driver);
}

/// PMAT-598: a mutable empty `set()` infers its element type from the later
/// `.add(...)`. It previously defaulted to `HashSet<i64>`, so `s = set();
/// s.add(Coord(..))` was an i64-vs-struct mismatch (E0308). The element-type
/// annotation is now suppressed so rustc infers from the insert. Cross-checked
/// vs python3: each builder dedupes to 2.
#[test]
fn empty_set_add() {
    let rust = xpile_transpile_to_rust("empty_set_add.py");
    // No `HashSet<i64>` annotation pins the empty set; bare new() infers.
    assert!(
        rust.contains("let mut s = std::collections::HashSet::new();")
            && !rust.contains("let mut s: std::collections::HashSet<i64>"),
        "empty set() must infer its element type:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(count_unique_coords(), 2);
    assert_eq!(count_unique_words(), 2);
    assert_eq!(count_ints(), 2);
}
"#;
    assert_rustc_runs("empty_set_add", &rust, driver);
}

/// PMAT-502aw (Tranche 2): str padding `s.rjust(w)` →
/// `format!("{:>1$}", s, (w) as usize)` and `s.ljust(w)` →
/// `format!("{:<1$}", s, (w) as usize)`. Rust format width is a minimum,
/// so a longer string is returned unchanged (matching Python).
#[test]
fn str_just() {
    let rust = xpile_transpile_to_rust("str_just.py");
    // PMAT-666: the width is now clamped with `.max(0)` (negative → unchanged).
    assert!(
        rust.contains("format!(\"{:>1$}\", s, (w).max(0) as usize)"),
        "rjust:\n{rust}"
    );
    assert!(
        rust.contains("format!(\"{:<1$}\", s, (w).max(0) as usize)"),
        "ljust:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pad_r("hi".to_string(), 5), "   hi");
    assert_eq!(pad_l("hi".to_string(), 5), "hi   ");
    // No truncation when already longer (Python returns unchanged).
    assert_eq!(pad_r("hello".to_string(), 3), "hello");
    assert_eq!(lit_pad(), "   hi");
}
"#;
    assert_rustc_runs("str_just", &rust, driver);
}

/// PMAT-666: zfill/center/ljust/rjust with a NEGATIVE width return the string
/// unchanged (Python treats width <= len as no padding). The bare `as usize`
/// cast underflowed a negative width to a huge value → capacity-overflow panic;
/// the width is now clamped with `.max(0)`. Cross-checked vs python3.
#[test]
fn str_width_negative() {
    let rust = xpile_transpile_to_rust("str_width_negative.py");
    assert!(
        rust.contains(".max(0) as usize"),
        "negative width should be clamped with .max(0):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(zfill_neg("ab".to_string()), "ab");   // was a panic
    assert_eq!(center_neg("ab".to_string()), "ab");
    assert_eq!(ljust_neg("ab".to_string()), "ab");
    assert_eq!(rjust_neg("ab".to_string()), "ab");
    assert_eq!(ljust_fill_neg("ab".to_string()), "ab");
    assert_eq!(zfill_pos_regression("ab".to_string()), "000ab");
}
"#;
    assert_rustc_runs("str_width_negative", &rust, driver);
}

/// PMAT-502ax (Tranche 2): dict get-or-insert `d.setdefault(k, default)`
/// → `(d).entry((k).clone()).or_insert(default).clone()`. Present key
/// returns the existing value; absent key inserts the default. Covers
/// param + local receivers.
#[test]
fn dict_setdefault() {
    let rust = xpile_transpile_to_rust("dict_setdefault.py");
    assert!(
        rust.contains("(d).entry((k).clone()).or_insert(0i64).clone()"),
        "setdefault:\n{rust}"
    );
    assert!(
        rust.contains("getset(mut d: std::collections::HashMap"),
        "mut param:\n{rust}"
    );
    // Local receiver marked mut by the count_pop_receivers pre-pass.
    assert!(
        rust.contains("let mut d: std::collections::HashMap"),
        "mut local:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert("a".to_string(), 7);
    // present key → existing value, no insert.
    assert_eq!(getset_present(d, "a".to_string()), 7);
    // absent key → inserts default and returns it.
    let d2 = std::collections::HashMap::new();
    assert_eq!(getset(d2, "x".to_string()), 0);
    assert_eq!(local_setdefault(), 6);
}
"#;
    assert_rustc_runs("dict_setdefault", &rust, driver);
}

/// PMAT-502ay (Tranche 2): filtered list comprehension
/// `[elem for v in xs if cond]` → the `if` wraps the accumulator append
/// inside the desugared for-loop.
#[test]
fn list_comp_filter() {
    let rust = xpile_transpile_to_rust("list_comp_filter.py");
    // The filter becomes an `if` guarding the push.
    assert!(
        rust.contains("if (x > 0i64) {") && rust.contains("__xpile_comp.push(x);"),
        "filter guards push:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(positives(vec![-1, 2, -3, 4]), vec![2, 4]);
    assert_eq!(doubled_positives(vec![-1, 2, 3]), vec![4, 6]);
    assert_eq!(assign_form(vec![1, 6, 7, 2]), 2);
}
"#;
    assert_rustc_runs("list_comp_filter", &rust, driver);
}

/// PMAT-713: a comprehension filter `[x for x in xs if x]` and `assert x` use
/// Python truthiness (like `if x:`, which already works) — they were rejected
/// ("filter/assert not Bool"). Coerced via `truthy_condition`. Cross-checked vs
/// python3 (HUNT-V9 items V9-16 + V9-17).
#[test]
fn truthiness_comp_assert() {
    let rust = xpile_transpile_to_rust("truthiness_comp_assert.py");
    let driver = r#"
fn main() {
    assert_eq!(nonzero_ints(vec![0, 1, 2, 0, 3]), vec![1, 2, 3]);
    assert_eq!(
        nonempty_strs(vec!["a".into(), "".into(), "b".into()]),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(assert_int(6), 12);
    assert_eq!(assert_list(vec![1, 2]), 2);
    // assert on a falsy value panics (AssertionError)
    assert!(std::panic::catch_unwind(|| assert_int(0)).is_err());
}
"#;
    assert_rustc_runs("truthiness_comp_assert", &rust, driver);
}

/// PMAT-502az (Tranche 2): filtered dict + set comprehensions
/// `{k: v for x in xs if cond}` / `{e for x in xs if cond}` — the `if`
/// guards the desugared insert/add.
#[test]
fn dict_set_comp_filter() {
    let rust = xpile_transpile_to_rust("dict_set_comp_filter.py");
    // Both desugarings guard the accumulator with an `if`.
    assert!(
        rust.contains("if (x > 0i64) {") && rust.contains("__xpile_comp.insert("),
        "filter guards insert/add:\n{rust}"
    );
    let driver = r#"
fn main() {
    let m = pos_map(vec![-1, 2, 3]);
    assert_eq!(m.get(&2), Some(&4));
    assert_eq!(m.get(&3), Some(&9));
    assert_eq!(m.len(), 2);
    let s = pos_set(vec![-1, 2, 2, 3]);
    assert!(s.contains(&2) && s.contains(&3) && !s.contains(&-1));
    assert_eq!(s.len(), 2);
    assert_eq!(dc_assign(vec![1, 6, 7, 2]), 2);
}
"#;
    assert_rustc_runs("dict_set_comp_filter", &rust, driver);
}

/// PMAT-502ba (Tranche 2): list comprehension over `range(...)` →
/// a counter `let mut x = start; while (x < stop) { …push…; x += step; }`
/// (mirroring the for-over-range desugaring), with optional `if` filter.
#[test]
fn list_comp_range() {
    let rust = xpile_transpile_to_rust("list_comp_range.py");
    // PMAT-635: range comp desugars to a counter while-loop (not a ForEach),
    // using a fresh synthetic counter `__forc{N}` so the comprehension variable
    // does not leak into the enclosing scope.
    assert!(
        rust.contains("let mut __forc0: i64 = 0i64;") && rust.contains("while (__forc0 < n) {"),
        "range counter loop:\n{rust}"
    );
    assert!(
        rust.contains("let mut __forc0: i64 = 1i64;"),
        "range(1, n) start:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(squares(4), vec![0, 1, 4, 9]);
    assert_eq!(odd_squares(4), vec![1, 4, 9]);
    assert_eq!(from_one(5), vec![1, 2, 3, 4]);
    assert_eq!(assign_form(3), 3);
}
"#;
    assert_rustc_runs("list_comp_range", &rust, driver);
}

/// PMAT-635: **correctness** — a range-comprehension variable is scoped to the
/// comprehension and must not leak into / clobber an enclosing binding (Python
/// semantics). Previously `[i for i in range(3)]` desugared to a function-scope
/// `let mut i` counter that shadowed an outer `i` (param/local). The fix renames
/// the counter to a fresh `__forc{N}`. Covers list/dict/set comps; cross-checked
/// vs python3.
#[test]
fn comp_var_no_leak() {
    let rust = xpile_transpile_to_rust("comp_var_no_leak.py");
    // The counter is synthetic; the user variable `i` is never the loop counter.
    assert!(
        rust.contains("__forc0"),
        "expected synthetic comp counter:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(list_no_leak(99), 102); // param i (99) preserved + len 3
    assert_eq!(dict_no_leak(99), 102);
    assert_eq!(set_no_leak(99), 102);
    assert_eq!(comp_values(), 34);
}
"#;
    assert_rustc_runs("comp_var_no_leak", &rust, driver);
}

/// PMAT-502bb (Tranche 2): in-place dict merge `a.update(b)` →
/// `a.extend((b).iter().map(|(k, v)| (k.clone(), v.clone())));` — merges
/// `b` into `a` (overwriting), without consuming `b`.
#[test]
fn dict_update() {
    let rust = xpile_transpile_to_rust("dict_update.py");
    assert!(
        rust.contains("a.extend((b).iter().map(|(__k, __v)| (__k.clone(), __v.clone())));"),
        "update emission:\n{rust}"
    );
    assert!(
        rust.contains("merge(mut a: std::collections::HashMap"),
        "mut param:\n{rust}"
    );
    assert!(
        rust.contains("let mut a: std::collections::HashMap"),
        "mut local:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut a = std::collections::HashMap::new();
    a.insert("x".to_string(), 1);
    a.insert("y".to_string(), 2);
    let mut b = std::collections::HashMap::new();
    b.insert("y".to_string(), 20);
    b.insert("z".to_string(), 3);
    assert_eq!(merge(a, b.clone()), 3);
    assert_eq!(b.len(), 2); // b not consumed
    assert_eq!(merge_local(b), 2);
}
"#;
    assert_rustc_runs("dict_update", &rust, driver);
}

/// PMAT-502bc (Tranche 2): general slice step `xs[a:b:c]` over a list
/// (positive literal `c`) → `<c>[<range>].iter().step_by(c).cloned()
/// .collect::<Vec<_>>()`.
#[test]
fn slice_step() {
    let rust = xpile_transpile_to_rust("slice_step.py");
    // PMAT-539: the slice range now resolves+clamps via the block form; the
    // step suffix is unchanged. Runtime values checked by the driver below.
    assert!(
        rust.contains("__sl[__lo..__hi].iter().step_by(2).cloned().collect::<Vec<_>>()"),
        "xs[::2]:\n{rust}"
    );
    assert!(
        rust.contains("__sl[__lo..__hi].iter().step_by(3).cloned().collect::<Vec<_>>()"),
        "xs[1:8:3]:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(every_other(vec![0, 1, 2, 3, 4, 5]), vec![0, 2, 4]);
    assert_eq!(bounded_step(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), vec![1, 4, 7]);
    assert_eq!(from_one_step(vec![0, 1, 2, 3, 4, 5]), vec![1, 3, 5]);
}
"#;
    assert_rustc_runs("slice_step", &rust, driver);
}

/// PMAT-502bd (Tranche 2): dict + set comprehensions over `range(...)` →
/// a counter while-loop around the dict/set accumulator (same shape as
/// the list-comp range branch), with optional `if` filter.
#[test]
fn dict_set_comp_range() {
    let rust = xpile_transpile_to_rust("dict_set_comp_range.py");
    // PMAT-635: range comp desugars to a counter while-loop (not a ForEach),
    // using a fresh synthetic counter `__forc{N}` so the comprehension variable
    // does not leak into the enclosing scope.
    assert!(
        rust.contains("let mut __forc0: i64 = 0i64;") && rust.contains("while (__forc0 < n) {"),
        "range counter loop:\n{rust}"
    );
    assert!(
        rust.contains("let mut __forc0: i64 = 2i64;"),
        "range(2, n) start:\n{rust}"
    );
    let driver = r#"
fn main() {
    let m = sq_map(4);
    assert_eq!(m.get(&3), Some(&9));
    assert_eq!(m.len(), 4);
    let s = even_set(4);
    assert!(s.contains(&1) && s.contains(&3) && !s.contains(&0));
    assert_eq!(s.len(), 3);
    assert_eq!(from_two(5), 3);
}
"#;
    assert_rustc_runs("dict_set_comp_range", &rust, driver);
}

/// PMAT-504 (Tranche 2): first-class closure — `f = lambda y: <body>`
/// binds a Rust closure `let f = |y: i64| { <body> };`, callable as
/// `f(arg)` (the return type is recorded so the call site types right).
#[test]
fn closure_local() {
    let rust = xpile_transpile_to_rust("closure_local.py");
    assert!(
        rust.contains("let inc = |y: i64| {") && rust.contains("inc(inc(x))"),
        "closure bind + nested call:\n{rust}"
    );
    // A Bool-returning closure makes the function return `bool`.
    assert!(
        rust.contains("is_positive(x: i64) -> bool") && rust.contains("let pos = |y: i64| {"),
        "bool closure:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(apply_twice(5), 7);
    assert_eq!(is_positive(3), true);
    assert_eq!(is_positive(-1), false);
    assert_eq!(scale(4), 12);
}
"#;
    assert_rustc_runs("closure_local", &rust, driver);
}

/// PMAT-504b (Tranche 2): multi-parameter + nullary closures — `f =
/// lambda x, y: …` → `let f = |x: i64, y: i64| { … };`; `lambda: 42` → `||`.
#[test]
fn closure_multiparam() {
    let rust = xpile_transpile_to_rust("closure_multiparam.py");
    assert!(
        rust.contains("let f = |x: i64, y: i64| {"),
        "two-param closure:\n{rust}"
    );
    assert!(rust.contains("let g = || {"), "nullary closure:\n{rust}");
    assert!(
        rust.contains("let h = |x: i64, y: i64, z: i64| {"),
        "three-param closure:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(add(3, 4), 7);
    assert_eq!(nullary(), 42);
    assert_eq!(combine(2, 3, 5), 11);
}
"#;
    assert_rustc_runs("closure_multiparam", &rust, driver);
}

/// PMAT-502be (Tranche 2): `bool(x)` truthiness cast — a pure desugar to
/// `!= 0`: int → `x != 0`, str/list/dict/set → `len(x) != 0`, bool →
/// identity.
#[test]
fn bool_cast() {
    let rust = xpile_transpile_to_rust("bool_cast.py");
    assert!(rust.contains("(x != 0i64)"), "int cast:\n{rust}");
    assert!(
        rust.contains("((s.len() as i64) != 0i64)"),
        "str cast:\n{rust}"
    );
    assert!(
        rust.contains("((xs.len() as i64) != 0i64)"),
        "list cast:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(from_int(5), true);
    assert_eq!(from_int(0), false);
    assert_eq!(from_int(-3), true);
    assert_eq!(from_str("hi".to_string()), true);
    assert_eq!(from_str("".to_string()), false);
    assert_eq!(from_list(vec![1]), true);
    assert_eq!(from_list(vec![]), false);
    assert_eq!(idempotent(true), true);
}
"#;
    assert_rustc_runs("bool_cast", &rust, driver);
}

/// PMAT-502bf (Tranche 2): `int(s)` / `float(s)` string parsing →
/// `(s).trim().parse::<i64|f64>().expect(…)` (trims like Python; panics
/// on bad input, matching `ValueError`). The numeric `int(float)` cast
/// still uses `as`.
#[test]
fn str_parse() {
    let rust = xpile_transpile_to_rust("str_parse.py");
    // PMAT-610: int(s) now validates + strips PEP 515 underscores before parse.
    assert!(
        rust.contains(".replace('_', \"\").parse::<i64>().expect("),
        "int(s):\n{rust}"
    );
    // PMAT-611: float(s) now validates + strips PEP 515 underscores before parse.
    assert!(
        rust.contains(".replace('_', \"\").parse::<f64>().expect("),
        "float(s):\n{rust}"
    );
    // PMAT-586: `int(float_x)` now guards a non-finite source (`__ic as i64`).
    assert!(
        rust.contains("__ic as i64"),
        "numeric int(float) cast:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(to_int("42".to_string()), 42);
    assert_eq!(to_int("  -7  ".to_string()), -7);
    assert_eq!(to_float("3.14".to_string()), 3.14);
    assert_eq!(add_parsed("10".to_string(), "20".to_string()), 30);
    assert_eq!(numeric_still(2.9), 2);
    assert!(std::panic::catch_unwind(|| to_int("abc".to_string())).is_err());
}
"#;
    assert_rustc_runs("str_parse", &rust, driver);
}

/// PMAT-610: `int(s)` accepts PEP 515 underscore digit separators
/// (`int("1_000") == 1000`), which Rust's `parse::<i64>()` rejects. xpile
/// validates Python's between-digits rule, strips, then parses; invalid
/// placements still raise (≈ ValueError). Cross-checked vs python3:
/// 1_000→1000, 1_000_000→1000000, -2_5→-25, 42→42; `_1`/`1_`/`1__0` reject.
#[test]
fn int_str_underscore() {
    let rust = xpile_transpile_to_rust("int_str_underscore.py");
    assert!(
        rust.contains(".replace('_', \"\").parse::<i64>()")
            && rust.contains(r#"__pb.contains("__")"#),
        "int(s) must validate + strip underscores:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(parse("1_000".to_string()), 1000);
    assert_eq!(parse("1_000_000".to_string()), 1000000);
    assert_eq!(parse("-2_5".to_string()), -25);
    assert_eq!(parse("42".to_string()), 42);
    // a string-literal operand is a temporary String: the block must keep it
    // alive (no E0716 temporary-dropped-while-borrowed).
    assert_eq!(parse_literal(), 1234);
    // invalid underscore placements raise (≈ Python ValueError).
    assert!(std::panic::catch_unwind(|| parse("_1".to_string())).is_err());
    assert!(std::panic::catch_unwind(|| parse("1_".to_string())).is_err());
    assert!(std::panic::catch_unwind(|| parse("1__0".to_string())).is_err());
}
"#;
    assert_rustc_runs("int_str_underscore", &rust, driver);
}

/// PMAT-611: `float(s)` accepts PEP 515 underscore digit separators
/// (`float("1_000.5") == 1000.5`), which Rust's `parse::<f64>()` rejects.
/// xpile validates Python's between-digits rule (covering the fractional and
/// exponent parts), strips, then parses; invalid placements still raise
/// (≈ ValueError). Cross-checked vs python3: 1_000.5→1000.5,
/// 1_000_000.0→1000000.0, 1.5e1_0→1.5e10; `1_.5`/`1.5_`/`_1.0`/`1_e5` reject.
#[test]
fn float_str_underscore() {
    let rust = xpile_transpile_to_rust("float_str_underscore.py");
    assert!(
        rust.contains(".replace('_', \"\").parse::<f64>()")
            && rust.contains("__pe[__k - 1].is_ascii_digit()"),
        "float(s) must validate + strip underscores:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(parse("1_000.5".to_string()), 1000.5);
    assert_eq!(parse("1_000_000.0".to_string()), 1000000.0);
    assert_eq!(parse("1.5e1_0".to_string()), 1.5e10);
    assert_eq!(parse("3.14".to_string()), 3.14);
    // a string-literal operand is a temporary String: the block must keep it
    // alive (no E0716 temporary-dropped-while-borrowed).
    assert_eq!(parse_literal(), 1234.5);
    // invalid underscore placements raise (≈ Python ValueError).
    assert!(std::panic::catch_unwind(|| parse("1_.5".to_string())).is_err());
    assert!(std::panic::catch_unwind(|| parse("1.5_".to_string())).is_err());
    assert!(std::panic::catch_unwind(|| parse("_1.0".to_string())).is_err());
    assert!(std::panic::catch_unwind(|| parse("1_e5".to_string())).is_err());
}
"#;
    assert_rustc_runs("float_str_underscore", &rust, driver);
}

/// PMAT-502bg (Tranche 2): list concatenation `xs + ys` →
/// `(xs).iter().chain((ys).iter()).cloned().collect::<Vec<_>>()` (a fresh
/// `Vec`, consuming neither operand).
#[test]
fn list_concat() {
    let rust = xpile_transpile_to_rust("list_concat.py");
    assert!(
        rust.contains("(a).iter().chain((b).iter()).cloned().collect::<Vec<_>>()"),
        "list concat:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(cat(vec![1, 2], vec![3, 4]), vec![1, 2, 3, 4]);
    assert_eq!(cat_lit(), vec![1, 2, 3, 4]);
    assert_eq!(cat_len(vec![1, 2], vec![3, 4, 5]), 5);
    // operands not consumed:
    let a = vec![1, 2];
    let _ = cat(a.clone(), vec![9]);
    assert_eq!(a.len(), 2);
}
"#;
    assert_rustc_runs("list_concat", &rust, driver);
}

/// PMAT-502bh (Tranche 2): `"<fmt>".format(args…)` with sequential `{}`
/// placeholders → `format!("<fmt>", args…)`.
#[test]
fn str_format() {
    let rust = xpile_transpile_to_rust("str_format.py");
    assert!(rust.contains("format!(\"val={}\", x)"), "one arg:\n{rust}");
    assert!(
        rust.contains("format!(\"{} + {} done\", a, b)"),
        "two args:\n{rust}"
    );
    assert!(
        rust.contains("format!(\"{{literal}} {}\", x)"),
        "escaped braces:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(one(42), "val=42");
    assert_eq!(two(3, 7), "3 + 7 done");
    assert_eq!(with_str("x".to_string(), 5), "x: 5");
    assert_eq!(escaped(9), "{literal} 9");
}
"#;
    assert_rustc_runs("str_format", &rust, driver);
}

/// PMAT-714: `"{}".format(x)` over a float/bool was rejected ("needs a spec"),
/// though `f"{x}"`/`str(x)` produce the right Python repr. The no-spec `{}` now
/// wraps a float/bool arg in the f-string display conversion (float → `3.0`/`3.14`,
/// bool → `True`/`False`) when referenced once. Cross-checked vs python3 (V9-26).
#[test]
fn str_format_float_default() {
    let rust = xpile_transpile_to_rust("str_format_float_default.py");
    let driver = r#"
fn main() {
    assert_eq!(show_float(3.14), "val=3.14");
    assert_eq!(show_bool(true), "flag=True");      // Python bool repr
    assert_eq!(whole_float(3.0), "3.0");           // not Rust's "3"
    assert_eq!(mixed(1, 2.5, "hi".to_string()), "1 2.5 hi");
}
"#;
    assert_rustc_runs("str_format_float_default", &rust, driver);
}

/// PMAT-502bi (Tranche 2): `s.index(sub)` → byte index of the first
/// match, panicking (ValueError) when absent (like `.find` but no `-1`).
#[test]
fn str_index() {
    let rust = xpile_transpile_to_rust("str_index.py");
    assert!(
        rust.contains(".find(&(String::from(\"b\"))[..]).map(|__b| __s[..__b].chars().count() as i64).expect(")
            && rust.contains("substring not found"),
        "str.index emission:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(find_b("abc".to_string()), 1);
    assert_eq!(find_lit(), 2);
    assert!(std::panic::catch_unwind(|| find_b("xyz".to_string())).is_err());
}
"#;
    assert_rustc_runs("str_index", &rust, driver);
}

/// PMAT-502bj (Tranche 2): module-level int/bool/float constants →
/// `const NAME: TY = VALUE;`, referenceable from function bodies.
#[test]
fn module_const() {
    let rust = xpile_transpile_to_rust("module_const.py");
    assert!(
        rust.contains("const MAX: i64 = 100i64;"),
        "int const:\n{rust}"
    );
    assert!(
        rust.contains("const NEG: i64 = -5i64;"),
        "negative const:\n{rust}"
    );
    assert!(
        rust.contains("const FLAG: bool = true;"),
        "bool const:\n{rust}"
    );
    assert!(
        rust.contains("const RATIO: f64 = 2.5f64;"),
        "float const:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(get_max(), 100);
    assert_eq!(use_neg(10), 5);
    assert_eq!(use_flag(), true);
    assert_eq!(scaled(4.0), 10.0);
}
"#;
    assert_rustc_runs("module_const", &rust, driver);
}

/// PMAT-502bk (Tranche 2): `continue` / `break` in loops → Rust
/// `continue;` / `break;`. (`continue` in a `range(...)` for-loop is
/// rejected separately; `break` there is fine.)
#[test]
fn loop_control() {
    let rust = xpile_transpile_to_rust("loop_control.py");
    assert!(rust.contains("continue;"), "continue:\n{rust}");
    assert!(rust.contains("break;"), "break:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(sum_pos(vec![1, -2, 3, -4, 5]), 9);
    assert_eq!(first_neg(vec![1, 2, -3, 4]), -3);
    assert_eq!(first_neg(vec![1, 2, 3]), 0);
    assert_eq!(sum_below_three(10), 3);
}
"#;
    assert_rustc_runs("loop_control", &rust, driver);
}

/// PMAT-502bl (Tranche 2): void functions (`-> None`) → `fn … -> () { …; () }`.
/// (Arg mutation isn't observed by the caller under value semantics — the
/// `&mut` aliasing path is a v0.3.0 sub-track — but the function compiles
/// and its observable effects, e.g. an `assert`, work.)
#[test]
fn void_fn() {
    let rust = xpile_transpile_to_rust("void_fn.py");
    assert!(
        rust.contains("pub fn check_pos(x: i64) -> () {") && rust.contains("assert!((x > 0i64));"),
        "void assert fn:\n{rust}"
    );
    assert!(
        rust.contains("pub fn put(mut d: std::collections::HashMap"),
        "void mutator fn (mut receiver):\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    check_pos(5); // returns () — no panic
    let mut d = std::collections::HashMap::new();
    d.insert("seed".to_string(), 0);
    put(d, "a".to_string(), 1); // compiles + runs
    // the assert-void's effect is observable: bad input panics.
    assert!(std::panic::catch_unwind(|| check_pos(-1)).is_err());
}
"#;
    assert_rustc_runs("void_fn", &rust, driver);
}

/// PMAT-502bm (Tranche 2): early returns / guard clauses + a terminal
/// `if/elif/else` whose branches all return (→ `Expr::IfExpr`).
#[test]
fn early_return() {
    let rust = xpile_transpile_to_rust("early_return.py");
    // Terminal if/elif/else becomes a nested if-expression trailing return.
    assert!(
        rust.contains("if (x > 0i64) { 1i64 } else if (x < 0i64)"),
        "terminal if/elif/else:\n{rust}"
    );
    // Guard clause emits an early `return` then a trailing expr.
    assert!(
        rust.contains("return 0i64;"),
        "guard-clause early return:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sign(5), 1);
    assert_eq!(sign(-3), -1);
    assert_eq!(sign(0), 0);
    assert_eq!(abs_val(-4), 4);
    assert_eq!(abs_val(7), 7);
    assert_eq!(guard(-2), 0);
    assert_eq!(guard(5), 6);
}
"#;
    assert_rustc_runs("early_return", &rust, driver);
}

/// PMAT-502bn (Tranche 2): `pass` (no-op) → no emitted statements (an
/// empty `if`/branch, or a `pass`-only void function body).
#[test]
fn pass_stmt() {
    let rust = xpile_transpile_to_rust("pass_stmt.py");
    // pass-only void function: empty body returning `()`.
    assert!(rust.contains("pub fn noop() -> () {"), "void noop:\n{rust}");
    // pass in an `if` body → an empty `if { }`.
    assert!(
        rust.contains("if (x < 0i64) {\n    }"),
        "empty if from pass:\n{rust}"
    );
    let driver = r#"
fn main() {
    noop();
    assert_eq!(guard_pass(-2), -1);
    assert_eq!(guard_pass(5), 6);
    assert_eq!(skip_first(vec![0, 1, 0, 2, 3]), 6);
}
"#;
    assert_rustc_runs("pass_stmt", &rust, driver);
}

/// PMAT-502bo (Tranche 2): negative float literals `-3.14` fold to a
/// single `LitFloat(-3.14)` → `-3.14f64` (not the i64-only `checked_neg`).
#[test]
fn neg_float() {
    let rust = xpile_transpile_to_rust("neg_float.py");
    assert!(rust.contains("-3.14f64"), "neg float literal:\n{rust}");
    assert!(rust.contains("-1.5f64"), "neg float in arith:\n{rust}");
    assert!(
        !rust.contains("checked_neg"),
        "float negation must not use checked_neg:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pi(), -3.14);
    assert_eq!(offset(2.0), 0.5);
}
"#;
    assert_rustc_runs("neg_float", &rust, driver);
}

/// PMAT-502bp (Tranche 2): negation of a float *variable* `-x` (x: float)
/// → `x * -1.0` (a `FloatBinOp`, not the i64-only `checked_neg`).
/// PMAT-650: changed from `0.0 - x` so the sign of a zero is preserved.
#[test]
fn neg_float_var() {
    let rust = xpile_transpile_to_rust("neg_float_var.py");
    assert!(rust.contains("(x * -1f64)"), "float-var negation:\n{rust}");
    assert!(
        !rust.contains("checked_neg"),
        "float negation must not use checked_neg:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(neg(3.5), -3.5);
    assert_eq!(neg(-2.0), 2.0);
    assert_eq!(diff(1.5, 4.0), 2.5);
}
"#;
    assert_rustc_runs("neg_float_var", &rust, driver);
}

/// PMAT-650: unary negation of a float variable preserves the sign of a zero.
/// `-x` (x == 0.0) must print "-0.0" like Python — the old `0.0 - x` emit gave
/// `+0.0` (`0.0 - 0.0 == +0.0` in IEEE-754); `x * -1.0` flips the sign bit.
/// Cross-checked vs python3.
#[test]
fn neg_float_signed_zero() {
    let rust = xpile_transpile_to_rust("neg_float_signed_zero.py");
    let driver = r#"
fn main() {
    assert_eq!(neg_zero_str(), "-0.0");   // was "0.0" before the fix
    assert_eq!(neg_nonzero_str(), "-3.5");
    assert_eq!(double_neg(), "0.0");      // -(-0.0) flips back to +0.0
    assert_eq!(neg_in_expr(), "0.0");     // -0.0 + 0.0 == +0.0
}
"#;
    assert_rustc_runs("neg_float_signed_zero", &rust, driver);
}

/// PMAT-629: `s *= n` (str) and `xs *= n` (list) are repetition, not numeric
/// multiplication — `s *= n` emitted `String::checked_mul` (E0599) and `xs *= n`
/// was rejected. Both now route to `Expr::Repeat` (like `s * n` / `xs * n`); an
/// int `x *= 3` stays numeric. Found by hunt #9 (H9-11). Cross-checked vs python3.
#[test]
fn mul_assign_repeat() {
    let rust = xpile_transpile_to_rust("mul_assign_repeat.py");
    assert!(
        // str/list *= use repeat; int *= stays checked_mul.
        rust.contains("s = (s).repeat(")
            && rust.contains("flat_map(|_| __rep.iter().cloned())")
            && rust.contains("x = (x).checked_mul(3i64)"),
        "str/list *= should repeat; int *= should stay numeric:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(rep_str("ab".to_string(), 3), "ababab");
    assert_eq!(rep_list(vec![1, 2], 3), vec![1, 2, 1, 2, 1, 2]);
    assert_eq!(int_mul(4), 12);
}
"#;
    assert_rustc_runs("mul_assign_repeat", &rust, driver);
}

/// PMAT-502bq (Tranche 2): augmented assignment over a float `x += y`
/// (and `-= *= /=`) → plain infix `FloatBinOp`, not the i64-only
/// `checked_*` path. `/=` (true division) is supported in aug position.
#[test]
fn aug_assign_float() {
    let rust = xpile_transpile_to_rust("aug_assign_float.py");
    assert!(rust.contains("x = (x + y)"), "float += :\n{rust}");
    // PMAT-581: float `/=` guards the zero divisor (divisor binds to `__fz`).
    assert!(rust.contains("(x) / __fz"), "float /= :\n{rust}");
    assert!(
        !rust.contains("checked_add"),
        "float aug-assign must not use checked_*:\n{rust}"
    );
    let driver = r#"
fn main() {
    // python3: x=3.0; x+=2 ->5; x-=1 ->4; x*=2 ->8; x/=4 ->2.0
    assert!((accum(3.0, 2.0) - 2.0).abs() < 1e-9);
    assert!((scale_first(vec![2.5, 9.0], 4.0) - 10.0).abs() < 1e-9);
}
"#;
    assert_rustc_runs("aug_assign_float", &rust, driver);
}

/// PMAT-614: Python float `//` is CPython `float_divmod` (fmod-based), not
/// `(a / b).floor()`. The naive floor over-rounds when `a/b` lands just below an
/// integer in float repr (`1.0 // 0.1` == 9.0, not 10.0) and mishandles infinite
/// operands (`inf // 2` == nan, `-5.0 // inf` == -1.0). Cross-checked vs python3.
#[test]
fn float_floordiv_semantics() {
    let rust = xpile_transpile_to_rust("float_floordiv_semantics.py");
    assert!(
        rust.contains("let __fm = __fa % __fz;") && rust.contains("__fd - __ffl > 0.5"),
        "float // should be the fmod-based divmod:\n{rust}"
    );
    let driver = r#"
fn main() {
    let inf = f64::INFINITY;
    // the textbook float-repr cases (Python floors "down"):
    assert!((floordiv(1.0, 0.1) - 9.0).abs() < 1e-9);
    assert!((steps(2.0, 0.1) - 19.0).abs() < 1e-9);
    assert!((floordiv(5.5, 1.1) - 4.0).abs() < 1e-9);
    // ordinary + signed cases:
    assert!((floordiv(10.0, 3.0) - 3.0).abs() < 1e-9);
    assert!((floordiv(-7.5, 2.0) - (-4.0)).abs() < 1e-9);
    // infinite operands (CPython float_divmod):
    assert!(floordiv(inf, 2.0).is_nan());
    assert!((floordiv(-5.0, inf) - (-1.0)).abs() < 1e-9);
    assert!((floordiv(5.0, -inf) - (-1.0)).abs() < 1e-9);
}
"#;
    assert_rustc_runs("float_floordiv_semantics", &rust, driver);
}

/// PMAT-651: float floor-division preserves the sign of a zero result. CPython's
/// `float_divmod` returns `copysign(0.0, a/b)` when the snapped quotient is zero,
/// so `-0.0 // 1.0` is `-0.0`; the fmod-based formula used to `floor(0.0)` →
/// `+0.0`, dropping the sign. Cross-checked vs python3 (compares the printed
/// repr, the only place `-0.0` vs `+0.0` is observable).
#[test]
fn floordiv_signed_zero() {
    let rust = xpile_transpile_to_rust("floordiv_signed_zero.py");
    assert!(
        rust.contains("(0.0_f64).copysign(__fa / __fz)"),
        "zero-quotient floor-div should copysign the sign of a/b:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(negzero_pos(), "-0.0");  // was "0.0" before the fix
    assert_eq!(negzero_var(), "-0.0");  // was "0.0" before the fix
    assert_eq!(pos_neg(), "-0.0");      // regression guard (already correct)
    assert_eq!(poszero(), "0.0");
    assert_eq!(normal_neg(), "-4.0");
    assert_eq!(small_pos(), "0.0");     // +0.0 zero-quotient
}
"#;
    assert_rustc_runs("floordiv_signed_zero", &rust, driver);
}

/// PMAT-614 (was PMAT-502br): float floor-division `a // b` is CPython
/// `float_divmod` (fmod-based), not `(a / b).floor()`; modulo `a % b` is
/// CPython `float_rem` (Python floor semantics, `%` follows the divisor's sign).
#[test]
fn float_floordiv_mod() {
    let rust = xpile_transpile_to_rust("float_floordiv_mod.py");
    // PMAT-614: float `//` is the fmod-based divmod (binds both operands).
    assert!(
        rust.contains("let __fm = __fa % __fz;") && rust.contains("__fd - __ffl > 0.5"),
        "float // :\n{rust}"
    );
    // PMAT-591: float `%` is CPython `float_rem` (fmod + sign-adjust).
    assert!(
        rust.contains("let __r = __fn % __fz;") && rust.contains("0.0_f64.copysign(__fz)"),
        "float % :\n{rust}"
    );
    let driver = r#"
fn main() {
    // python3: 7//2=3, -7//2=-4, 7%3=1, -7%3=2, 7%-3=-2
    assert!((fd(7.0, 2.0) - 3.0).abs() < 1e-9);
    assert!((fd(-7.0, 2.0) - (-4.0)).abs() < 1e-9);
    // PMAT-614: the common float-repr case Python floors "down" (1.0//0.1==9).
    assert!((fd(1.0, 0.1) - 9.0).abs() < 1e-9);
    assert!((fd(2.0, 0.1) - 19.0).abs() < 1e-9);
    // infinite operands: inf//2 is nan, -5//inf is -1 (CPython float_divmod).
    assert!(fd(f64::INFINITY, 2.0).is_nan());
    assert!((fd(-5.0, f64::INFINITY) - (-1.0)).abs() < 1e-9);
    assert!((fmod(7.0, 3.0) - 1.0).abs() < 1e-9);
    assert!((fmod(-7.0, 3.0) - 2.0).abs() < 1e-9);
    assert!((fmod(7.0, -3.0) - (-2.0)).abs() < 1e-9);
    assert!((wrap(13.0, 5.0) - 3.0).abs() < 1e-9);
}
"#;
    assert_rustc_runs("float_floordiv_mod", &rust, driver);
}

/// PMAT-591: float `%` must match CPython `float_rem` bit-exactly — fmod
/// (Rust `%`) + sign-adjust toward the divisor + `copysign(0.0, divisor)`
/// on a zero remainder. The prior floor formula diverged in the last ULP
/// (~60% of non-power-of-two divisors) and lost the divisor-signed zero.
/// Exact-equality (not tolerance) vs python3 repr:
///   1.0%0.3=0.10000000000000003   0.7%0.2=0.09999999999999992
///   2.5%0.7=0.40000000000000013   100.0%0.7=0.6000000000000063
///   -1.0%0.3=0.19999999999999996  1.0%-0.3=-0.19999999999999996
///   4.0%-2.0=-0.0  -4.0%2.0=+0.0  6.0%3.0=+0.0
#[test]
fn float_mod_fmod() {
    let rust = xpile_transpile_to_rust("float_mod_fmod.py");
    assert!(
        rust.contains("let __r = __fn % __fz;") && rust.contains("0.0_f64.copysign(__fz)"),
        "float % fmod shape:\n{rust}"
    );
    let driver = r#"
fn main() {
    // last-ULP parity (exact f64 equality vs python3 repr)
    assert_eq!(rem(1.0, 0.3), 0.10000000000000003);
    assert_eq!(rem(0.7, 0.2), 0.09999999999999992);
    assert_eq!(rem(2.5, 0.7), 0.40000000000000013);
    assert_eq!(rem(100.0, 0.7), 0.6000000000000063);
    // mixed-sign: result takes the divisor's sign
    assert_eq!(rem(-1.0, 0.3), 0.19999999999999996);
    assert_eq!(rem(1.0, -0.3), -0.19999999999999996);
    // signed zero: a zero remainder carries the divisor's sign
    let z1 = rem(4.0, -2.0);
    assert!(z1 == 0.0 && z1.is_sign_negative());
    let z2 = rem(-4.0, 2.0);
    assert!(z2 == 0.0 && !z2.is_sign_negative());
    let z3 = rem(6.0, 3.0);
    assert!(z3 == 0.0 && !z3.is_sign_negative());
}
"#;
    assert_rustc_runs("float_mod_fmod", &rust, driver);
}

/// PMAT-502bs (Tranche 2): Python 3 true division `/` always yields a float
/// — `int / int` casts both operands to f64 (`7 / 2 == 3.5`); a mixed
/// `float / int` casts only the int side (no `f64 / i64` mismatch).
#[test]
fn true_division() {
    let rust = xpile_transpile_to_rust("true_division.py");
    // PMAT-581: true division guards the zero divisor (Python raises
    // ZeroDivisionError for `1/0`); the divisor binds to `__fz`.
    assert!(
        rust.contains("(((a) as f64)) / __fz"),
        "int/int true division:\n{rust}"
    );
    assert!(
        rust.contains("let __fz: f64 = ((2i64) as f64)"),
        "mixed float/int division:\n{rust}"
    );
    let driver = r#"
fn main() {
    // python3: 7/2=3.5, 6/3=2.0, (3+4)/2=3.5, 5.0/2=2.5
    assert!((div(7, 2) - 3.5).abs() < 1e-9);
    assert!((div(6, 3) - 2.0).abs() < 1e-9);
    assert!((avg(3, 4) - 3.5).abs() < 1e-9);
    assert!((half(5.0) - 2.5).abs() < 1e-9);
}
"#;
    assert_rustc_runs("true_division", &rust, driver);
}

/// PMAT-502bt (Tranche 2): Python `**` with a float operand → float power
/// `(a).powf(b)` (both operands cast to f64). Negative/fractional exponents
/// work (`2.0 ** -1`, `9 ** 0.5`); `int ** int` stays integer.
#[test]
fn float_power() {
    let rust = xpile_transpile_to_rust("float_power.py");
    // PMAT-734b: float `**` now lowers to a guarded `{ … __pb.powf(__pe) … }` block
    // (overflow → OverflowError), so the operands bind to `__pb`/`__pe`.
    assert!(rust.contains(".powf(__pe)"), "float power:\n{rust}");
    assert!(
        rust.contains("let __pb: f64 = ((n) as f64)") && rust.contains("let __pe: f64 = 0.5f64"),
        "int-base float power (PMAT-734b guarded powf):\n{rust}"
    );
    let driver = r#"
fn main() {
    // python3: 3**2=9.0, 2**10=1024.0, 9**0.5=3.0, 2**-1=0.5
    assert!((square(3.0) - 9.0).abs() < 1e-9);
    assert!((powf(2.0, 10.0) - 1024.0).abs() < 1e-9);
    assert!((root(9) - 3.0).abs() < 1e-9);
    assert!((powf(2.0, -1.0) - 0.5).abs() < 1e-9);
}
"#;
    assert_rustc_runs("float_power", &rust, driver);
}

/// PMAT-636: `int ** <negative int literal>` is a float in Python
/// (`2 ** -1 == 0.5`), not an integer. A negative-literal exponent (detected
/// from the AST, including the `UnaryOp(USub, ...)` form) takes the float-power
/// path; non-negative integer powers stay integer. Cross-checked vs python3.
#[test]
fn neg_int_power() {
    let rust = xpile_transpile_to_rust("neg_int_power.py");
    // The negative-exponent forms use `powf`; the positive form stays `checked_pow`.
    assert!(rust.contains(".powf("), "neg-exp float power:\n{rust}");
    assert!(rust.contains("checked_pow"), "pos-exp int power:\n{rust}");
    let driver = r#"
fn main() {
    assert!((half() - 0.5).abs() < 1e-12);
    assert!((milli() - 0.001).abs() < 1e-12);
    assert!((recip_sq(2) - 0.25).abs() < 1e-12);
    assert!((sum_neg_powers() - 0.75).abs() < 1e-12);
    assert_eq!(pos_power(), 1024);
}
"#;
    assert_rustc_runs("neg_int_power", &rust, driver);
}

/// PMAT-502bu (Tranche 2): float augmented assignment with a non-float rhs
/// casts the int side to f64 (no `f64 <op> i64` mismatch), and `**=` uses
/// `powf` (not the int `checked_pow` path).
#[test]
fn aug_assign_float_int_rhs() {
    let rust = xpile_transpile_to_rust("aug_assign_float_int_rhs.py");
    assert!(
        rust.contains("x = (x + ((1i64) as f64))"),
        "float += int rhs:\n{rust}"
    );
    assert!(
        rust.contains("let __pe: f64 = ((3i64) as f64)") && rust.contains(".powf(__pe)"),
        "float **= int rhs (PMAT-734b guarded powf):\n{rust}"
    );
    assert!(
        !rust.contains("checked_pow"),
        "float **= must not use checked_pow:\n{rust}"
    );
    let driver = r#"
fn main() {
    // python3: x=3; +=1->4; *=3->12; /=2->6.0; //=2->3.0; %=5->3.0; **=2->9.0
    assert!((run(3.0) - 9.0).abs() < 1e-9);
    assert!((pow_assign(2.0) - 8.0).abs() < 1e-9);
}
"#;
    assert_rustc_runs("aug_assign_float_int_rhs", &rust, driver);
}

/// PMAT-502bv (Tranche 2): bare `return` (no value) in a void function — the
/// early-exit guard-clause shape → `return ();`.
#[test]
fn bare_return_guard() {
    let rust = xpile_transpile_to_rust("bare_return_guard.py");
    assert!(rust.contains("return ();"), "bare return:\n{rust}");
    let driver = r#"
fn main() {
    // guard prevents the `100 / 0` floor-div panic when v == 0
    guard_div(0);
    guard_div(4);
    push_pos(vec![1, 2], 9);
    push_pos(vec![1, 2], -1);
}
"#;
    assert_rustc_runs("bare_return_guard", &rust, driver);
}

/// PMAT-502bw (Tranche 2): the `print` builtin → `println!` (single-space
/// separator, trailing newline; bare `print()` → `println!()`).
#[test]
fn print_builtin() {
    let rust = xpile_transpile_to_rust("print_builtin.py");
    assert!(rust.contains(r#"println!("{}", n)"#), "print(int):\n{rust}");
    assert!(
        rust.contains(r#"println!("{} {}", name, n)"#),
        "print(a, b):\n{rust}"
    );
    assert!(rust.contains("println!();"), "bare print():\n{rust}");
    let driver = r#"
fn main() { demo(String::from("x"), 42); }
"#;
    assert_rustc_runs("print_builtin", &rust, driver);
}

/// PMAT-502bx (Tranche 2): print of `bool`/`float` args — Python prints
/// `True`/`False` and `3.0` (not Rust's `true`/`3`), via the str() machinery.
#[test]
fn print_bool_float() {
    let rust = xpile_transpile_to_rust("print_bool_float.py");
    // bool arg → the True/False desugar; float arg → the `.0` format block.
    assert!(rust.contains(r#""True""#), "bool print desugar:\n{rust}");
    assert!(
        rust.contains(r#"format!("{}.0""#),
        "float print format:\n{rust}"
    );
    let driver = r#"
fn main() { demo(2.5, true, 5); }
"#;
    assert_rustc_runs("print_bool_float", &rust, driver);
}

/// PMAT-502by (Tranche 2): `print(..., sep=…, end=…)` keyword args — `sep`
/// joins args, a custom `end` switches `println!` → `print!` (no newline).
#[test]
fn print_sep_end() {
    let rust = xpile_transpile_to_rust("print_sep_end.py");
    assert!(
        rust.contains(r#"println!("{}, {}", a, b)"#),
        "sep=', ':\n{rust}"
    );
    assert!(
        rust.contains(r#"print!("{}", String::from("loading"))"#),
        "end='' → print!:\n{rust}"
    );
    assert!(
        rust.contains(r#"println!("{} | {}", a, b)"#),
        "sep=' | ':\n{rust}"
    );
    let driver = r#"
fn main() { demo(1, 2); }
"#;
    assert_rustc_runs("print_sep_end", &rust, driver);
}

/// PMAT-502bz (Tranche 2): chained assignment `x = y = z = <literal>` → one
/// binding per target (each an independent copy of the scalar literal).
#[test]
fn chained_assign() {
    let rust = xpile_transpile_to_rust("chained_assign.py");
    // a is mutated later → `let mut a`; b/c are plain `let`.
    assert!(
        rust.contains("let mut a: i64 = 0i64"),
        "chained let:\n{rust}"
    );
    assert!(rust.contains("let c: i64 = 0i64"), "third target:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(init_sum(), 8);
    assert_eq!(flags(), 2);
}
"#;
    assert_rustc_runs("chained_assign", &rust, driver);
}

/// PMAT-502ca (Tranche 2): `enumerate(xs, start)` — the optional start index
/// offsets the index var via a checked add (`(__i as i64).checked_add(start)`,
/// C-PY-INT-ARITH, PMAT-595).
#[test]
fn enumerate_start() {
    let rust = xpile_transpile_to_rust("enumerate_start.py");
    assert!(
        rust.contains("(__i as i64).checked_add(1i64)")
            && rust.contains("(__i as i64).checked_add(10i64)"),
        "enumerate start offset:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(weighted(vec![10, 20, 30]), 140);
    assert_eq!(last_index(vec![5, 5, 5]), 12);
}
"#;
    assert_rustc_runs("enumerate_start", &rust, driver);
}

/// PMAT-594: `enumerate(xs, start=N)` keyword form must honor the start (it was
/// only read from the 2nd positional arg, silently dropping the keyword → +0).
/// Both spellings emit `+ 10i64`. Cross-checked vs python3: sum of indices
/// 10+11+12 == 33 for a 3-element list.
#[test]
fn enumerate_start_kwarg() {
    let rust = xpile_transpile_to_rust("enumerate_start_kwarg.py");
    // The keyword form must produce the same checked offset as positional.
    assert_eq!(
        rust.matches("(__i as i64).checked_add(10i64)").count(),
        2,
        "both enumerate forms must offset by start:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sum_keyword(vec![1, 2, 3]), 33);
    assert_eq!(sum_positional(vec![1, 2, 3]), 33);
}
"#;
    assert_rustc_runs("enumerate_start_kwarg", &rust, driver);
}

/// PMAT-642: `enumerate(xs, start)` accepts a NEGATIVE literal start
/// (`start=-1`, parsed as `UnaryOp(USub, Int)`) — was rejected as a "non-literal
/// start". The codegen already `checked_add`s the start, so a negative one
/// works. Cross-checked vs python3.
#[test]
fn enumerate_neg_start() {
    let rust = xpile_transpile_to_rust("enumerate_neg_start.py");
    assert!(
        rust.contains("checked_add(-1i64)") && rust.contains("checked_add(-5i64)"),
        "negative enumerate start should offset by the negative literal:\n{rust}"
    );
    let driver = r#"
fn main() {
    // kw_neg: i in {-1,0,1}, x in {10,20,30} -> -10 + 0 + 30 = 20
    assert_eq!(kw_neg(vec![10, 20, 30]), 20);
    // positional_neg: i in {-5,-4,-3} -> -12
    assert_eq!(positional_neg(vec![1, 2, 3]), -12);
    // pos_start: i in {5,6,7} -> 18
    assert_eq!(pos_start(vec![1, 2, 3]), 18);
}
"#;
    assert_rustc_runs("enumerate_neg_start", &rust, driver);
}

/// PMAT-502cb (Tranche 2): `str.format` positional `{N}` placeholders
/// (reorder / repeat) — re-emitted verbatim into Rust's `format!`.
#[test]
fn format_positional() {
    let rust = xpile_transpile_to_rust("format_positional.py");
    assert!(
        rust.contains(r#"format!("{1} {0}", a, b)"#),
        "reorder:\n{rust}"
    );
    assert!(rust.contains(r#"format!("{0}-{0}", a)"#), "repeat:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(swap(String::from("x"), String::from("y")), "y x");
    assert_eq!(dup(7), "7-7");
    assert_eq!(seq(String::from("a"), String::from("b")), "a and b");
}
"#;
    assert_rustc_runs("format_positional", &rust, driver);
}

/// PMAT-502cc (Tranche 2): context-aware `not <bool var>` → `(!b)` (the
/// context-free path mis-inferred a bare Ident as int and rejected it).
#[test]
fn not_bool_var() {
    let rust = xpile_transpile_to_rust("not_bool_var.py");
    assert!(rust.contains("(!b)"), "not bool var:\n{rust}");
    assert!(rust.contains("if (!active)"), "not in guard:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(toggle(true), false);
    assert_eq!(toggle(false), true);
    assert_eq!(clamp(false, 9), 0);
    assert_eq!(clamp(true, 9), 9);
}
"#;
    assert_rustc_runs("not_bool_var", &rust, driver);
}

/// PMAT-502cd (Tranche 2): string indexing `s[i]` → a 1-char string
/// (positive / negative-from-end / variable int index).
#[test]
fn str_char_at() {
    let rust = xpile_transpile_to_rust("str_char_at.py");
    assert!(
        rust.contains("__cs[__idx as usize].to_string()"),
        "str char-at:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(first(String::from("hello")), "h");
    assert_eq!(last(String::from("hello")), "o");
    assert_eq!(at(String::from("hello"), 1), "e");
    assert_eq!(at(String::from("hello"), -2), "l");
}
"#;
    assert_rustc_runs("str_char_at", &rust, driver);
}

/// PMAT-502ce (Tranche 2): context-aware `and`/`or` over bool variables →
/// `(a && b)` / `(a || b)` (the context-free path mis-inferred bare Idents).
#[test]
fn bool_op_var() {
    let rust = xpile_transpile_to_rust("bool_op_var.py");
    assert!(rust.contains("(a && b)"), "and over bools:\n{rust}");
    assert!(rust.contains("((a || b) || c)"), "or chain:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(both(true, false), false);
    assert_eq!(both(true, true), true);
    assert_eq!(either(false, false, true), true);
    assert_eq!(gate(5, true), 5);
    assert_eq!(gate(5, false), 0);
}
"#;
    assert_rustc_runs("bool_op_var", &rust, driver);
}

/// PMAT-637: Python `x or default` / `x and y` over non-bool operands returns
/// the OPERAND (by truthiness), not a bool — the common default-value idiom.
/// Supported for `x <op> y` where x is a variable and both operands share a
/// non-bool type (int / str / list / dict / set). Bool operands keep `||`/`&&`.
/// Cross-checked vs python3.
#[test]
fn short_circuit_operand() {
    let rust = xpile_transpile_to_rust("short_circuit_operand.py");
    // Operand-returning forms lower to an if-expression on the truthiness;
    // the bool form keeps the plain `||`.
    assert!(rust.contains("if (x != 0i64)"), "int truthiness:\n{rust}");
    assert!(rust.contains("(a || b)"), "bool form unchanged:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(or_default(0), 5);
    assert_eq!(or_default(7), 7);
    assert_eq!(and_then(0), 0);
    assert_eq!(and_then(3), 99);
    assert_eq!(str_or(String::from("hi")), "hi");
    assert_eq!(str_or(String::from("")), "fallback");
    assert_eq!(list_or(vec![1, 2]), 3);
    assert_eq!(list_or(vec![]), 18);
    assert_eq!(bool_logic(false, true), true);
}
"#;
    assert_rustc_runs("short_circuit_operand", &rust, driver);
}

/// PMAT-638: short-circuit CHAINS `a or b or c` / `a and b and c` return the
/// first decisive operand by truthiness (the multi-fallback idiom). Leading
/// operands are variables (`Ident`); the last may be any expression. Generalizes
/// PMAT-637's 2-operand form via a right-to-left `IfExpr` fold. Cross-checked vs
/// python3.
#[test]
fn short_circuit_chain() {
    let rust = xpile_transpile_to_rust("short_circuit_chain.py");
    // Nested if-expressions over each operand's truthiness.
    assert!(rust.contains("if (a != 0i64)"), "chain truthiness:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(first_truthy(0, 0, 7), 7);
    assert_eq!(first_truthy(3, 0, 7), 3);
    assert_eq!(first_truthy(0, 5, 7), 5);
    assert_eq!(all_required(3, 5, 7), 7);
    assert_eq!(all_required(3, 0, 7), 0);
    assert_eq!(name_with_default(String::from(""), String::from("")), "default");
    assert_eq!(name_with_default(String::from("x"), String::from("y")), "x");
    assert_eq!(name_with_default(String::from(""), String::from("e")), "e");
}
"#;
    assert_rustc_runs("short_circuit_chain", &rust, driver);
}

/// PMAT-667: a bool-result `and`/`or` with a container/int operand in a boolean
/// context — `if xs and xs[0] > i:` — was rejected ("operands must be Bool").
/// Each operand is coerced to its truthiness and folded with `&&`/`||`. The
/// operand-RETURN form (`x or 5`, same non-bool type) is unaffected — it still
/// returns the value, not a bool. Cross-checked vs python3.
#[test]
fn bool_op_truthy_operand() {
    let rust = xpile_transpile_to_rust("bool_op_truthy_operand.py");
    let driver = r#"
fn main() {
    assert_eq!(guard(vec![], 0), 0);      // was rejected
    assert_eq!(guard(vec![1], 0), 1);
    assert_eq!(guard(vec![1], 5), 0);
    assert_eq!(int_and_bool(0, true), 0);
    assert_eq!(int_and_bool(3, true), 1);
    assert_eq!(int_and_bool(3, false), 0);
    assert_eq!(all_bool_regression(true, true), 1);
    assert_eq!(all_bool_regression(true, false), 0);
    assert_eq!(or_default_regression(0), 5);  // value-return preserved
    assert_eq!(or_default_regression(7), 7);
}
"#;
    assert_rustc_runs("bool_op_truthy_operand", &rust, driver);
}

/// PMAT-697: a float operand of `and`/`or` is truthy iff `!= 0.0` (IEEE-754
/// exact: `-0.0` falsy, `nan`/`inf` truthy). The value-return forms (`x or 9.0`,
/// `x and y` over float idents) and the bool-context form (`if x and y > 1.0`)
/// were rejected. Cross-checked vs python3 (HUNT-V7 item V7-5, float sub-slice).
#[test]
fn bool_op_float_truthy() {
    let rust = xpile_transpile_to_rust("bool_op_float_truthy.py");
    let driver = r#"
fn main() {
    assert_eq!(or_default(0.0), 9.0);  // 0.0 falsy
    assert_eq!(or_default(-0.0), 9.0); // -0.0 falsy (IEEE-754: -0.0 != 0.0 is false)
    assert_eq!(or_default(3.0), 3.0);
    assert!(or_default(f64::NAN).is_nan()); // nan truthy -> returns x
    assert_eq!(and_val(0.0, 5.0), 0.0);
    assert_eq!(and_val(2.0, 5.0), 5.0);
    assert_eq!(in_cond(2.0, 5.0), 5.0);
    assert_eq!(in_cond(0.0, 5.0), -1.0);
    assert_eq!(in_cond(2.0, 0.5), -1.0);
}
"#;
    assert_rustc_runs("bool_op_float_truthy", &rust, driver);
}

/// PMAT-710: a bool operand into a float-coercing op (`/` `//` `%` `**`) emitted
/// `(bool) as f64` → rustc E0606. Python's bool is an int subtype (`True/2`==0.5),
/// so xpile casts through i64 (`((b) as i64) as f64`). One `to_f64_operand` fix
/// covers all four operators. Cross-checked vs python3 (HUNT-V9 item V9-4).
#[test]
fn bool_float_ops() {
    let rust = xpile_transpile_to_rust("bool_float_ops.py");
    assert!(
        rust.contains("as i64)) as f64"),
        "bool operand should cast through i64 to f64:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(div(true, 2), 0.5);
    assert_eq!(floordiv(true, 2.0), 0.0);
    assert_eq!(modulo(true, 2.0), 1.0);
    assert_eq!(power(true, 3.0), 1.0);
}
"#;
    assert_rustc_runs("bool_float_ops", &rust, driver);
}

/// PMAT-699: a bare-variable key or value in a dict literal is moved into
/// `m.insert(...)`; reusing it afterward was rustc E0382. The DictLit codegen now
/// clones bare idents at insert (literals/temporaries are emitted as-is).
/// Cross-checked vs python3 (HUNT-V8 item V8-2).
#[test]
fn dict_lit_noncopy_clone() {
    let rust = xpile_transpile_to_rust("dict_lit_noncopy_clone.py");
    assert!(
        rust.contains("__xpile_map.insert(k.clone(), v.clone())")
            && rust.contains("__xpile_map.insert(String::from(\"key\"), s.clone())"),
        "dict literal should clone bare-ident keys/values (literal key untouched):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(kv("x".to_string(), 5), 5);
    assert_eq!(with_str_val("hello".to_string()), 5);
    assert_eq!(two_pairs("a".to_string(), "b".to_string()), 2);
    assert_eq!(int_keys(1, 2), 2); // Copy keys clone fine under rustc
}
"#;
    assert_rustc_runs("dict_lit_noncopy_clone", &rust, driver);
}

/// PMAT-502cf (Tranche 2): dict comprehension over `d.items()` with a tuple
/// target → a `ForEachPair(Pairs)` loop building the dict.
#[test]
fn dict_comp_items() {
    let rust = xpile_transpile_to_rust("dict_comp_items.py");
    assert!(
        rust.contains("for (k, v) in") && rust.contains(".insert(k.clone()"),
        "dict comp over items:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut m = std::collections::HashMap::new();
    m.insert(String::from("a"), 3);
    m.insert(String::from("b"), -1);
    let d = doubled(m.clone());
    assert_eq!(d[&String::from("a")], 6);
    assert_eq!(d[&String::from("b")], -2);
    let p = positives(m.clone());
    assert_eq!(p.get(&String::from("a")), Some(&3));
    assert_eq!(p.get(&String::from("b")), None);
}
"#;
    assert_rustc_runs("dict_comp_items", &rust, driver);
}

/// PMAT-502cg (Tranche 2): list & set comprehensions over `d.items()` with a
/// tuple target → `ForEachPair(Pairs)` loops (the `if` filter composes).
#[test]
fn comp_items() {
    let rust = xpile_transpile_to_rust("comp_items.py");
    assert!(
        rust.contains("for (k, v) in") && rust.contains(".push(v)"),
        "list comp over items:\n{rust}"
    );
    assert!(rust.contains(".insert(v)"), "set comp over items:\n{rust}");
    let driver = r#"
fn main() {
    let mut m = std::collections::HashMap::new();
    m.insert(String::from("a"), 3);
    m.insert(String::from("b"), -1);
    let mut vs = values(m.clone());
    vs.sort();
    assert_eq!(vs, vec![-1, 3]);
    assert_eq!(pos_keys(m.clone()), vec![String::from("a")]);
    let st = value_set(m.clone());
    assert!(st.contains(&3) && st.contains(&-1) && st.len() == 2);
}
"#;
    assert_rustc_runs("comp_items", &rust, driver);
}

/// PMAT-502ch (Tranche 2): `str.format` with format specs `{:.2f}` / `{:05d}`
/// — translated to Rust specs by the arg's type.
#[test]
fn format_spec() {
    let rust = xpile_transpile_to_rust("format_spec.py");
    // PMAT-680: a float `.Nf` spec now routes through the NaN-guarded FormatSpec
    // (plain `{}` field + `is_nan()` guard) instead of an inlined `{:.2}`.
    assert!(
        rust.contains(r#"format!("${}", "#) && rust.contains("__nf.is_nan()"),
        "float spec should route through the NaN-guarded FormatSpec:\n{rust}"
    );
    assert!(
        rust.contains(r#"format!("id={:05}", n)"#),
        "int width spec:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(money(3.14159), "$3.14");
    assert_eq!(padded(42), "id=00042");
    assert_eq!(both(2.5, 7), "2.5 (007)");
}
"#;
    assert_rustc_runs("format_spec", &rust, driver);
}

/// PMAT-659: a float-precision f-string spec (`{:.Nf}` / `{:.N%}`) of NaN prints
/// "nan" in Python, but Rust's `format!` prints "NaN". The FormatSpec emit now
/// guards NaN (a float-precision spec is float-only). inf already matches; the
/// `.`-fill str case (PMAT-658) is not a float precision so it's untouched.
/// Cross-checked vs python3.
#[test]
fn format_nan_precision() {
    let rust = xpile_transpile_to_rust("format_nan_precision.py");
    assert!(
        rust.contains("__nf.is_nan()") && rust.contains("String::from(\"nan\")"),
        "float-precision format must guard NaN:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(fmt_nan(), "nan");      // was "NaN"
    assert_eq!(fmt_inf(), "inf");
    assert_eq!(fmt_normal(), "3.14");
    assert_eq!(fmt_percent_nan(), "nan%");
    assert_eq!(fmt_sign_prec(), "+3.50");
}
"#;
    assert_rustc_runs("format_nan_precision", &rust, driver);
}

/// PMAT-658: a format-spec FILL char before an alignment (`{:->10}`, `{:*<8}`,
/// `{:.^9}`). Python allows any char as a fill when it precedes an align; the
/// translator dropped a `-` fill (mistaken for a sign → spaces) and rejected
/// `*`/`.` fills. Rust uses the identical `{:fill<width}` syntax. Both the
/// `.format()` and f-string paths route through the shared translator.
/// Cross-checked vs python3.
#[test]
fn format_fill_char() {
    let rust = xpile_transpile_to_rust("format_fill_char.py");
    let driver = r#"
fn main() {
    assert_eq!(fill_dash(), "--------ab");           // '-' fill was dropped
    assert_eq!(fill_star_left(), "hi******");        // '*' fill was rejected
    assert_eq!(fill_caret_center(), "...xy....");     // '.' fill was rejected
    assert_eq!(fill_dash_fstring(), "--------ab");
    assert_eq!(int_fill(), "****42");
    assert_eq!(width_no_fill_regression(), "    zz"); // align-only still works
}
"#;
    assert_rustc_runs("format_fill_char", &rust, driver);
}

/// PMAT-597: the standalone `format(value[, spec])` builtin (distinct from
/// `str.format`). `format(x)` == `str(x)`; `format(x, "<spec>")` reuses the
/// f-string format mini-language. Previously fell through to a bare
/// `format(...)` call — rustc E0423 (`format` is a macro, not a function).
/// Cross-checked vs python3: `format(255,"x")=="ff"`, `format(42,"05d")=="00042"`,
/// `format(7)=="7"`, `format(0.25,".1%")=="25.0%"`.
#[test]
fn format_builtin() {
    let rust = xpile_transpile_to_rust("format_builtin.py");
    assert!(
        // PMAT-613: a bare radix spec now emits the sign-magnitude IntRadixStr
        // block (correct for negatives) rather than `format!("{:x}", n)`.
        rust.contains(r#"format!("{}{:x}", __sign, __m)"#)
            && rust.contains(r#"format!("{:05}", n)"#),
        "format(n, spec) → format! macro:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(hex_of(255), "ff");
    assert_eq!(padded(42), "00042");
    assert_eq!(plain(7), "7");
    assert_eq!(pct(0.25), "25.0%");
}
"#;
    assert_rustc_runs("format_builtin", &rust, driver);
}

/// PMAT-613: an f-string radix spec (`:x`/`:X`/`:b`/`:o`) of a possibly-negative
/// int. Python formats negatives sign-magnitude (`f"{-255:x}"` == "-ff"), but
/// `format!("{:x}", n)` is two's-complement (`ffffffffffffff01`) → silent wrong
/// output. The bare radix now reuses the sign-magnitude `IntRadixStr` path
/// (prefixed: false), matching Python and the `hex`/`bin`/`oct` builtins.
/// Cross-checked vs python3 (n = -255, 255, -8, 0).
#[test]
fn fstring_radix_negative() {
    let rust = xpile_transpile_to_rust("fstring_radix_negative.py");
    assert!(
        rust.contains(r#"format!("{}{:x}", __sign, __m)"#) && rust.contains("__n.unsigned_abs()"),
        "f-string radix should emit the sign-magnitude block:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(hexfmt(-255), "-ff");
    assert_eq!(hexfmt(255), "ff");
    assert_eq!(hexup(-255), "-FF");
    assert_eq!(binfmt(-5), "-101");
    assert_eq!(octfmt(-8), "-10");
    assert_eq!(hexfmt(0), "0");
    assert_eq!(labeled(-255), "val=-ff!");
}
"#;
    assert_rustc_runs("fstring_radix_negative", &rust, driver);
}

/// PMAT-502ci (Tranche 2): `for i in reversed(range(...))` — descending range
/// iteration (desugars to a step -1 range).
#[test]
fn reversed_range() {
    let rust = xpile_transpile_to_rust("reversed_range.py");
    // start = n - 1, descending. PMAT-634: a fresh counter `__forc{N}` drives
    // the loop (`while __forc{N} > …`), not the user variable.
    assert!(
        rust.contains("(n).checked_sub(1i64)") && rust.contains("while (__forc0 > "),
        "reversed range:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(digits_desc(4), 3210);
    assert_eq!(mid(0), 432);
    assert_eq!(digits_desc(0), 0);
}
"#;
    assert_rustc_runs("reversed_range", &rust, driver);
}

/// PMAT-502cj (Tranche 2): `list(range(...))` materialises a range into a Vec
/// (`.collect::<Vec<i64>>()`, `.step_by` for a positive step); `list(xs)` copies.
#[test]
fn list_range() {
    let rust = xpile_transpile_to_rust("list_range.py");
    assert!(
        rust.contains("(0i64..n).collect::<Vec<i64>>()"),
        "list(range(n)):\n{rust}"
    );
    assert!(rust.contains(".step_by(2usize)"), "stepped range:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(upto(4), vec![0, 1, 2, 3]);
    assert_eq!(span(2, 5), vec![2, 3, 4]);
    assert_eq!(evens(10), vec![0, 2, 4, 6, 8]);
    assert_eq!(upto(0), Vec::<i64>::new());
    assert_eq!(copy(vec![7, 8]), vec![7, 8]);
}
"#;
    assert_rustc_runs("list_range", &rust, driver);
}

/// PMAT-502ck (Tranche 2): for-loops over a call iterable that lowers to a
/// list — `reversed(xs)` / `sorted(xs)` / `list(range(n))`.
#[test]
fn for_over_call() {
    let rust = xpile_transpile_to_rust("for_over_call.py");
    assert!(
        rust.contains("__xv.reverse(); __xv }.iter().cloned()"),
        "for over reversed(xs):\n{rust}"
    );
    assert!(
        rust.contains("(0i64..n).collect::<Vec<i64>>().iter().cloned()"),
        "for over list(range(n)):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(rev_fold(vec![1, 2, 3]), 321);
    assert_eq!(sort_fold(vec![3, 1, 2]), 123);
    assert_eq!(range_sum(5), 10);
}
"#;
    assert_rustc_runs("for_over_call", &rust, driver);
}

/// PMAT-502cl (Tranche 2): string iteration `for c in s` — each char a 1-char
/// string (lowered via `Expr::StrChars`).
#[test]
fn str_iter() {
    let rust = xpile_transpile_to_rust("str_iter.py");
    assert!(
        rust.contains(
            ".chars().map(|__c| __c.to_string()).collect::<Vec<String>>().iter().cloned()"
        ),
        "string iteration:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(count_vowels(String::from("education")), 5);
    assert_eq!(reverse_str(String::from("abc")), "cba");
}
"#;
    assert_rustc_runs("str_iter", &rust, driver);
}

/// PMAT-502cm (Tranche 2): `ord(c)` (str → code point) and `chr(n)` (int →
/// 1-char str) builtins.
#[test]
fn ord_chr() {
    let rust = xpile_transpile_to_rust("ord_chr.py");
    // PMAT-702: ord asserts exactly one char (`__oc.next().is_some()` panic for a
    // multi-char string) instead of silently taking the first; chr unchanged.
    assert!(
        rust.contains("__oc.next().is_some()") && rust.contains("char::from_u32("),
        "ord/chr:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(code(String::from("A")), 65);
    assert_eq!(char(97), "a");
    assert_eq!(shift(String::from("a")), "b");
}
"#;
    assert_rustc_runs("ord_chr", &rust, driver);
}

/// PMAT-702: `ord()` requires exactly one character — `ord("ab")` / `ord("")` is
/// a Python TypeError, NOT the first char's code point. xpile now asserts a
/// single char (was: silently returned the first). The lowering is a parenthesized
/// block, so it stays valid in any expression position (`ord(c) + 1`).
/// Cross-checked vs python3 (HUNT-V8 item V8-15).
#[test]
fn ord_single_char() {
    let rust = xpile_transpile_to_rust("ord_single_char.py");
    let driver = r#"
fn main() {
    assert_eq!(code("a".to_string()), 97);
    assert_eq!(code("é".to_string()), 233); // Unicode code point
    assert_eq!(code_plus("a".to_string()), 98); // block-expr in arithmetic
    // multi-char and empty are a Python TypeError -> panic
    assert!(std::panic::catch_unwind(|| code("ab".to_string())).is_err());
    assert!(std::panic::catch_unwind(|| code("".to_string())).is_err());
}
"#;
    assert_rustc_runs("ord_single_char", &rust, driver);
}

/// PMAT-502cn (Tranche 2): 2-arg `min`/`max` over `str` operands (Ord).
#[test]
fn min_max_str() {
    let rust = xpile_transpile_to_rust("min_max_str.py");
    assert!(rust.contains("(a).min(b)"), "min(str):\n{rust}");
    assert!(rust.contains("(a).max(b)"), "max(str):\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(smaller(String::from("apple"), String::from("banana")), "apple");
    assert_eq!(larger(String::from("apple"), String::from("banana")), "banana");
}
"#;
    assert_rustc_runs("min_max_str", &rust, driver);
}

/// PMAT-502co (Tranche 2): no-arg `str.split()` → whitespace split.
/// PMAT-649: the emit now uses a C0-inclusive predicate + empty filter.
#[test]
fn split_whitespace() {
    let rust = xpile_transpile_to_rust("split_whitespace.py");
    assert!(
        rust.contains(".split(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).filter(|__c| !__c.is_empty()).map(|__c| __c.to_string()).collect::<Vec<String>>()"),
        "split():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(word_count(String::from("  hello   world  foo ")), 3);
    assert_eq!(first_word(String::from("  alpha beta")), "alpha");
}
"#;
    assert_rustc_runs("split_whitespace", &rust, driver);
}

/// PMAT-649: Python `str.split()` (no arg) splits on `str.isspace()` whitespace,
/// which INCLUDES the C0 separators U+001C-1F (FS/GS/RS/US) — Rust's
/// `split_whitespace` excludes them. The emit now uses the PMAT-600 C0-inclusive
/// predicate + an empty filter (preserving the leading/trailing/consecutive
/// no-empty collapse). Cross-checked vs python3.
#[test]
fn split_c0_separators() {
    let rust = xpile_transpile_to_rust("split_c0_separators.py");
    let driver = r#"
fn main() {
    // "a\x1cb\x1dc\x1ed\x1fe" → 5 parts (all four C0 separators split)
    assert_eq!(count_c0(), 5);
    // leading/trailing/consecutive C0 + ASCII ws collapse, no empties
    assert_eq!(count_mixed(), 3);
    // plain ASCII whitespace still works
    assert_eq!(count_plain(), 3);
    assert_eq!(last_part(), "c");
}
"#;
    assert_rustc_runs("split_c0_separators", &rust, driver);
}

/// PMAT-502cp (Tranche 2): tuple literals as list elements `[(1, 2), (3, 4)]`.
#[test]
fn list_of_tuples() {
    let rust = xpile_transpile_to_rust("list_of_tuples.py");
    assert!(
        rust.contains("vec![(1i64, 2i64), (3i64, 4i64)]"),
        "list of tuple literals:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(make(), vec![(1, 2), (3, 4)]);
    assert_eq!(dot(make()), 14);
}
"#;
    assert_rustc_runs("list_of_tuples", &rust, driver);
}

/// PMAT-502cq (Tranche 2): `str.removeprefix(p)` / `removesuffix(p)` →
/// `strip_prefix`/`strip_suffix` (unchanged when the affix is absent).
#[test]
fn remove_affix() {
    let rust = xpile_transpile_to_rust("remove_affix.py");
    assert!(
        rust.contains("__s.strip_prefix(&("),
        "removeprefix:\n{rust}"
    );
    assert!(
        rust.contains("__s.strip_suffix(&("),
        "removesuffix:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(strip_pre(String::from("foo_bar")), "bar");
    assert_eq!(strip_pre(String::from("baz")), "baz");
    assert_eq!(strip_suf(String::from("note.txt")), "note");
    assert_eq!(strip_suf(String::from("note")), "note");
}
"#;
    assert_rustc_runs("remove_affix", &rust, driver);
}

/// PMAT-502cr (Tranche 2): `str.swapcase()` — per-char upper↔lower.
#[test]
fn swapcase() {
    let rust = xpile_transpile_to_rust("swapcase.py");
    assert!(
        rust.contains(".chars().map(|__c| if __c.is_uppercase()"),
        "swapcase:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(swap(String::from("Hello, World! 42")), "hELLO, wORLD! 42");
}
"#;
    assert_rustc_runs("swapcase", &rust, driver);
}

/// PMAT-502cs (Tranche 2): `str.zfill(width)` — sign-aware zero-pad.
#[test]
fn zfill() {
    let rust = xpile_transpile_to_rust("zfill.py");
    assert!(
        rust.contains("__s.starts_with('-') || __s.starts_with('+')"),
        "zfill sign-aware:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pad(String::from("42")), "00042");
    assert_eq!(pad(String::from("-42")), "-0042");
    assert_eq!(pad(String::from("+7")), "+0007");
    assert_eq!(pad(String::from("123456")), "123456");
    assert_eq!(pad(String::from("")), "00000");
}
"#;
    assert_rustc_runs("zfill", &rust, driver);
}

/// PMAT-630: a context-dependent argument to a user-function call — a `bool`
/// `and`/`or`/`not`/ternary expression — was lowered context-free (`lower_call`
/// uses `lower_expr`), losing param types, so `g(5, c and d)` was rejected
/// ("operands of `and`/`or` must be Bool"). User-call args are now lowered
/// context-aware. Found by hunt #9 (H9-15). Cross-checked vs python3.
#[test]
fn bool_arg_to_call() {
    let rust = xpile_transpile_to_rust("bool_arg_to_call.py");
    assert!(
        rust.contains("g(5i64, (c && d))") && rust.contains("g(5i64, (!c))"),
        "bool and/or/not args to a user call should lower context-aware:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(f_and(true, false), 5);
    assert_eq!(f_or(true, false), 6);
    assert_eq!(f_not(false), 6);
    assert_eq!(f_tern(true), 6);
}
"#;
    assert_rustc_runs("bool_arg_to_call", &rust, driver);
}

/// PMAT-502ct (Tranche 2): default parameter values — omitted trailing args
/// are filled with the declared default at the call site.
#[test]
fn default_params() {
    let rust = xpile_transpile_to_rust("default_params.py");
    assert!(
        rust.contains(r#"greet(name, String::from("Hello"))"#),
        "default filled:\n{rust}"
    );
    assert!(
        rust.contains("add(1i64, 10i64, 100i64)"),
        "two defaults:\n{rust}"
    );
    assert!(
        rust.contains("add(1i64, 10i64, 5i64)"),
        "kw override:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(use_default(String::from("Sam")), "Hello, Sam");
    assert_eq!(with_hi(String::from("Sam")), "Hi, Sam");
    assert_eq!(call_add(), 111);
    assert_eq!(call_kw(), 16);
}
"#;
    assert_rustc_runs("default_params", &rust, driver);
}

/// PMAT-628: a non-Copy variable used more than once — twice in a list literal
/// (`[inner, inner]`) or appended twice (`g.append(row); g.append(row)`) —
/// emitted a move-then-use (E0382). The reused non-Copy var is now cloned
/// (mirrors PMAT-588 call-arg clone); distinct elements are not cloned. Found by
/// hunt #8 (H8-0/1). Cross-checked vs python3.
#[test]
fn list_reused_var_clone() {
    let rust = xpile_transpile_to_rust("list_reused_var_clone.py");
    assert!(
        // reused vars clone; the literal's emit shows it.
        rust.contains("vec![(inner).clone(), (inner).clone()]")
            && rust.contains("g.push((row).clone());"),
        "reused non-Copy vars in a list literal / append must clone:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(literal(), vec![vec![1, 2, 3], vec![1, 2, 3]]);
    assert_eq!(appended(), vec![vec![0, 0], vec![0, 0]]);
    assert_eq!(distinct(), vec![vec![1], vec![2]]);
}
"#;
    assert_rustc_runs("list_reused_var_clone", &rust, driver);
}

/// PMAT-627: a default-using function called in argument position of another
/// call (`f(g(x))` where `g` has a default). The outer call was default-filled
/// but the nested call (lowered context-free by `lower_call`) was left
/// under-applied → E0061. Default-filling now recurses into nested user-calls in
/// argument position. Found by hunt #8 (H8-16). Cross-checked vs python3.
#[test]
fn nested_default_call() {
    let rust = xpile_transpile_to_rust("nested_default_call.py");
    assert!(
        // both the outer AND the inner call carry the filled default.
        rust.contains("inc(inc(x, 1i64), 1i64)"),
        "nested default-using calls must fill the inner default too:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(into_user(5), 7);          // inc(inc(5)) = inc(6) = 7
    assert_eq!(deep(5), 8);               // inc(inc(inc(5))) = 8
    assert_eq!(two_args(5, 10), 17);      // inc(inc(5), inc(10)) = inc(6, 11) = 17
}
"#;
    assert_rustc_runs("nested_default_call", &rust, driver);
}

/// PMAT-502cu (Tranche 2): `str.center(width)` — CPython parity-biased pad.
#[test]
fn center() {
    let rust = xpile_transpile_to_rust("center.py");
    assert!(
        rust.contains("__marg / 2 + (__marg & __w & 1)"),
        "center bias:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(c(String::from("x")), "  x  ");
    assert_eq!(c(String::from("ab")), "  ab ");
    assert_eq!(c(String::from("abcde")), "abcde");
    assert_eq!(c(String::from("abcdef")), "abcdef");
}
"#;
    assert_rustc_runs("center", &rust, driver);
}

/// PMAT-632: optional fill-char arg for `rjust`/`ljust`/`center`
/// (`"ab".rjust(5, "*")`). Pads by repeating the fill string to the
/// deficit char count; already-wide strings are returned unchanged. The
/// `center` left-bias matches the space-fill form. Cross-checked vs python3.
#[test]
fn str_pad_fill() {
    let rust = xpile_transpile_to_rust("str_pad_fill.py");
    // Manual repeat-pad path (not `format!` width, which can't take a
    // dynamic fill char).
    assert!(rust.contains(".repeat(__w - __n)"), "fill repeat:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(pad_r("ab".to_string(), 5), "***ab");
    assert_eq!(pad_l("ab".to_string(), 5), "ab***");
    assert_eq!(pad_c("ab".to_string(), 6), "--ab--");
    assert_eq!(pad_c("ab".to_string(), 7), "---ab--"); // CPython left-bias
    // No padding when already wide enough (Python returns unchanged).
    assert_eq!(pad_r("abcdef".to_string(), 3), "abcdef");
    assert_eq!(pad_c("abcdef".to_string(), 3), "abcdef");
    assert_eq!(lit_pad(), "0005");
}
"#;
    assert_rustc_runs("str_pad_fill", &rust, driver);
}

/// PMAT-502cv (Tranche 2): `hex(n)` / `oct(n)` / `bin(n)` → radix strings
/// (sign-first, `0x`/`0o`/`0b` prefix).
#[test]
fn int_radix() {
    let rust = xpile_transpile_to_rust("int_radix.py");
    assert!(rust.contains(r#"format!("{}0x{:x}""#), "hex:\n{rust}");
    assert!(rust.contains(r#"format!("{}0b{:b}""#), "bin:\n{rust}");
    assert!(rust.contains(r#"format!("{}0o{:o}""#), "oct:\n{rust}");
    let driver = r#"
fn main() {
    assert_eq!(h(255), "0xff");
    assert_eq!(h(-255), "-0xff");
    assert_eq!(h(0), "0x0");
    assert_eq!(b(5), "0b101");
    assert_eq!(b(-5), "-0b101");
    assert_eq!(o(8), "0o10");
}
"#;
    assert_rustc_runs("int_radix", &rust, driver);
}

/// PMAT-502cw (Tranche 2): `set(xs)` materialises a list into a HashSet.
#[test]
fn set_from_list() {
    let rust = xpile_transpile_to_rust("set_from_list.py");
    assert!(
        rust.contains(".iter().cloned().collect::<std::collections::HashSet<_>>()"),
        "set(xs):\n{rust}"
    );
    let driver = r#"
fn main() {
    let u = uniq(vec![1, 2, 2, 3, 3, 3]);
    assert_eq!(u.len(), 3);
    assert!(u.contains(&2));
    assert_eq!(has(vec![1, 2, 3], 2), true);
    assert_eq!(has(vec![1, 2, 3], 9), false);
}
"#;
    assert_rustc_runs("set_from_list", &rust, driver);
}

/// PMAT-502dy (Tranche 2): nested subscript assignment `grid[i][j] = v` (2D/ND
/// list grids), including the augmented form.
#[test]
fn nested_index_assign() {
    let rust = xpile_transpile_to_rust("nested_index_assign.py");
    // PMAT-641: a runtime (variable) index wraps each level (`__aidx{n}`) like
    // Python negative indexing; an all-non-negative-literal path stays bare.
    assert!(
        rust.contains("grid[__aidx0 as usize][__aidx1 as usize] =")
            && rust.contains("g[__aidx0 as usize][__aidx1 as usize][__aidx2 as usize] ="),
        "nested index assign:\n{rust}"
    );
    let driver = r#"
fn main() {
    // nested (2D/3D) subscript assignment; cross-checked vs python3.
    assert_eq!(diag_fill(3), 4); // grid[2][2]=3, grid[0][0]=1 → 4
    let g = vec![vec![vec![0i64; 2]; 2]; 2];
    assert_eq!(cube_set(g, 1, 1, 1, 7), 7);
}
"#;
    assert_rustc_runs("nested_index_assign", &rust, driver);
}

/// PMAT-502ex (Tranche 2): Optional epic cut 2+3 — `Optional[T]` **parameters**
/// and `x is None` / `x is not None` **tests**. An `Optional[T]` param lowers to
/// a Rust `Option<T>`; `x is None` → `(x).is_none()`, `x is not None` →
/// `(x).is_some()` (a new `Expr::IsNone`, bool-typed). The operand must type as
/// `Optional`. This is the narrowing-FREE consuming slice (the param is only
/// tested, never used as `T`). Cross-checked vs python3.
#[test]
fn optional_is_none() {
    let rust = xpile_transpile_to_rust("optional_is_none.py");
    assert!(
        rust.contains(".is_none()") && rust.contains(".is_some()") && rust.contains("Option<i64>"),
        "Optional params + is-None tests should emit Option<T> + is_none/is_some:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(is_absent(None));
    assert!(!is_absent(Some(5)));
    assert!(is_present(Some(5)));
    assert!(!is_present(None));
    assert_eq!(guard(None), -1);
    assert_eq!(guard(Some(3)), 0);
    assert!(both_none(None, None));
    assert!(!both_none(Some(1), None));
    assert!(str_present(Some("x".to_string())));
    assert!(!str_present(None));
}
"#;
    assert_rustc_runs("optional_is_none", &rust, driver);
}

/// PMAT-502ew (Tranche 2): `Optional[T]` **return type** (first cut of the
/// Optional epic). `-> Optional[int]` → Rust `Option<i64>`; the body produces
/// concrete `T` values and the return site wraps them — `return None` →
/// `None`, `return x` → `Some(x)` (via `Type::Optional` + `Expr::OptionExpr`).
/// `from typing import Optional` is accepted+skipped. Optional *parameters* /
/// locals and `is None` flow-narrowing are a deferred follow-up. Cross-checked
/// vs python3 (driver matches/unwraps the `Option`).
#[test]
fn optional_return() {
    let rust = xpile_transpile_to_rust("optional_return.py");
    assert!(
        rust.contains("-> Option<i64>") && rust.contains("Some(") && rust.contains("None"),
        "Optional return should emit Option<T> + Some/None:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(find_even(vec![1, 3, 4, 5]), Some(4));
    assert_eq!(find_even(vec![1, 3, 5]), None);
    assert_eq!(first_long(vec!["a".to_string(), "abcd".to_string()]), Some("abcd".to_string()));
    assert_eq!(first_long(vec!["a".to_string()]), None);
    assert_eq!(maybe_reciprocal(4.0), Some(0.25));
    assert_eq!(maybe_reciprocal(0.0), None);
    assert_eq!(always_none(), None);
    assert_eq!(always_some(7), Some(7));
}
"#;
    assert_rustc_runs("optional_return", &rust, driver);
}

/// PMAT-502ey (Tranche 2): 1-arg `d.get(k)` → `Optional[V]` (continuing the
/// Optional epic). `d.get(k)` (no default) lowers to `Expr::DictGetOpt` →
/// `(d).get(&(k)).cloned()` : `Option<V>`; the 2-arg `d.get(k, default)` form is
/// unchanged. Also locks in the no-double-wrap return fix: returning an already
/// -`Optional` value (an `Optional` param, or another `.get(k)`) passes through
/// verbatim instead of re-wrapping into `Some(Option<..>)`. Cross-checked vs
/// python3 (driver matches/unwraps the `Option`).
#[test]
fn dict_get_optional() {
    let rust = xpile_transpile_to_rust("dict_get_optional.py");
    assert!(
        rust.contains(".get(&(k)).cloned()\n") && rust.contains("-> Option<i64>"),
        "1-arg dict get should emit `.get(&(k)).cloned()` : Option<V>:\n{rust}"
    );
    // The Optional param must pass through verbatim — not be re-wrapped.
    assert!(
        rust.contains("-> Option<i64> {\n    x\n}"),
        "returning an Optional value must not double-wrap into Some(..):\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 5i64);
    assert_eq!(lookup(d.clone(), String::from("a")), Some(5i64));
    assert_eq!(lookup(d.clone(), String::from("z")), None);
    assert_eq!(lookup_or(d.clone(), String::from("a")), 5i64);
    assert_eq!(lookup_or(d.clone(), String::from("z")), -1i64);
    assert_eq!(passthrough(Some(7)), Some(7));
    assert_eq!(passthrough(None), None);
}
"#;
    assert_rustc_runs("dict_get_optional", &rust, driver);
}

/// PMAT-618: comparing a no-default `d.get(k)` (`Option<T>`) with a bare value.
/// `d.get(k) == 5` emitted `Option<i64> == i64` (E0308). Python returns `None`
/// when the key is absent (`None == 5` is `False`), modeled exactly as
/// `Option<T> == Some(5)` — the bare side is wrapped in `Some`. Found by
/// differential hunt #7 (H7-7). Cross-checked vs python3.
#[test]
fn dict_get_compare() {
    let rust = xpile_transpile_to_rust("dict_get_compare.py");
    assert!(
        rust.contains(".get(&(k)).cloned() == Some(5i64)")
            && rust.contains(".get(&(k)).cloned() != Some(5i64)"),
        "d.get(k) ==/!= v should wrap the value side in Some(..):\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 5i64);
    d.insert(String::from("b"), 3i64);
    // present+match, present+nomatch, absent
    assert_eq!(eq5(d.clone(), String::from("a")), true);
    assert_eq!(eq5(d.clone(), String::from("b")), false);
    assert_eq!(eq5(d.clone(), String::from("z")), false);
    assert_eq!(ne5(d.clone(), String::from("a")), false);
    assert_eq!(ne5(d.clone(), String::from("b")), true);
    assert_eq!(ne5(d.clone(), String::from("z")), true);
}
"#;
    assert_rustc_runs("dict_get_compare", &rust, driver);
}

/// PMAT-620: a no-default `d.get(k)` in an f-string field is `Option<T>`, which
/// has no `Display` — `f"{d.get(k)}"` emitted `format!("{}", Option)` (E0308:
/// transpile-success → invalid Rust). `str(d.get(k))` / `print(d.get(k))` already
/// reject a bare Optional, so the f-string case now rejects too (fail-loud,
/// consistent). The supported forms (`d.get(k, default)`, `d[k]`) still work.
/// Found by differential hunt #7 (H7-9). Cross-checked vs python3.
#[test]
fn fstring_dict_get_optional_rejected() {
    let py = fixture("fstring_dict_get_rejected.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "f-string interpolation of a bare `d.get(k)` (Optional) must be refused"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Optional") && stderr.contains("d.get(k, <default>)"),
        "the rejection should explain the Optional + suggest a default:\n{stderr}"
    );
    // The supported forms (`d.get(k, default)`, `d[k]`) still transpile + run.
    let rust = xpile_transpile_to_rust("fstring_dict_get_ok.py");
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 7i64);
    assert_eq!(with_default(d.clone(), String::from("a")), String::from("val=7"));
    assert_eq!(with_default(d.clone(), String::from("z")), String::from("val=0"));
    assert_eq!(index(d.clone(), String::from("a")), String::from("val=7"));
}
"#;
    assert_rustc_runs("fstring_dict_get_ok", &rust, driver);
}

/// PMAT-708: a bare dict (or set) interpolated in an f-string emitted
/// `format!("{}", hashmap)` → E0277 (HashMap/HashSet have no Display). It is now
/// rejected at lowering — parity with `str()`/`print()`/`.format()`/`%` over a
/// dict/set; iteration order is also non-deterministic (PMAT-537). (HUNT-V8 V8-10.)
#[test]
fn fstring_dict_rejected() {
    let py = fixture("fstring_dict_rejected.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "a bare dict in an f-string must be refused (HashMap has no Display)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("dict/set in an f-string") && stderr.contains("non-deterministic"),
        "the rejection should explain the no-Display / order issue:\n{stderr}"
    );
}

/// PMAT-602: a non-Optional annotation over an Optional initializer (1-arg
/// `d.get(k)`) is a type lie that would emit `Option<i64>` into an `i64`
/// binding (E0308). xpile rejects it cleanly (transpile-success ⟹ valid Rust),
/// since Python doesn't enforce annotations and unwrapping would diverge on the
/// None case. The correct forms (`d.get(k, default)` / `Optional[...]`) still
/// transpile. Cross-checked vs python3: `with_default({"a":7})==7`, `{}` → 0.
#[test]
fn ann_optional_mismatch_rejected() {
    let py = fixture("ann_optional_mismatch.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "`x: int = d.get(k)` (Optional into non-Optional) must be refused"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("initializer is Optional") && stderr.contains("d.get(k, default)"),
        "the rejection should explain the Optional mismatch:\n{stderr}"
    );
    // The correct 2-arg form still transpiles + runs.
    let rust = xpile_transpile_to_rust("ann_optional_ok.py");
    let driver = r#"
fn main() {
    use std::collections::HashMap;
    let mut d: HashMap<String, i64> = HashMap::new();
    d.insert("a".to_string(), 7);
    assert_eq!(with_default(d), 7);
    let e: HashMap<String, i64> = HashMap::new();
    assert_eq!(with_default(e), 0);
}
"#;
    assert_rustc_runs("ann_optional_ok", &rust, driver);
}

/// PMAT-502ez (Tranche 2): Optional **flow-narrowing** (Optional epic cut 4).
/// After a provably-exiting `if x is None: return …` / `raise` guard, a later
/// read of `x` lowers to `Expr::OptionUnwrap` → `(x).unwrap()` : `T`, so the
/// dominant Python Optional idiom (guard-then-use) transpiles to compilable
/// Rust. Narrowing is sound: only non-reassigned `Optional` params guarded by an
/// always-exiting None-check are narrowed. Works for multiple stacked guards,
/// str payloads, and `raise`-exiting guards. Cross-checked vs python3.
#[test]
fn optional_narrow() {
    let rust = xpile_transpile_to_rust("optional_narrow.py");
    assert!(
        rust.contains("(x).unwrap()") && rust.contains("(name).unwrap()"),
        "a narrowed Optional read should emit `(x).unwrap()`:\n{rust}"
    );
    // The guard itself still emits the `is_none()` test (cut3), unchanged.
    assert!(
        rust.contains("if (x).is_none()"),
        "the None-guard should still emit `(x).is_none()`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(inc_or_zero(Some(5)), 6);
    assert_eq!(inc_or_zero(None), 0);
    assert_eq!(double_guard(Some(3), Some(4)), 7);
    assert_eq!(double_guard(None, Some(1)), -1);
    assert_eq!(double_guard(Some(2), None), -2);
    assert_eq!(label(Some("bob".to_string())), "bob!");
    assert_eq!(label(None), "anon");
    assert_eq!(via_raise(Some(7)), 14);
}
"#;
    assert_rustc_runs("optional_narrow", &rust, driver);
}

/// PMAT-502fa (Tranche 2): Optional **intra-branch narrowing** for `if x is not
/// None:` (complement of the cut-4 early-return guard). Inside the then-body, a
/// read of `x` lowers to `Expr::OptionUnwrap` → `(x).unwrap()` : `T`, so the
/// `if x is not None: <use x>` idiom transpiles to compilable Rust. Narrowing is
/// scoped to the then-body (restored afterwards) and only applies to a
/// non-reassigned `Optional` name; it persists into nested statements (a loop)
/// within the branch. Cross-checked vs python3.
#[test]
fn optional_narrow_branch() {
    let rust = xpile_transpile_to_rust("optional_narrow_branch.py");
    assert!(
        rust.contains("if (x).is_some()") && rust.contains("(x).unwrap()"),
        "`is not None` then-branch should test is_some() and unwrap x inside:\n{rust}"
    );
    assert!(
        rust.contains("(name).unwrap()"),
        "narrowing should apply to str payloads too:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(safe_inc(Some(5)), 6);
    assert_eq!(safe_inc(None), 0);
    assert_eq!(shout(Some("hi".to_string())), "hi!");
    assert_eq!(shout(None), "?");
    assert_eq!(sum_to(Some(4)), 6);
    assert_eq!(sum_to(None), 0);
}
"#;
    assert_rustc_runs("optional_narrow_branch", &rust, driver);
}

/// PMAT-502fb (Tranche 2): bitwise invert `~x`. Python's `~x` is the exact
/// identity `-(x + 1)`, which is precisely Rust's `!x` on a signed integer
/// (`~5 == -6` in both). Lowers to `UnOp::BitNot` → `(!(x))`; requires an I64
/// operand. Cross-checked vs python3 (including the `~~a == a` involution and a
/// realistic `n & ~mask`).
#[test]
fn bit_invert() {
    let rust = xpile_transpile_to_rust("bit_invert.py");
    assert!(
        rust.contains("(!(a))"),
        "`~a` should lower to Rust `(!(a))`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(invert(5), -6);
    assert_eq!(invert(-3), 2);
    assert_eq!(invert_expr(6, 3), -3);
    assert_eq!(double_invert(9), 9);
    assert_eq!(mask_complement(255), 248);
}
"#;
    assert_rustc_runs("bit_invert", &rust, driver);
}

/// PMAT-503b (exceptions epic): value-producing `try`/`except` →
/// `catch_unwind`. xpile models Python exceptions as Rust panics
/// (ZeroDivisionError via the floor-div `.expect`, IndexError via list
/// indexing, KeyError via HashMap indexing), so `try: return <expr> except
/// [E]: return <expr>` lowers to `Expr::TryCatch` → a
/// `std::panic::catch_unwind(AssertUnwindSafe(|| <body>))` match that runs the
/// handler on `Err`. Cross-checked vs python3 (the driver installs a no-op
/// panic hook so the expected, caught panics don't spam stderr).
#[test]
fn try_except_catches_panics() {
    let rust = xpile_transpile_to_rust("try_except.py");
    assert!(
        rust.contains("catch_unwind") && rust.contains("Err(_) =>"),
        "try/except should lower to a catch_unwind match:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(safe_div(10, 3), 3);
    assert_eq!(safe_div(7, 0), -1);
    assert_eq!(safe_index(vec![5, 6, 7], 2), 7);
    assert_eq!(safe_index(vec![5, 6, 7], 99), 0);
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 1i64);
    assert_eq!(safe_lookup(d.clone(), String::from("a")), 1);
    assert_eq!(safe_lookup(d.clone(), String::from("z")), -1);
}
"#;
    assert_rustc_runs("try_except", &rust, driver);
}

/// PMAT-503c (exceptions epic): statement-position **assignment-form**
/// `try`/`except` — `try: x = <expr> except [E]: x = <expr>` → `let x = match
/// catch_unwind(|| <body>) { Ok(v)=>v, Err(_)=><handler> }` (reuses
/// `Expr::TryCatch`; the closure produces the value, so no closure-mutation
/// hazard). Covers a fresh binding (`v`) and a reassignment of an already-bound
/// `mut` name (`base`, read inside both arms). The mutability pre-walk now
/// descends into try arms so the reassigned name is marked `mut`. Cross-checked
/// vs python3.
#[test]
fn try_except_assignment_form() {
    let rust = xpile_transpile_to_rust("try_except_assign.py");
    assert!(
        rust.contains("let v") && rust.contains("catch_unwind"),
        "fresh-binding try-assign should emit `let v = catch_unwind match`:\n{rust}"
    );
    assert!(
        rust.contains("let mut base"),
        "a reassigned try-target must be `let mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 5i64);
    assert_eq!(lookup(d.clone(), String::from("a")), 5);
    assert_eq!(lookup(d.clone(), String::from("z")), -1);
    assert_eq!(accumulate(vec![7, 8, 9], 1), 18);
    assert_eq!(accumulate(vec![7, 8, 9], 50), 20);
}
"#;
    assert_rustc_runs("try_except_assign", &rust, driver);
}

/// PMAT-505a (classes epic, first cut): a `@dataclass` / field-only class →
/// a Rust `#[derive(Clone, Debug, PartialEq)] pub struct` with `pub` fields in
/// declaration order. This first cut emits the struct *definition* only;
/// construction (`Point(1, 2)`) and field access (`p.x`) are a follow-up
/// sub-slice (they need a `Type::Struct`). The driver constructs the emitted
/// structs in hand-written Rust and exercises the derived traits, verifying the
/// definition is correct and usable.
#[test]
fn dataclass_struct_definition() {
    let rust = xpile_transpile_to_rust("dataclass_def.py");
    assert!(
        rust.contains("#[derive(Clone, Debug, PartialEq)]")
            && rust.contains("pub struct Point {")
            && rust.contains("pub x: i64,")
            && rust.contains("pub items: Vec<i64>,"),
        "dataclass should emit a derived pub struct with pub fields:\n{rust}"
    );
    let driver = r#"
fn main() {
    let p = Point { x: 1, y: 2 };
    assert_eq!(p.x, 1);
    assert_eq!(p.y, 2);
    assert_eq!(p.clone(), p);            // Clone + PartialEq
    let _ = format!("{:?}", p);          // Debug
    let t = Tagged { label: "k".to_string(), count: 3, ratio: 0.5, items: vec![1, 2] };
    assert_eq!(t.count, 3);
    assert_eq!(t.items, vec![1, 2]);
}
"#;
    assert_rustc_runs("dataclass_def", &rust, driver);
}

/// PMAT-592 (classes epic): a `@dataclass(frozen=True)` is hashable in Python,
/// so it may be a set element or dict key. The struct must derive `Eq + Hash`
/// (a bare `#[derive(Clone, Debug, PartialEq)]` struct is rejected as a
/// `HashSet` element / `HashMap` key — E0277/E0599). Derived only when every
/// field is itself Eq+Hash-capable (`i64`/`bool`/`String`). Cross-checked vs
/// python3: `count_unique()==2` (Coord(1,2) dedupes), `dict_key_lookup()==100`.
#[test]
fn dataclass_eq_hash() {
    let rust = xpile_transpile_to_rust("dataclass_eq_hash.py");
    assert!(
        rust.contains("#[derive(Clone, Debug, PartialEq, Eq, Hash)]"),
        "frozen dataclass with Eq+Hash-capable fields must derive Eq+Hash:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(count_unique(), 2);
    assert_eq!(dict_key_lookup(), 100);
}
"#;
    assert_rustc_runs("dataclass_eq_hash", &rust, driver);
}

/// PMAT-648: `@dataclass(order=True)` derives `PartialOrd` so instance
/// comparisons (`<`/`<=`/`>`/`>=`) compile — they were rejected (E0369, no
/// PartialOrd). Lexicographic by field order = Python's tuple comparison; works
/// for float fields too (PartialOrd, not Ord). Cross-checked vs python3.
#[test]
fn dataclass_order() {
    let rust = xpile_transpile_to_rust("dataclass_order.py");
    assert!(
        rust.contains("#[derive(Clone, Debug, PartialEq, PartialOrd)]"),
        "order=True dataclass must derive PartialOrd:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(less(1, 2, 1, 3), 1);  // (1,2) < (1,3)
    assert_eq!(less(2, 0, 1, 9), 0);  // (2,0) < (1,9) is false
    assert_eq!(ge(5, 3), 1);
    assert_eq!(float_less(), 1);
}
"#;
    assert_rustc_runs("dataclass_order", &rust, driver);
}

/// PMAT-506b (classes epic): dataclass **construction + field access**.
/// Positional `Name(a, b)` → `Expr::StructLit` (`Name { f0: a, f1: b }`);
/// `obj.field` → `Expr::FieldAccess` (`(obj).field`); struct-typed params,
/// returns, and locals via a new `Type::Struct`. Cross-checked vs python3.
#[test]
fn dataclass_construction_and_field_access() {
    let rust = xpile_transpile_to_rust("dataclass_use.py");
    assert!(
        rust.contains("Point { x: a, y: b }") && rust.contains("(p).x"),
        "construction → struct literal + field read → (obj).field:\n{rust}"
    );
    assert!(
        rust.contains("-> Point") && rust.contains("p: Point"),
        "struct-typed return + param should emit the bare struct name:\n{rust}"
    );
    let driver = r#"
fn main() {
    let m = make(3, 4);
    assert_eq!(m.x, 3);
    assert_eq!(m.y, 4);
    assert_eq!(dist_sq(m), 25);
    assert_eq!(origin_sum(), 7);
    assert_eq!(label_len(Labeled { name: "hi".to_string(), value: 5 }), 7);
}
"#;
    assert_rustc_runs("dataclass_use", &rust, driver);
}

/// PMAT-506c (classes epic): struct **field assignment** `obj.field = value` →
/// `Stmt::FieldAssign` → `(obj).field = value;`. The mutated struct binding is
/// marked `mut` by the pre-walk (a struct param becomes `mut p: P`). Cross-
/// checked vs python3.
#[test]
fn dataclass_field_assignment() {
    let rust = xpile_transpile_to_rust("dataclass_field_assign.py");
    assert!(
        rust.contains("(c).value = ") && rust.contains("mut c: Counter"),
        "field assignment should emit `(c).field = …` with a `mut` receiver:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(advance(Counter { value: 10, step: 2 }, 3), 16);
    assert_eq!(reset_and_set(Counter { value: 99, step: 99 }), 1);
}
"#;
    assert_rustc_runs("dataclass_field_assign", &rust, driver);
}

/// PMAT-506d (classes epic): dataclass **methods** — `def m(self, …)` →
/// `impl Name { pub fn m(&self, …) … }`, called as `obj.m(args)` →
/// `Expr::MethodCall` → `(obj).m(args)`. The `self` receiver emits as `&self`
/// and types as the struct (so `self.field` / `self.other_method()` work).
/// Read-only first cut. Cross-checked vs python3.
#[test]
fn dataclass_methods() {
    let rust = xpile_transpile_to_rust("dataclass_methods.py");
    assert!(
        rust.contains("impl Rect {")
            && rust.contains("pub fn area(&self) -> i64")
            && rust.contains("(r).area()"),
        "methods should emit an impl block with &self + method-call dispatch:\n{rust}"
    );
    let driver = r#"
fn main() {
    let r = Rect { w: 3, h: 4 };
    assert_eq!(r.area(), 12);
    assert_eq!(r.scaled_area(2), 24);
    assert_eq!(total(r), 36);
}
"#;
    assert_rustc_runs("dataclass_methods", &rust, driver);
}

/// PMAT-506e (classes epic): dataclass **keyword construction** —
/// `Point(x=1, y=2)`, mixed `Point(10, y=20)`, and reordered `Point(y=5, x=3)`
/// all map to `Point { x: …, y: … }` (fields emitted in declaration order).
/// Cross-checked vs python3.
#[test]
fn dataclass_keyword_construction() {
    let rust = xpile_transpile_to_rust("dataclass_kwargs.py");
    assert!(
        rust.contains("P") && rust.contains("Point { x: 1i64, y: 2i64 }"),
        "keyword construction should emit fields in declaration order:\n{rust}"
    );
    assert!(
        rust.contains("Point { x: 3i64, y: 5i64 }"),
        "reordered keywords must still emit in field order:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(all_kw(), 3);
    assert_eq!(mixed(), 30);
    assert_eq!(reordered(), -2);
}
"#;
    assert_rustc_runs("dataclass_kwargs", &rust, driver);
}

/// PMAT-506f (classes epic): dataclass **field defaults** `x: T = <literal>`.
/// Construction omitting a defaulted field fills it from the literal default
/// (lowered in the pre-pass): `Config()` → all defaults; `Config(timeout=5)` →
/// override one, default the rest. Cross-checked vs python3.
#[test]
fn dataclass_field_defaults() {
    let rust = xpile_transpile_to_rust("dataclass_defaults.py");
    assert!(
        rust.contains("Config { timeout: 30i64, retries: 3i64, name: String::from(\"default\") }"),
        "omitted fields should be filled from their literal defaults:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(all_defaults(), 33);
    assert_eq!(partial(), 8);
    assert_eq!(named_override(), "custom");
}
"#;
    assert_rustc_runs("dataclass_defaults", &rust, driver);
}

/// PMAT-506g (classes epic): dataclass **`@staticmethod`** — a method with no
/// `self` receiver lowers to a plain associated function `pub fn m(args)` inside
/// the `impl` block, and a call `Class.method(args)` lowers to
/// `Class::method(args)` (reusing `Expr::Call` with a qualified callee — no new
/// IR). An instance method may call a static method via the class name.
/// Cross-checked vs python3 (add=9, triple=21, boosted=40).
#[test]
fn dataclass_staticmethod() {
    let rust = xpile_transpile_to_rust("dataclass_staticmethod.py");
    assert!(
        rust.contains("impl MathBox {")
            && rust.contains("pub fn add(a: i64, b: i64) -> i64")
            && rust.contains("MathBox::add(a, b)"),
        "static methods should emit a no-self assoc fn + `Class::method` call sites:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(use_add(4, 5), 9);
    assert_eq!(use_triple(7), 21);
    assert_eq!(use_boosted(10), 40);
}
"#;
    assert_rustc_runs("dataclass_staticmethod", &rust, driver);
}

/// PMAT-506g (classes epic — correctness): calling an *instance* method via the
/// class name (`Box.get(5)`, Python's unbound-method form) is REJECTED with a
/// clear diagnostic rather than emitting `Box::get(5)` (an associated fn lacking
/// the required `&self` receiver). Only `@staticmethod`s are reachable via the
/// `Class.method(...)` form, upholding "transpile-success ⟹ valid Rust".
#[test]
fn staticmethod_instance_via_class_is_rejected() {
    let py = fixture("staticmethod_instance_via_class_rejected.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "calling an instance method via the class name must be refused"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not a `@staticmethod`"),
        "the rejection should name the staticmethod requirement:\n{stderr}"
    );
}

/// PMAT-506h (classes epic): dataclass **`@classmethod`** — a method with a
/// `cls` receiver lowers to a no-receiver associated function (the `cls` param
/// is dropped); `cls(...)` in the body constructs the enclosing class and
/// `cls.method(...)` calls a sibling static/class method, both resolved via the
/// enclosing class name. Called as `Class.method(args)` → `Class::method(args)`
/// (the same dispatch as `@staticmethod`, no new IR). Cross-checked vs python3
/// (origin_sum=0, diagonal_sum(5)=10, unit_sum=2).
#[test]
fn dataclass_classmethod() {
    let rust = xpile_transpile_to_rust("dataclass_classmethod.py");
    assert!(
        rust.contains("pub fn origin() -> Point")
            && rust.contains("Point { x: 0i64, y: 0i64 }")
            && rust.contains("Point::diagonal(1i64)"),
        "classmethod `cls(...)` should construct the class; `cls.m()` → `Class::m()`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(origin_sum(), 0);
    assert_eq!(diagonal_sum(5), 10);
    assert_eq!(unit_sum(), 2);
}
"#;
    assert_rustc_runs("dataclass_classmethod", &rust, driver);
}

/// PMAT-506i (classes epic): augmented struct field assignment
/// `obj.field <op>= v` → `obj.field = obj.field <op> v`, reusing the shipped
/// `FieldAccess` read + `FieldAssign` write (PMAT-506c). The receiver is marked
/// `mut` by the pre-walk (an Attribute aug-target now counts). Common as
/// `account.balance += deposit`. Cross-checked vs python3 (145, 13, 28).
#[test]
fn dataclass_augmented_field_assign() {
    let rust = xpile_transpile_to_rust("dataclass_aug_field.py");
    assert!(
        rust.contains("let mut a: Account")
            && rust.contains("(a).balance = ((a).balance).checked_add(d1)"),
        "`obj.field += v` should desugar to a FieldAssign of a FieldAccess + op, receiver `mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(run_deposits(20, 30), 145);
    assert_eq!(scale_bonus(4), 13);
    assert_eq!(combined(10), 28);
}
"#;
    assert_rustc_runs("dataclass_aug_field", &rust, driver);
}

/// PMAT-506j (classes epic): dataclass **`@property`** — a read-only `self`
/// method accessed as a bare attribute (`r.area`, no parens) lowers to a no-arg
/// method call `(r).area()` (an `Expr::MethodCall`; only registered properties
/// auto-call, so a bare non-property access stays an error). Properties are
/// usable on `self` from another method too. Cross-checked vs python3
/// (area=12, perimeter=14, describe=26).
#[test]
fn dataclass_property() {
    let rust = xpile_transpile_to_rust("dataclass_property.py");
    assert!(
        rust.contains("pub fn area(&self) -> i64")
            && rust.contains("(r).area()")
            && rust.contains("(self).area()"),
        "a `@property` should emit as a `&self` method and a bare read should call it:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(area_of(3, 4), 12);
    assert_eq!(perimeter_of(3, 4), 14);
    assert_eq!(described(3, 4), 26);
}
"#;
    assert_rustc_runs("dataclass_property", &rust, driver);
}

/// PMAT-510 (Tranche 2): the `match` statement — the literal-dispatch subset
/// (`case <literal>:` + a trailing `case _:`, Name subject) desugars to an
/// `if`/`elif`/`else` chain, reusing all existing `if` lowering (no new IR).
/// Works as a terminal (each case returns → an if-expression) and in statement
/// position (assignment bodies; `walk_counts` descends into cases so a
/// case-assigned name is `mut`). Cross-checked vs python3.
#[test]
fn match_statement() {
    let rust = xpile_transpile_to_rust("match_stmt.py");
    assert!(
        rust.contains("if (n == 0i64) { 100i64 } else if (n == 1i64)")
            && rust.contains("letter == String::from(\"A\")")
            && rust.contains("let mut result"),
        "match should desugar to an if/elif/else chain (terminal + statement form):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(classify(0), 100);
    assert_eq!(classify(1), 200);
    assert_eq!(classify(-1), 300);
    assert_eq!(classify(7), 0);
    assert_eq!(grade_points("A".to_string()), 4);
    assert_eq!(grade_points("B".to_string()), 3);
    assert_eq!(grade_points("C".to_string()), 0);
    assert_eq!(step(0, 5), 6);
    assert_eq!(step(1, 5), 10);
    assert_eq!(step(9, 5), 5);
}
"#;
    assert_rustc_runs("match_stmt", &rust, driver);
}

/// PMAT-512 (Tranche 2): `match` **`|`-patterns** (`case 0 | 1 | 2:`) — an
/// or-pattern of literal alternatives desugars to an OR of equality tests
/// (`subject == 0 || subject == 1 || …`), extending the match→if desugar (no
/// new IR). Works over int and str literals, terminal + statement position.
/// Cross-checked vs python3 (day_kind 0/0/1/-1, vowel_score 2/1).
#[test]
fn match_or_pattern() {
    let rust = xpile_transpile_to_rust("match_or_pattern.py");
    assert!(
        rust.contains("if ((d == 5i64) || (d == 6i64))"),
        "an `|`-pattern should become an OR of equality tests:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(day_kind(5), 0);
    assert_eq!(day_kind(6), 0);
    assert_eq!(day_kind(2), 1);
    assert_eq!(day_kind(9), -1);
    assert_eq!(vowel_score("a".to_string()), 2);
    assert_eq!(vowel_score("z".to_string()), 1);
}
"#;
    assert_rustc_runs("match_or_pattern", &rust, driver);
}

/// PMAT-513 (Tranche 2): a Python `class C(Enum):` with `NAME = <int literal>`
/// members → a Rust `enum`. Member access `C.NAME` → `C::NAME`
/// (`Expr::EnumVariant`); the compile-time-known `C.NAME.value` lowers to its
/// discriminant literal. Enum-typed params/locals + member equality work
/// (the enum reuses `Type::Struct` at use sites). Cross-checked vs python3
/// (red=1, blue=3, is_go true/false, passthrough=10).
#[test]
fn enum_basic() {
    let rust = xpile_transpile_to_rust("enum_basic.py");
    assert!(
        rust.contains("pub enum Color {")
            && rust.contains("(s == Signal::GO)")
            && rust.contains("let c: Color = Color::GREEN"),
        "an Enum class should emit a Rust enum + `C::NAME` member access:\n{rust}"
    );
    // `Color.RED.value` is the compile-time discriminant literal; PMAT-515:
    // `Color.GREEN.name` is the compile-time variant-name string.
    assert!(
        rust.contains("pub fn red_value() -> i64 {\n    1i64\n}")
            && rust.contains("String::from(\"GREEN\")"),
        "`C.NAME.value`/`.name` should lower to the discriminant / name literal:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(red_value(), 1);
    assert_eq!(blue_value(), 3);
    assert_eq!(green_name(), "GREEN");
    assert_eq!(is_go(Signal::GO), true);
    assert_eq!(is_go(Signal::STOP), false);
    assert_eq!(passthrough(), 10);
}
"#;
    assert_rustc_runs("enum_basic", &rust, driver);
}

/// PMAT-516 (Tranche 2 — correctness): `s.startswith((a, b, …))` /
/// `.endswith((…))` — Python accepts a tuple of prefixes/suffixes (true if any
/// matches). Previously this transpiled to `…starts_with(&(a, b)[..])`
/// (transpile-success-but-INVALID-Rust). Now expands to an OR of per-prefix
/// `starts_with`/`ends_with` checks. The 1-arg form is unaffected.
/// Cross-checked vs python3 (url_kind 1/2/0, is_source 1/0, single 1/0).
#[test]
fn str_startswith_endswith_tuple() {
    let rust = xpile_transpile_to_rust("str_startswith_tuple.py");
    assert!(
        rust.contains("s.starts_with(&(String::from(\"http://\"))[..]) || s.starts_with(&(String::from(\"https://\"))[..])"),
        "a tuple of prefixes should expand to an OR of starts_with checks:\n{rust}"
    );
    let driver = r##"
fn main() {
    assert_eq!(url_kind("https://x".to_string()), 1);
    assert_eq!(url_kind("ftp://y".to_string()), 2);
    assert_eq!(url_kind("mailto:z".to_string()), 0);
    assert_eq!(is_source("a.py".to_string()), 1);
    assert_eq!(is_source("a.txt".to_string()), 0);
    assert_eq!(single_prefix("# hi".to_string()), 1);
    assert_eq!(single_prefix("hi".to_string()), 0);
}
"##;
    assert_rustc_runs("str_startswith_tuple", &rust, driver);
}

/// PMAT-517 (Tranche 2): `str.replace(old, new, count)` (3-arg) → Rust
/// `s.replacen(...)` — replace the first `count` occurrences. The 2-arg form is
/// unchanged. Cross-checked vs python3 ("bXnana", "f00booloo", "ZZZ").
#[test]
fn str_replace_count() {
    let rust = xpile_transpile_to_rust("str_replace_count.py");
    assert!(
        rust.contains(
            ".replacen(&(String::from(\"a\"))[..], &(String::from(\"X\"))[..], (1i64) as usize)"
        ),
        "3-arg replace should emit `replacen`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(replace_first("banana".to_string()), "bXnana");
    assert_eq!(replace_two("foobooloo".to_string()), "f00booloo");
    assert_eq!(replace_all("zzz".to_string()), "ZZZ");
}
"#;
    assert_rustc_runs("str_replace_count", &rust, driver);
}

/// PMAT-518 + PMAT-621: `str.split(sep, maxsplit)` (2-arg) → Rust
/// `s.splitn(maxsplit + 1, sep)` (Python caps the number of *splits*, so the
/// part count is `maxsplit + 1`). PMAT-621: a NEGATIVE maxsplit means "no limit",
/// but `(maxsplit as usize) + 1` wrapped to 0 (`usize::MAX + 1`) → zero parts;
/// `saturating_add(1)` keeps it at `usize::MAX` → all parts. The 1-arg form is
/// unchanged. Cross-checked vs python3 (incl. `split(",", -1)` → all parts).
#[test]
fn str_split_maxsplit() {
    let rust = xpile_transpile_to_rust("str_split_maxsplit.py");
    assert!(
        rust.contains(".splitn(((1i64) as usize).saturating_add(1), &(String::from(\"=\"))[..])"),
        "2-arg split should emit `splitn(maxsplit.saturating_add(1), sep)`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(key_part("a=b=c".to_string()), "a");
    assert_eq!(value_part("a=b=c".to_string()), "b=c");
    assert_eq!(field_count("x,y,z,w".to_string()), 4);
    assert_eq!(capped_count("x,y,z,w".to_string()), 3);
    // PMAT-621: negative maxsplit splits on every occurrence (no limit).
    assert_eq!(neg_count("x,y,z,w".to_string()), 4);
}
"#;
    assert_rustc_runs("str_split_maxsplit", &rust, driver);
}

/// PMAT-644: `str.rsplit(sep, maxsplit)` — split from the RIGHT (cap at maxsplit
/// splits), parts in left-to-right order. Rust `rsplitn` yields right-to-left,
/// so the codegen reverses the collected Vec. Bare `rsplit(sep)` maps to `split`.
/// Cross-checked vs python3.
#[test]
fn str_rsplit_maxsplit() {
    let rust = xpile_transpile_to_rust("str_rsplit_maxsplit.py");
    assert!(
        rust.contains(".rsplitn(") && rust.contains(".into_iter().rev().collect::<Vec<String>>()"),
        "rsplit should emit reversed `rsplitn`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(last_two(String::from("a/b/c/d")), "a/b|c|d"); // 2 splits from right
    assert_eq!(strip_ext(String::from("a.b.c")), "a.b");
    assert_eq!(no_limit(String::from("a.b.c")), "a|b|c");      // neg maxsplit = all
    assert_eq!(bare(String::from("a.b.c")), 3);                // bare == split
}
"#;
    assert_rustc_runs("str_rsplit_maxsplit", &rust, driver);
}

/// PMAT-519 (Tranche 2 — correctness): `frozenset(iterable)` — Rust has no
/// frozen set, so it maps to a `HashSet` (an immutable set is one that's never
/// mutated), routed through the same `SetFromList` path as `set(...)`. Previously
/// a silent miscompile (emitted an undefined `frozenset(...)` call). Cross-checked
/// vs python3 (unique_count 3, has_member true/false, vowels_present 3).
#[test]
fn frozenset_basic() {
    let rust = xpile_transpile_to_rust("frozenset_basic.py");
    assert!(
        !rust.contains("frozenset(") && rust.contains("collect::<std::collections::HashSet<_>>"),
        "frozenset should lower to a HashSet, not an undefined `frozenset(...)` call:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(unique_count(vec![3, 3, 1, 2, 2]), 3);
    assert_eq!(has_member(vec![1, 2, 3], 2), true);
    assert_eq!(has_member(vec![1, 2, 3], 9), false);
    assert_eq!(vowels_present("hello world".to_string()), 3);
}
"#;
    assert_rustc_runs("frozenset_basic", &rust, driver);
}

/// PMAT-520 (Tranche 2 — correctness): `list(set(...))` / `sorted(set(...))` —
/// materialise a set back to a `Vec` (`Expr::SetToList`). Previously a silent
/// miscompile: the nested `set(...)` fell through to context-free lowering and
/// emitted an undefined `set(...)`/`list(...)` call. Cross-checked vs python3
/// (unique_count 4, smallest 1, largest 3, desc_first 3).
#[test]
fn list_sorted_of_set() {
    let rust = xpile_transpile_to_rust("list_sorted_of_set.py");
    assert!(
        !rust.contains("list(set(")
            && !rust.contains("sorted(set(")
            && rust.contains(
                ".collect::<std::collections::HashSet<_>>().iter().cloned().collect::<Vec<_>>()"
            ),
        "list/sorted of a set should materialise to a Vec, not emit undefined calls:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(unique_count(vec![3, 3, 1, 2, 2, 5]), 4);
    assert_eq!(smallest_unique(vec![3, 3, 1, 2]), 1);
    assert_eq!(largest_unique(vec![3, 3, 1, 2]), 3);
    assert_eq!(sorted_desc_first(vec![3, 3, 1, 2]), 3);
}
"#;
    assert_rustc_runs("list_sorted_of_set", &rust, driver);
}

/// PMAT-521 (Tranche 2 — correctness): reduction builtins over a non-list
/// iterable — `sum(range(...))`, `sum/max/min(set(...))`. Previously silent
/// miscompiles: the arg (`range(...)` / `set(...)`) fell through to context-free
/// lowering and emitted undefined `range(...)`/`set(...)` calls. A shared
/// `materialize_iterable_arg` now turns `range(...)` into a Vec and a set into
/// `SetToList` before the reduce. Cross-checked vs python3 (10, 9, 6, 3, 1).
#[test]
fn reduce_over_iterable() {
    let rust = xpile_transpile_to_rust("reduce_over_iterable.py");
    assert!(
        !rust.contains("sum(range(") && !rust.contains("(set(") && !rust.contains("max(set"),
        "reductions over range/set should materialise the iterable, not emit undefined calls:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sum_range(5), 10);
    assert_eq!(sum_range_from(2, 5), 9);
    assert_eq!(sum_unique(vec![1, 1, 2, 3, 3]), 6);
    assert_eq!(max_unique(vec![3, 1, 2, 2]), 3);
    assert_eq!(min_unique(vec![3, 1, 2]), 1);
}
"#;
    assert_rustc_runs("reduce_over_iterable", &rust, driver);
}

/// PMAT-522 (Tranche 2 — correctness): builtins over a `range(...)` arg
/// (`len`/`sorted`/`reversed`) and `list(dict)`. All previously silent
/// miscompiles (the arg fell through to context-free → undefined `range(...)`/
/// `list(...)`). `range(...)` now materialises to a Vec via
/// `lower_arg_materializing_range`; `list(<dict>)` → the dict's keys.
/// Cross-checked vs python3 (5, 1, 4, 3).
#[test]
fn builtins_over_range_dict() {
    let rust = xpile_transpile_to_rust("builtins_over_range_dict.py");
    assert!(
        !rust.contains("len(range(") && !rust.contains("sorted(range(")
            && !rust.contains("reversed(range(") && !rust.contains("list(d"),
        "builtins over range/dict should materialise the iterable, not emit undefined calls:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(len_range(5), 5);
    assert_eq!(sorted_range_desc_first(5), 4);
    assert_eq!(reversed_range_first(5), 4);
    let mut d = std::collections::HashMap::new();
    d.insert("a".to_string(), 1);
    d.insert("b".to_string(), 2);
    d.insert("c".to_string(), 3);
    assert_eq!(dict_keys_count(d), 3);
}
"#;
    assert_rustc_runs("builtins_over_range_dict", &rust, driver);
}

/// PMAT-523 (Tranche 2): negative-step `range` materialisation —
/// `list(range(n, 0, -1))` / `sum(range(n, 0, -1))` etc. Python `range(start,
/// stop, step<0)` → Rust `((stop)+1 ..= (start)).rev().step_by(|step|)`. (The
/// counted `for i in range(n, 0, -1)` loop already worked; only materialisation
/// was deferred.) Cross-checked vs python3 (5, 1, 4, 15, 0).
#[test]
fn range_negative_step() {
    let rust = xpile_transpile_to_rust("range_negative_step.py");
    assert!(
        rust.contains(").rev().collect::<Vec<i64>>()") && rust.contains(") + 1)..=("),
        "a negative-step range should materialise via a reversed inclusive range:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(countdown_first(5), 5);
    assert_eq!(countdown_last(5), 1);
    assert_eq!(stride_neg3_count(10), 4);
    assert_eq!(sum_countdown(5), 15);
    assert_eq!(empty_neg(5), 0);
}
"#;
    assert_rustc_runs("range_negative_step", &rust, driver);
}

/// PMAT-524 (Tranche 2 — correctness): a `sorted`/`min`/`max` `key=` lambda that
/// indexes a tuple element (`key=lambda p: p[1]` over `list[tuple[..]]`).
/// Previously a silent miscompile: the key param `p` defaulted to `i64`, so
/// `p[1]` lowered to generic `[1]` indexing (invalid on a Rust tuple). The key
/// param now binds to the collection's element type, so `p[1]` → `(p).1`.
/// Cross-checked vs python3 (1, 3, 2, 3).
#[test]
fn sort_key_tuple_index() {
    let rust = xpile_transpile_to_rust("sort_key_tuple_index.py");
    assert!(
        rust.contains("(p).1") && !rust.contains("p[1i64 as usize]"),
        "a tuple-indexing sort key should lower to a `.1` field access:\n{rust}"
    );
    let driver = r#"
fn main() {
    let d = vec![(3, 9), (1, 2), (2, 5)];
    assert_eq!(sorted_by_second(d.clone()), 1);
    assert_eq!(max_by_second(d.clone()), 3);
    assert_eq!(min_by_first(d.clone()), 2);
    assert_eq!(sorted_desc_by_first(d.clone()), 3);
}
"#;
    assert_rustc_runs("sort_key_tuple_index", &rust, driver);
}

/// PMAT-525 (Tranche 2 — correctness): an expression-position comprehension /
/// generator / filter whose loop variable is a tuple or struct element. The
/// body was previously lowered with the loop var UNBOUND (→ default `i64`), so
/// `p[1]` over a tuple miscompiled and `p.x` over a struct was rejected.
/// `lower_comp_to_map` now binds the loop var to the iterable's element type
/// before lowering the body + filter. Cross-checked vs python3 (9, 16, 2, 7).
#[test]
fn comp_typed_element() {
    let rust = xpile_transpile_to_rust("comp_typed_element.py");
    assert!(
        rust.contains("(p).1") && !rust.contains("p[1i64 as usize]"),
        "a comprehension over tuple elements should lower `p[1]` to `.1`:\n{rust}"
    );
    let driver = r#"
fn main() {
    let d = vec![(3, 9), (1, 2), (2, 5)];
    assert_eq!(seconds_first(d.clone()), 9);
    assert_eq!(sum_seconds(d.clone()), 16);
    assert_eq!(count_big(d.clone()), 2);
    let ps = vec![Point { x: 1, y: 0 }, Point { x: 4, y: 0 }, Point { x: 2, y: 0 }];
    assert_eq!(sum_x(ps), 7);
}
"#;
    assert_rustc_runs("comp_typed_element", &rust, driver);
}

/// PMAT-526 (Tranche 2 — correctness): `map`/`filter` builtin lambdas indexing a
/// tuple element. Previously the lambda param was lowered unbound (→ `i64`), so
/// `map(lambda p: p[0] + p[1], ps)` miscompiled (generic `[..]` indexing on a
/// Rust tuple). The param now binds to the list's element type, so `p[0]` →
/// `.0`. Cross-checked vs python3 (12, 2, 5).
#[test]
fn map_filter_typed_param() {
    let rust = xpile_transpile_to_rust("map_filter_typed_param.py");
    assert!(
        rust.contains("(p).0") && rust.contains("(p).1") && !rust.contains("p[0i64 as usize]"),
        "map/filter lambda over tuple elements should lower `p[0]` to `.0`:\n{rust}"
    );
    let driver = r#"
fn main() {
    let d = vec![(3, 9), (1, 2), (2, 5)];
    assert_eq!(map_pair_sum_first(d.clone()), 12);
    assert_eq!(filter_big_count(d.clone()), 2);
    assert_eq!(map_pick_second(d.clone()), 5);
}
"#;
    assert_rustc_runs("map_filter_typed_param", &rust, driver);
}

/// PMAT-527 (Tranche 2): container truthiness in boolean conditions — `if xs:`,
/// `while q:`, `x if xs else y`, `not d`. Python treats a non-empty
/// list/dict/set/str as truthy; these now lower to `len(c) != 0` (and `not c` →
/// `len(c) == 0`), reusing `Len` + `BinOp` (no new IR). Cross-checked vs python3.
#[test]
fn container_truthiness() {
    let rust = xpile_transpile_to_rust("container_truthiness.py");
    assert!(
        rust.contains(".len() as i64) != 0i64") || rust.contains(".len() as i64 != 0i64"),
        "a list-truthy condition should lower to a `len(..) != 0` test:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(max_or_zero(vec![3, 1, 2]), 3);
    assert_eq!(max_or_zero(vec![]), 0);
    assert_eq!(first_or_default(vec!["a".to_string()]), "a");
    assert_eq!(first_or_default(vec![]), "none");
    assert_eq!(sum_drain(vec![1, 2, 3]), 6);
    let mut d = std::collections::HashMap::new();
    assert_eq!(is_empty_dict(d.clone()), true);
    d.insert("a".to_string(), 1);
    assert_eq!(is_empty_dict(d), false);
    assert_eq!(has_items("".to_string()), 0);
    assert_eq!(has_items("x".to_string()), 1);
}
"#;
    assert_rustc_runs("container_truthiness", &rust, driver);
}

/// PMAT-661: int/float truthiness in if/while/elif conditions. `if n:` /
/// `if len(xs):` / `while n:` were rejected ("condition does not type as bool");
/// they now coerce int → `n != 0` and float → `x != 0.0` (Python's float edges:
/// -0.0 falsy, nan truthy). Bool conditions pass through. Cross-checked vs python3.
#[test]
fn int_truthiness() {
    let rust = xpile_transpile_to_rust("int_truthiness.py");
    assert!(
        rust.contains("!= 0i64") && rust.contains("!= 0f64"),
        "int/float truthiness should lower to `!= 0`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(if_int(5), 1);
    assert_eq!(if_int(0), 0);
    assert_eq!(if_len(vec![1, 2]), 1);
    assert_eq!(if_len(vec![]), 0);
    assert_eq!(while_int(3), 6);
    assert_eq!(if_float(2.5), 1);
    assert_eq!(if_float(0.0), 0);
    assert_eq!(elif_int(5), 1);
    assert_eq!(elif_int(0), 0);
    assert_eq!(elif_int(200), 2);
    assert_eq!(if_bool_regression(true), 1);  // bool passes through
}
"#;
    assert_rustc_runs("int_truthiness", &rust, driver);
}

/// PMAT-663: `not <int>` / `not <float>` / `not len(xs)` — the inverse of the
/// PMAT-661 truthiness coercion. `not n` was rejected; it now lowers to `n == 0`
/// (float → `x == 0.0`). Container `not` and bool `not` are unchanged.
/// Cross-checked vs python3.
#[test]
fn not_int_truthiness() {
    let rust = xpile_transpile_to_rust("not_int_truthiness.py");
    let driver = r#"
fn main() {
    assert_eq!(not_int(0), 1);
    assert_eq!(not_int(5), 0);
    assert_eq!(not_len(vec![]), 1);
    assert_eq!(not_len(vec![1]), 0);
    assert_eq!(not_float(0.0), 1);
    assert_eq!(not_float(2.5), 0);
    let mut d0 = std::collections::HashMap::new();
    let mut d1 = std::collections::HashMap::new();
    d1.insert(1, 1);
    assert_eq!(not_container_regression(d0), 1);
    assert_eq!(not_container_regression(d1), 0);
    assert_eq!(not_bool_regression(true), 0);
    assert_eq!(not_bool_regression(false), 1);
}
"#;
    assert_rustc_runs("not_int_truthiness", &rust, driver);
}

/// PMAT-528 (Tranche 2): `xs.pop()` / `xs.pop(i)` as a bare statement (discard
/// the popped value), e.g. `while xs: xs.pop()`. The value-position form already
/// worked; a bare statement now reuses the same pop lowering wrapped in a
/// discard `let _ = …;` (receiver auto-`mut`). Cross-checked vs python3 (4,2,32).
#[test]
fn list_pop_statement() {
    let rust = xpile_transpile_to_rust("list_pop_statement.py");
    assert!(
        rust.contains("let _: i64 = (xs).pop()"),
        "a bare `xs.pop()` should lower to a discard `let _`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(drain_count(vec![1, 2, 3, 4]), 4);
    assert_eq!(pop_front_len(vec![5, 6, 7]), 2);
    assert_eq!(pop_twice_sum(vec![10, 20, 30, 40]), 32);
}
"#;
    assert_rustc_runs("list_pop_statement", &rust, driver);
}

/// PMAT-529 (Tranche 2): bare-statement `d.pop(k)` / `d.pop(k, default)` on a
/// dict — the value-position forms (`x = d.pop(k)`) already worked; a bare
/// statement now reuses the same pop lowering wrapped in a discard `let _ = …;`
/// (receiver auto-`mut`), broadening PMAT-528 (which covered list pop) to dict
/// receivers. Emits `(d).remove(&…).unwrap()` / `.unwrap_or(default)`.
/// Cross-checked vs python3 (2, 2, 21).
#[test]
fn dict_pop_statement() {
    let rust = xpile_transpile_to_rust("dict_pop_statement.py");
    assert!(
        rust.contains("let _: i64 = (d).remove("),
        "a bare `d.pop(k)` should lower to a discard `let _` over `.remove`:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d1 = std::collections::HashMap::new();
    d1.insert(String::from("a"), 1i64);
    d1.insert(String::from("b"), 2i64);
    d1.insert(String::from("c"), 3i64);
    assert_eq!(remove_key(d1), 2);
    let mut d2 = std::collections::HashMap::new();
    d2.insert(String::from("a"), 1i64);
    d2.insert(String::from("b"), 2i64);
    assert_eq!(remove_with_default(d2), 2);
    let mut d3 = std::collections::HashMap::new();
    d3.insert(String::from("a"), 1i64);
    d3.insert(String::from("b"), 20i64);
    d3.insert(String::from("c"), 3i64);
    assert_eq!(drain_two(d3), 21);
}
"#;
    assert_rustc_runs("dict_pop_statement", &rust, driver);
}

/// PMAT-530 (Tranche 2): `s[::-1]` reverse-slice over a `str` — the list form
/// `xs[::-1]` already lowered to `Expr::Reversed`; the str form now lowers to a
/// `StrMethod` with the new `Reverse` op → `.chars().rev().collect::<String>()`
/// (reverse by Unicode scalar value). Composes with other string methods
/// (`s.upper()[::-1]`) and inside larger expressions (`s == s[::-1]`).
/// Cross-checked vs python3 (olleh, True, False, CBA).
#[test]
fn str_reverse_slice() {
    let rust = xpile_transpile_to_rust("str_reverse_slice.py");
    assert!(
        rust.contains(".chars().rev().collect::<String>()"),
        "`s[::-1]` should lower to a `.chars().rev().collect`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(reverse(String::from("hello")), "olleh");
    assert_eq!(is_palindrome(String::from("racecar")), true);
    assert_eq!(is_palindrome(String::from("hello")), false);
    assert_eq!(reverse_upper(String::from("abc")), "CBA");
}
"#;
    assert_rustc_runs("str_reverse_slice", &rust, driver);
}

/// PMAT-531 (Tranche 2): a **tuple target** in an expression-position
/// generator expression / comprehension (`sum(v for k, v in d.items())`,
/// `sum(x * y for x, y in zip(a, b))`). The statement-position list comp
/// already supported tuple targets (via `ForEachPair`); the shared expr-position
/// core `lower_comp_to_map` now binds a 2-name tuple target through a Rust
/// tuple-destructure closure param (`|__k| { let (k, v) = __k.clone(); … }`),
/// splitting the element 2-tuple type. Works over `d.items()`, `zip(...)`,
/// `enumerate(...)`, with an `if` filter. No new IR. Cross-checked vs python3
/// (6, 20, 2, 32, 80).
#[test]
fn genexpr_tuple_target() {
    let rust = xpile_transpile_to_rust("genexpr_tuple_target.py");
    assert!(
        rust.contains("let (k, v) = __k.clone();"),
        "a tuple-target genexpr should destructure the closure param:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 1i64);
    d.insert(String::from("b"), 2i64);
    d.insert(String::from("c"), 3i64);
    assert_eq!(sum_values(d.clone()), 6);
    let mut d2 = std::collections::HashMap::new();
    d2.insert(String::from("a"), 1i64);
    d2.insert(String::from("b"), 20i64);
    d2.insert(String::from("c"), 3i64);
    assert_eq!(max_value(d2), 20);
    let mut d3 = std::collections::HashMap::new();
    d3.insert(String::from("a"), 1i64);
    d3.insert(String::from("b"), -2i64);
    d3.insert(String::from("c"), 3i64);
    d3.insert(String::from("d"), -4i64);
    assert_eq!(count_positive(d3), 2);
    assert_eq!(dot(vec![1, 2, 3], vec![4, 5, 6]), 32);
    assert_eq!(weighted(vec![10, 20, 30]), 80);
}
"#;
    assert_rustc_runs("genexpr_tuple_target", &rust, driver);
}

/// PMAT-532 (Tranche 2): in-place set/dict mutators `s.update(other)` /
/// `s.clear()` / `d.clear()`. `set.update` was rejected even though `dict.update`
/// worked (an asymmetry); `set.clear`/`dict.clear` were rejected even though
/// `list.clear` worked. All three reuse existing IR — `set.update` → the
/// `ListExtend` stmt (`s.extend((other).iter().cloned())`, valid for `HashSet`),
/// and the clears → `ListMutate { Clear }` (`name.clear();`, valid for
/// `HashSet`/`HashMap`). No new IR/codegen. Cross-checked vs python3 (5, 4, 0, 0).
#[test]
fn set_dict_mutators() {
    let rust = xpile_transpile_to_rust("set_dict_mutators.py");
    assert!(
        rust.contains("s.extend((t).iter().cloned());"),
        "`s.update(t)` should reuse the list-extend lowering:\n{rust}"
    );
    let driver = r#"
fn main() {
    let s: std::collections::HashSet<i64> = [1, 2, 3].into_iter().collect();
    let t: std::collections::HashSet<i64> = [3, 4, 5].into_iter().collect();
    assert_eq!(merge(s, t), 5);
    assert_eq!(update_literal(), 4);
    let s2: std::collections::HashSet<i64> = [1, 2, 3].into_iter().collect();
    assert_eq!(wipe_set(s2), 0);
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), 1i64);
    d.insert(String::from("b"), 2i64);
    assert_eq!(wipe_dict(d), 0);
}
"#;
    assert_rustc_runs("set_dict_mutators", &rust, driver);
}

/// PMAT-533 (Tranche 2): in-place `append` on a **subscript receiver** —
/// `g[i].append(e)` (list-of-list) and `d[k].append(e)` (dict-of-list). The
/// bare `<name>.append(e)` form already worked; this is the indexed-receiver
/// companion via the new `Stmt::IndexAppend`. List base indexes a mutable place
/// directly (`g[(i) as usize].push(e)`); dict base reaches the value via
/// `get_mut(&(k)).unwrap().push(e)` (KeyError parity). The mutability pre-walk
/// recognises the subscript receiver, so the base is `mut`. Cross-checked vs
/// python3 (2, 35, 3).
#[test]
fn subscript_append() {
    let rust = xpile_transpile_to_rust("subscript_append.py");
    assert!(
        rust.contains("as usize].push(") && rust.contains(".get_mut(&("),
        "subscript append should index a place / get_mut the value:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(grid_row_append(vec![vec![1, 2], vec![3]], 1), 2);
    assert_eq!(first_row_total(vec![vec![5], vec![9]]), 35);
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("a"), vec![1i64, 2]);
    d.insert(String::from("b"), vec![3i64]);
    assert_eq!(bucket_append(d, String::from("a"), 7), 3);
}
"#;
    assert_rustc_runs("subscript_append", &rust, driver);
}

/// PMAT-534 (Tranche 2): `x in range(...)` / `x not in range(...)` membership →
/// a **bounds check**, not a materialized Vec (`x in range(10**9)` must not
/// allocate). `range(n)` → `0 <= x && x < n`; `range(a, b)` → `a <= x && x < b`;
/// a 3-arg literal step adds the reachability check `(x - start) % |step| == 0`
/// (Python floor-mod via `rem_euclid`). Composes inside a genexpr filter.
/// Cross-checked vs python3 (the full boundary sweep matches).
#[test]
fn in_range_membership() {
    let rust = xpile_transpile_to_rust("in_range_membership.py");
    assert!(
        rust.contains("(0i64 <= x) && (x < n)"),
        "`x in range(n)` should lower to a bounds check, not a Vec:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(in_n(3, 5), true);
    assert_eq!(in_n(5, 5), false);
    assert_eq!(in_ab(2), true);
    assert_eq!(in_ab(10), false);
    assert_eq!(not_in_n(7, 5), true);
    assert_eq!(in_step(4), true);
    assert_eq!(in_step(5), false);
    assert_eq!(count_hits(vec![1, 3, 4, 6, 7, 5]), 4);
}
"#;
    assert_rustc_runs("in_range_membership", &rust, driver);
}

/// PMAT-535 (Tranche 2): `int(b)` / `float(b)` over a `bool` — Python
/// `True`/`False` → `1`/`0` (`1.0`/`0.0`). Previously `int(bool)` emitted a bare
/// undefined `int(...)` call and `float(bool)` was rejected; the int/float cast
/// handler only covered int/float/str. Rust allows `bool as i64` but NOT
/// `bool as f64`, so `float(bool)` casts through `i64` first. Enables the common
/// `sum(int(b) for b in bs)` boolean-count idiom. Cross-checked vs python3
/// (1, 0, 3, 2, 1, 0, 2.5, 0.0).
#[test]
fn int_float_of_bool() {
    let rust = xpile_transpile_to_rust("int_float_of_bool.py");
    assert!(
        rust.contains("(b) as i64"),
        "`int(b)` should lower to `(b) as i64`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(bool_to_int(true), 1);
    assert_eq!(bool_to_int(false), 0);
    assert_eq!(count_true(vec![true, false, true, true]), 3);
    assert_eq!(predicate_to_int(15), 2);
    assert_eq!(predicate_to_int(5), 1);
    assert_eq!(predicate_to_int(-3), 0);
    assert_eq!(bool_to_float_scaled(true), 2.5f64);
    assert_eq!(bool_to_float_scaled(false), 0.0f64);
}
"#;
    assert_rustc_runs("int_float_of_bool", &rust, driver);
}

/// PMAT-536 (Tranche 2): keyword (named-field) form of `str.format` —
/// `"{x}".format(x=n)`. Positional `.format(n)` already worked; the named form
/// rewrites each `{name}` placeholder to a positional `{N}` (first-occurrence
/// order, repeats reuse the index) and passes the referenced kwargs positionally
/// to the existing `lower_str_format`. Handles reordering, repeats, format specs,
/// and tolerates unused kwargs. Cross-checked vs python3 (hello world!, 2,3,
/// 2-1, 7 7 7, 3.14).
#[test]
fn str_format_kwargs() {
    let rust = xpile_transpile_to_rust("str_format_kwargs.py");
    assert!(
        rust.contains(r#"format!("{0}-{1}", b, a)"#),
        "named fields should rewrite to positional, reordered by template:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(greet(String::from("world")), "hello world!");
    assert_eq!(coords(2, 3), "2,3");
    assert_eq!(reorder(1, 2), "2-1");
    assert_eq!(repeated(7), "7 7 7");
    assert_eq!(with_spec(3.14159), "3.14");
}
"#;
    assert_rustc_runs("str_format_kwargs", &rust, driver);
}

/// PMAT-538 (correctness): Python `//` / `%` with a **negative divisor**.
/// `div_euclid` / `rem_euclid` only match Python for a positive divisor; for a
/// negative divisor Python `//` floors toward −∞ and `%` takes the sign of the
/// divisor, so the euclidean ops silently diverge (e.g. `-7 // -2` is 3 in
/// Python but `div_euclid` gives 4). The emit now uses the truncating
/// quotient/remainder plus a floor correction. Cross-checked vs python3 across
/// all sign combinations (3/-4/-4/3, -1/-2/2/1, 1/1).
#[test]
fn floordiv_mod_signs() {
    let rust = xpile_transpile_to_rust("floordiv_mod_signs.py");
    assert!(
        !rust.contains("div_euclid") && !rust.contains("rem_euclid"),
        "must not use euclidean ops (wrong for a negative divisor):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(fdiv(-7, -2), 3);
    assert_eq!(fdiv(7, -2), -4);
    assert_eq!(fdiv(-7, 2), -4);
    assert_eq!(fdiv(7, 2), 3);
    assert_eq!(fmod(-7, -2), -1);
    assert_eq!(fmod(7, -3), -2);
    assert_eq!(fmod(-7, 3), 2);
    assert_eq!(fmod(7, 2), 1);
    assert_eq!(clock(13), 1);
    assert_eq!(clock(25), 1);
}
"#;
    assert_rustc_runs("floordiv_mod_signs", &rust, driver);
}

/// PMAT-539 (correctness): Python slice bounds — negative bounds (`xs[-2:]`,
/// `xs[:-1]`) count from the end, every bound clamps to `[0, len]`, and
/// `lo > hi` yields empty. The naive `(lo) as usize` emit panicked on a
/// negative bound (wraps to a huge usize) or an out-of-range bound — so the
/// ubiquitous `xs[:-1]` / `xs[-3:]` idioms crashed at runtime. The emit now
/// resolves + clamps each bound. Cross-checked vs python3 (9, 10, 9, def, 2, 0).
#[test]
fn negative_slice() {
    let rust = xpile_transpile_to_rust("negative_slice.py");
    assert!(
        rust.contains("if __b < 0 { (__n + __b).max(0) }"),
        "negative slice bounds must resolve from the end + clamp:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(last_two(vec![1, 2, 3, 4, 5]), 9);
    assert_eq!(drop_last(vec![1, 2, 3, 4, 5]), 10);
    assert_eq!(middle(vec![1, 2, 3, 4, 5]), 9);
    assert_eq!(tail_str(String::from("abcdef")), "def");
    assert_eq!(clamp_oob(vec![1, 2, 3]), 2);
    assert_eq!(reversed_bounds(vec![1, 2, 3, 4, 5]), 0);
}
"#;
    assert_rustc_runs("negative_slice", &rust, driver);
}

/// PMAT-540 (correctness): mixed `float`/`int` comparison and arithmetic.
/// Rust rejects `f64 == i64` (E0308) and `f64 + i64` (E0277), so `x == 3`,
/// `x < n`, `x * 2 + 1` over a float `x` produced non-compiling Rust. The
/// float-ARITHMETIC path promotes the int operand to `f64` (Python promotes
/// numerically). PMAT-745 superseded the comparison half: an int↔float
/// comparison is now EXACT (no f64 cast that would lose precision above 2^53),
/// emitting an `i128`-tiebreak block instead. Cross-checked vs python3
/// (F/T/T/F/6.0/T/F/7.5).
#[test]
fn mixed_float_int() {
    let rust = xpile_transpile_to_rust("mixed_float_int.py");
    assert!(
        // PMAT-745: the comparison `x == 3` is now EXACT (i128 tiebreak), not a
        // lossy `((3i64) as f64)` cast; float ARITHMETIC still promotes the int.
        rust.contains("(__cn as i128) == (__cf as i128)")
            && rust.contains("x * ((2i64) as f64)"),
        "mixed float/int: comparison must be exact (PMAT-745) and float arithmetic must promote the int to f64:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(at_least(3.0, 5), false);
    assert_eq!(at_least(5.0, 5), true);
    assert_eq!(is_whole_three(3.0), true);
    assert_eq!(is_whole_three(3.5), false);
    assert_eq!(scaled(2.5), 6.0f64);
    assert_eq!(half_past(3.5), true);
    assert_eq!(half_past(15.0), false);
    assert_eq!(int_times_float(2.5), 7.5f64);
}
"#;
    assert_rustc_runs("mixed_float_int", &rust, driver);
}

/// PMAT-541 (correctness): mixed-numeric `min`/`max` — `min(x, n)` with
/// `x: float`, `n: int` emitted `f64::min(i64)` (E0308). When any operand is a
/// float, every operand is promoted to f64 (Python compares numerically).
/// Homogeneous int/float/str min-max is untouched. Cross-checked vs python3
/// (2.5, 2.0, 4.0, 7.5, 2.0, 4.0).
#[test]
fn min_max_mixed_numeric() {
    let rust = xpile_transpile_to_rust("min_max_mixed_numeric.py");
    // PMAT-601: float min/max now use the Python first-arg-wins fold; the int
    // operand is still promoted to f64.
    assert!(
        rust.contains("((n) as f64)") && rust.contains("if __x < __m"),
        "mixed min/max must promote the int operand to f64 (first-arg-wins fold):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(lo(2.5, 5), 2.5f64);
    assert_eq!(lo(5.5, 2), 2.0f64);
    assert_eq!(hi(1.5, 4), 4.0f64);
    assert_eq!(hi(7.5, 4), 7.5f64);
    assert_eq!(lo_int_first(2, 3.5), 2.0f64);
    assert_eq!(clamp_hi(1.5, 4, 2), 4.0f64);
}
"#;
    assert_rustc_runs("min_max_mixed_numeric", &rust, driver);
}

/// PMAT-601: 2-arg `max`/`min` over floats follow Python's first-argument-wins
/// semantics (and NaN propagation), not `f64::max`/`f64::min` (which treat
/// `+0.0 > -0.0` and drop NaN). On a tie / incomparable compare the first
/// argument is kept. Cross-checked vs python3: `max(-0.0,0.0)` is `-0.0`,
/// `min(-0.0,0.0)` is `-0.0`, `max(0.0,-0.0)` is `0.0`; distinct values normal.
#[test]
fn float_min_max() {
    let rust = xpile_transpile_to_rust("float_min_max.py");
    assert!(
        rust.contains("if __x > __m { __m = __x; }")
            && rust.contains("if __x < __m { __m = __x; }"),
        "float max/min must use the first-arg-wins fold:\n{rust}"
    );
    let driver = r#"
fn main() {
    // signed-zero ties keep the first argument.
    assert!(mx(-0.0, 0.0).is_sign_negative());
    assert!(mn(-0.0, 0.0).is_sign_negative());
    assert!(!mx(0.0, -0.0).is_sign_negative());
    // distinct values behave normally.
    assert_eq!(mx(2.0, 3.0), 3.0);
    assert_eq!(mn(2.0, 3.0), 2.0);
    assert_eq!(mx3(1.0, 5.0, 3.0), 5.0);
}
"#;
    assert_rustc_runs("float_min_max", &rust, driver);
}

/// PMAT-542 (correctness): mixed `float`/`int` ternary branches. `x if b else 0`
/// (float then-branch, int else-branch) was rejected (both arms of a Rust
/// `if`-expression must share a type) even though Python yields a float when
/// either branch is float. The int branch is now promoted to f64. Cross-checked
/// vs python3 (2.5, 0.0, 0.0, 2.5, 1.5, 0.0).
#[test]
fn ternary_mixed_float_int() {
    let rust = xpile_transpile_to_rust("ternary_mixed_float_int.py");
    assert!(
        rust.contains("(0i64) as f64") || rust.contains("((0i64) as f64)"),
        "the int ternary branch must promote to f64:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(or_zero(true, 2.5), 2.5f64);
    assert_eq!(or_zero(false, 2.5), 0.0f64);
    assert_eq!(zero_or(true, 2.5), 0.0f64);
    assert_eq!(zero_or(false, 2.5), 2.5f64);
    assert_eq!(lit_branches(true), 1.5f64);
    assert_eq!(lit_branches(false), 0.0f64);
}
"#;
    assert_rustc_runs("ternary_mixed_float_int", &rust, driver);
}

/// PMAT-543 (Tranche 2): two-generator comprehensions over `range(...)` —
/// `[i*j for i in range(n) for j in range(n)]`. The 2-generator desugar already
/// handled `list[T]` iterables (nested `ForEach`); a bare `range(...)` generator
/// iterable now materializes to a `Vec` via `lower_range_list` (mirroring the
/// 1-generator range handling). Works for list + dict comps, with filters, and
/// mixed range/list generators. Cross-checked vs python3 (9, 22, 90, 9).
#[test]
fn comp_2gen_range() {
    let rust = xpile_transpile_to_rust("comp_2gen_range.py");
    let driver = r#"
fn main() {
    assert_eq!(products(3), 9);
    assert_eq!(off_diagonal(4), 22);
    assert_eq!(mixed(vec![10, 20]), 90);
    assert_eq!(grid_size(3), 9);
}
"#;
    assert_rustc_runs("comp_2gen_range", &rust, driver);
}

/// PMAT-562 (Tranche 2): three-way `zip` — `for a, b, c in zip(x, y, z)`. New
/// `Stmt::ForEachZip3` emits a left-nested `.zip().zip()` chain with a nested
/// `((a, b), c)` destructure (stops at the shortest iterable, like Python).
/// Cross-checked vs python3 (270, 666, 3).
#[test]
fn zip3() {
    let rust = xpile_transpile_to_rust("zip3.py");
    assert!(
        rust.contains("for ((x, y), z) in")
            && rust.contains(".iter().cloned().zip(")
            && rust.contains(".iter().cloned()).zip("),
        "zip3 should emit a nested .zip().zip() with ((x, y), z) destructure:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(dot3(vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]), 270);
    assert_eq!(sum_triples(vec![1, 2, 3], vec![10, 20, 30], vec![100, 200, 300]), 666);
    assert_eq!(shortest_stops(vec![1, 2, 3, 4, 5], vec![1, 2, 3], vec![1, 2, 3, 4]), 3);
}
"#;
    assert_rustc_runs("zip3", &rust, driver);
}

/// PMAT-563 (Tranche 2): **multiple `if` filters** in a comprehension /
/// generator expression — Python ANDs them (`[x for x in xs if a if b]` ==
/// `… if a and b`). The shared `combine_comp_filters` folds all clauses into a
/// left-nested `&&` chain across list/set/dict comps, genexprs, comp-over-range,
/// and the 2-generator path. Cross-checked vs python3 (154, 3, 5, 1, 3, 117).
#[test]
fn multi_if_comp() {
    let rust = xpile_transpile_to_rust("multi_if_comp.py");
    let driver = r#"
fn main() {
    assert_eq!(genexpr_2if(vec![5, -3, 50, 200, 99]), 154);
    assert_eq!(listcomp_2if(vec![1, 2, 3, 4, -6, 8]), 3);
    assert_eq!(listcomp_range_2if(20), 5);
    assert_eq!(setcomp_2if(vec![5, 15, 25, 105, -1]), 1);
    assert_eq!(dictcomp_2if(vec![1, 2, 9, 50, -3]), 3);
    assert_eq!(three_if(vec![3, 6, 9, 99, 102, 5]), 117);
}
"#;
    assert_rustc_runs("multi_if_comp", &rust, driver);
}

/// PMAT-556 (Tranche 2): expression-position **two-generator** generator
/// expression / list comprehension — `sum(i*j for i in range(n) for j in
/// range(n))`, `len([… for i in a for j in b])`. The single-generator
/// expr-position path is `Map`/`Filter`; a 2-generator one builds its flattened
/// `Vec` via nested loops inside an `Expr::Block` (reusing the statement-position
/// `desugar_comp_2gen`), returning the accumulator as the block's trailing
/// expression. Cross-checked vs python3 (36, 36, 16, 180).
#[test]
fn genexpr_2gen() {
    let rust = xpile_transpile_to_rust("genexpr_2gen.py");
    assert!(
        rust.contains("__xcomp2"),
        "expected a block-built accumulator for the 2-generator genexpr:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pair_sum(4), 36);
    assert_eq!(filtered(4), 36);
    assert_eq!(count_pairs(4), 16);
    assert_eq!(over_lists(vec![1, 2, 3], vec![10, 20]), 180);
}
"#;
    assert_rustc_runs("genexpr_2gen", &rust, driver);
}

/// PMAT-559 (Tranche 2): tuple-unpack with a **subscript target** — the in-place
/// swap idiom `xs[i], xs[j] = xs[j], xs[i]` (and dict keys `d[a], d[b] = …`).
/// All RHS elements are lowered into temps first (so a swap reads both old
/// values before writing either), then each temp is assigned to its target
/// (`IndexAssign` / `DictSet`). The base is marked mutable by the pre-walk.
/// Cross-checked vs python3 (201, 54001, 1, 210).
#[test]
fn subscript_swap() {
    let rust = xpile_transpile_to_rust("subscript_swap.py");
    assert!(
        rust.contains("__unpack0") && rust.contains("__unpack1"),
        "subscript swap should stage the RHS into temps first:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(swap_first_two(vec![1, 2, 3]), 201);
    assert_eq!(reverse_inplace(vec![1, 2, 3, 4, 5]), 54001);
    assert_eq!(bubble_sort_min(vec![5, 3, 1, 4, 2]), 1);
    let mut d = std::collections::HashMap::new();
    d.insert(String::from("x"), 10);
    d.insert(String::from("y"), 20);
    assert_eq!(dict_swap(d, String::from("x"), String::from("y")), 210);
}
"#;
    assert_rustc_runs("subscript_swap", &rust, driver);
}

/// PMAT-560 (Tranche 2): negative-literal index on the **assignment** side —
/// `xs[-k] = v` / `xs[-k] += v` resolve to `xs[len(xs) - k]` (mirroring the
/// read-side desugar). PMAT-640: a single-index write now stages the (wrapped)
/// index into an `__aidx` temp first — this both wraps a runtime-negative index
/// like Python AND avoids the `index_mut` borrow conflict (E0502) that
/// `xs[xs.len() - 1] = v` otherwise hits. Cross-checked vs python3
/// (9, 103, 4010, 7, 8).
#[test]
fn neg_index_write() {
    let rust = xpile_transpile_to_rust("neg_index_write.py");
    assert!(
        rust.contains("__aidx"),
        "a single-index write should stage the (wrapped) index into a temp:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(set_last(vec![1, 2, 3], 9), 9);
    assert_eq!(set_2nd_last(vec![1, 2, 3, 4]), 103);
    assert_eq!(swap_ends(vec![10, 2, 3, 40]), 4010);
    assert_eq!(rotate_last_to_first(vec![1, 2, 3, 7]), 7);
    assert_eq!(neg_aug(vec![1, 2, 3]), 8);
}
"#;
    assert_rustc_runs("neg_index_write", &rust, driver);
}

/// PMAT-640: **correctness** — a runtime-negative list-index WRITE wraps like
/// Python (`xs[-1] = v` targets the last element) instead of `(-1) as usize`
/// panicking — the assign-side companion to PMAT-639's read fix. A non-negative
/// literal index keeps the bare path; an RHS reading the same list still
/// compiles (the wrapped index is staged before the `index_mut` assign).
/// Cross-checked vs python3.
#[test]
fn neg_runtime_index_assign() {
    let rust = xpile_transpile_to_rust("neg_runtime_index_assign.py");
    assert!(
        rust.contains("if __ai0 < 0 { xs.len() as i64 + __ai0 }"),
        "runtime-negative write should wrap the index:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(set_at(vec![1, 2, 3], -1, 99), 99);   // xs[-1] = 99
    assert_eq!(set_at(vec![1, 2, 3], 1, 7), 3);       // positive unaffected
    assert_eq!(aug_at(vec![1, 2, 3], -1), 103);       // xs[-1] += 100
    assert_eq!(set_from_self(vec![10, 20, 30], -1), 11); // xs[-1] = xs[0]+1
    assert_eq!(set_positive(vec![1, 2, 3]), 9);
}
"#;
    assert_rustc_runs("neg_runtime_index_assign", &rust, driver);
}

/// PMAT-641: **correctness** — a runtime-negative index at ANY nesting level of
/// a subscript WRITE wraps like Python (`grid[-1][-1] = v`), each level using
/// its own `len`. Completes the negative-index work (read 639, single write
/// 640, nested write here); the assign wrap now also subsumes the old
/// self-referential `needs_temps` path. Cross-checked vs python3.
#[test]
fn neg_nested_index_assign() {
    let rust = xpile_transpile_to_rust("neg_nested_index_assign.py");
    // Each level wraps via the progressively-indexed collection's own len.
    assert!(
        rust.contains("if __ai1 < 0 { grid[__aidx0 as usize].len() as i64 + __ai1 }"),
        "inner level wraps via the indexed sub-collection's len:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(set_outer(vec![vec![1, 2], vec![3, 4]], -1), 99);  // grid[-1][0]
    assert_eq!(set_inner(vec![vec![1, 2], vec![3, 4]], -1), 88);  // grid[0][-1]
    assert_eq!(set_both_neg(vec![vec![1, 2], vec![3, 4]]), 77);   // grid[-1][-1]
    assert_eq!(set_literal(vec![vec![1, 2], vec![3, 4]]), 50);
}
"#;
    assert_rustc_runs("neg_nested_index_assign", &rust, driver);
}

/// PMAT-544 (Tranche 2): `enumerate(s)` / `zip(s, …)` over a **string** —
/// iterate its characters (each a 1-char string). The paired-loop handler
/// required a `list` iterable; a `str` iterable now materializes to a
/// `List(Str)` via `Expr::StrChars` (the same conversion `for c in s` uses).
/// Supports `enumerate(s, start)` and `zip(s, list)`. Cross-checked vs python3
/// (2, 66, 6, 134).
#[test]
fn enumerate_zip_str() {
    let rust = xpile_transpile_to_rust("enumerate_zip_str.py");
    let driver = r#"
fn main() {
    assert_eq!(index_of(String::from("hello"), String::from("l")), 2);
    assert_eq!(weighted_ord(String::from("AB")), 66);
    assert_eq!(start_sum(String::from("abc")), 6);
    assert_eq!(zip_str_list(String::from("AB"), vec![1, 2]), 134);
}
"#;
    assert_rustc_runs("enumerate_zip_str", &rust, driver);
}

/// PMAT-545 (Tranche 2): `str.rfind` / `str.rindex` — reverse-search mirrors of
/// `find` / `index`. `rfind(sub)` → byte index of the last match or `-1`;
/// `rindex(sub)` panics on absence (Python `ValueError`). Both reuse the
/// `StrMethod` pipeline via Rust's `str::rfind`. Cross-checked vs python3
/// (5, -1, 3, 5).
#[test]
fn str_rfind() {
    let rust = xpile_transpile_to_rust("str_rfind.py");
    assert!(
        rust.contains(".rfind(&("),
        "`rfind`/`rindex` should lower to Rust `str::rfind`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(last_a(String::from("banana")), 5);
    assert_eq!(last_missing(String::from("banana")), -1);
    assert_eq!(last_pair(String::from("banana")), 3);
    assert_eq!(last_a_index(String::from("banana")), 5);
}
"#;
    assert_rustc_runs("str_rfind", &rust, driver);
}

/// PMAT-546 (Tranche 2): comprehensions / generator expressions over a
/// **string** — `[c.upper() for c in s]`, `{c for c in s}`, `{c: ord(c) for c
/// in s}`, `sum(ord(c) for c in s)`. A `str` comprehension iterable now
/// materializes to `List(Str)` (1-char strings) via `Expr::StrChars` at every
/// comprehension iterable site (the shared `str_iter_to_chars` helper). Works
/// for list/set/dict comps + genexprs, with filters. Cross-checked vs python3
/// (294, 3, 3, 3, 3).
#[test]
fn comp_over_str() {
    let rust = xpile_transpile_to_rust("comp_over_str.py");
    let driver = r#"
fn main() {
    assert_eq!(ord_sum(String::from("abc")), 294);
    assert_eq!(upper_count(String::from("banana")), 3);
    assert_eq!(distinct_chars(String::from("banana")), 3);
    assert_eq!(char_codes(String::from("abca")), 3);
    assert_eq!(digit_count(String::from("a1b2c3")), 3);
}
"#;
    assert_rustc_runs("comp_over_str", &rust, driver);
}

/// PMAT-547 (correctness): tuple-unpack init + later augment — `i, total = 0, 0`
/// then `total += i`. The augment was rejected ("augments before assigned"
/// because `LetTuple` never registered `ctx.bound`); the mutability pre-walk
/// also didn't count tuple-unpack targets (so a single non-loop augment stayed
/// immutable → E0384). Now `LetTuple` binds each name + the pre-walk counts
/// tuple targets, and `LetTuple` carries a per-name `mutable` flag → emits
/// `let (mut a, b) = …` (only the mutated name gets `mut`). Cross-checked vs
/// python3 (18, 10, 31).
#[test]
fn tuple_unpack_augment() {
    let rust = xpile_transpile_to_rust("tuple_unpack_augment.py");
    assert!(
        rust.contains("let (mut a, b) ="),
        "only the mutated unpacked name should be `mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(two_accumulators(vec![1, 2, 3]), 18);
    assert_eq!(while_accumulate(5), 10);
    assert_eq!(one_mut_one_const(), 31);
}
"#;
    assert_rustc_runs("tuple_unpack_augment", &rust, driver);
}

/// PMAT-548 (Tranche 2): negative-step list slice `xs[::-k]` (k ≥ 2) —
/// reverse, then take every k-th element. Generalises the `xs[::-1]` reverse;
/// the unbounded list form lowers to `.iter().rev().step_by(k)` over the
/// already-clamped range. Bounded negative-step slices (`xs[a:b:-k]`) and
/// stepped string slices remain deferred. Cross-checked vs python3 (12, 12, 60).
#[test]
fn negative_step_slice() {
    let rust = xpile_transpile_to_rust("negative_step_slice.py");
    assert!(
        rust.contains(".iter().rev().step_by(2)"),
        "`xs[::-2]` should reverse then step:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(every_other_rev(vec![1, 2, 3, 4, 5, 6]), 12);
    assert_eq!(every_third_rev(vec![1, 2, 3, 4, 5, 6, 7]), 12);
    assert_eq!(full_reverse(vec![10, 20, 30]), 60);
}
"#;
    assert_rustc_runs("negative_step_slice", &rust, driver);
}

/// PMAT-549 (Tranche 2): `math.gcd(a, b)` — greatest common divisor of two ints.
/// Lowers to a new `Expr::Gcd` whose codegen is an inline Euclidean-algorithm
/// block over the operands' absolute values (`gcd(0, 0) == 0`, always
/// non-negative). Cross-checked vs python3 (12, 1, 7, 2, 4).
#[test]
fn math_gcd() {
    let rust = xpile_transpile_to_rust("math_gcd.py");
    assert!(
        rust.contains("__gb = __ga % __gb"),
        "math.gcd should lower to an inline Euclidean block:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(gcd2(48, 36), 12);
    assert_eq!(gcd2(17, 5), 1);
    assert_eq!(gcd2(0, 7), 7);
    assert_eq!(reduce_fraction(12, 18), 2);
    assert_eq!(gcd_negative(-12, 8), 4);
}
"#;
    assert_rustc_runs("math_gcd", &rust, driver);
}

/// PMAT-550 (Tranche 2): `math.lcm(a, b)` — least common multiple of two ints.
/// New `Expr::Lcm` → inline `(abs(a)/gcd) * abs(b)` block (divide before
/// multiply; `lcm(0, x) == 0`, always non-negative). Cross-checked vs python3
/// (42, 35, 0, 12).
#[test]
fn math_lcm() {
    let rust = xpile_transpile_to_rust("math_lcm.py");
    assert!(
        rust.contains("(__la / __ga) * __lb"),
        "math.lcm should lower to (abs(a)/gcd)*abs(b):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(lcm2(21, 6), 42);
    assert_eq!(lcm_coprime(7, 5), 35);
    assert_eq!(lcm_zero(0, 9), 0);
    assert_eq!(lcm_negative(-4, 6), 12);
}
"#;
    assert_rustc_runs("math_lcm", &rust, driver);
}

/// PMAT-551 (Tranche 2): `math.factorial(n)` — n! of a non-negative int. New
/// `Expr::Factorial` → inline product loop (`0! == 1`; `checked_mul` overflow
/// guard; negative `n` panics = Python `ValueError`). Composes in arithmetic
/// (binomial coefficients). Cross-checked vs python3 (120, 3628800, 1, 10, 20).
#[test]
fn math_factorial() {
    let rust = xpile_transpile_to_rust("math_factorial.py");
    assert!(
        rust.contains("__f.checked_mul(__fi)"),
        "math.factorial should lower to a checked product loop:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(fact(5), 120);
    assert_eq!(fact(10), 3628800);
    assert_eq!(fact_zero(0), 1);
    assert_eq!(binomial(5, 2), 10);
    assert_eq!(binomial(6, 3), 20);
}
"#;
    assert_rustc_runs("math_factorial", &rust, driver);
}

/// PMAT-552 (Tranche 2): `math.isqrt(n)` — exact integer square root `⌊√n⌋`.
/// New `Expr::Isqrt` → inline integer-Newton block with a bit-length initial
/// guess (no float, so exact for every `i64` incl. `i64::MAX`; overflow-safe;
/// `isqrt(0)==0`; negative `n` panics). Cross-checked vs python3
/// (0, 3, 4, 10, true, false, 31622).
#[test]
fn math_isqrt() {
    let rust = xpile_transpile_to_rust("math_isqrt.py");
    assert!(
        rust.contains("__sn.leading_zeros()"),
        "math.isqrt should lower to an integer-Newton block:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(isqrt_floor(0), 0);
    assert_eq!(isqrt_floor(15), 3);
    assert_eq!(isqrt_floor(16), 4);
    assert_eq!(isqrt_floor(100), 10);
    assert_eq!(is_perfect_square(49), true);
    assert_eq!(is_perfect_square(50), false);
    assert_eq!(isqrt_big(1000000007), 31622);
}
"#;
    assert_rustc_runs("math_isqrt", &rust, driver);
}

/// PMAT-553 (Tranche 2): `math.comb(n, k)` — binomial coefficient. New
/// `Expr::Comb` → inline incremental-product block (`min(k, n-k)` iterations;
/// `k > n` → 0; negative args panic = Python `ValueError`; the running
/// `checked_mul` panics on i64 overflow per the int-arith contract).
/// Cross-checked vs python3 (120, 2598960, 0, 2).
#[test]
fn math_comb() {
    let rust = xpile_transpile_to_rust("math_comb.py");
    assert!(
        rust.contains("__cr.checked_mul(__cn - __ci)"),
        "math.comb should lower to an incremental binomial product:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(choose(10, 3), 120);
    assert_eq!(poker_hands(), 2598960);
    assert_eq!(out_of_range(5, 6), 0);
    assert_eq!(symmetric(7), 2);
}
"#;
    assert_rustc_runs("math_comb", &rust, driver);
}

/// PMAT-554 (Tranche 2): `math.perm(n, k)` — number of `k`-permutations of `n`,
/// `P(n, k) = n! / (n - k)!`, lowered to an inline descending-product block
/// (`∏ (n - i)` for `i` in `0..k`); `k > n` yields `0`, and the one-arg form
/// `math.perm(n)` lowers to `factorial`. Cross-checked vs python3
/// (20, 720, 120, 0, 1).
#[test]
fn math_perm() {
    let rust = xpile_transpile_to_rust("math_perm.py");
    assert!(
        rust.contains("__pr.checked_mul(__pn - __pi)"),
        "math.perm should lower to a descending product block:\n{rust}"
    );
    assert!(
        rust.contains("__f.checked_mul(__fi)"),
        "one-arg math.perm(n) should lower to factorial:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(arrange(5, 2), 20);
    assert_eq!(license_plates(), 720);
    assert_eq!(all_of(5), 120);
    assert_eq!(out_of_range(3, 5), 0);
    assert_eq!(empty_pick(7), 1);
}
"#;
    assert_rustc_runs("math_perm", &rust, driver);
}

/// PMAT-514 (Tranche 2): `match` on an **enum** — dotted value patterns
/// (`case Color.RED:`) and `|`-patterns of them desugar (via the match→if path)
/// to enum-member equality (`c == Color::RED`), combining PMAT-510/512 (`match`)
/// with PMAT-513 (enums). No new IR. Cross-checked vs python3 (warmth 2/1/0,
/// is_primary_pair 1/0/1, label 100/200/300).
#[test]
fn match_enum() {
    let rust = xpile_transpile_to_rust("match_enum.py");
    assert!(
        rust.contains("if (c == Color::RED)")
            && rust.contains("(c == Color::RED) || (c == Color::BLUE)"),
        "match on an enum should compare against `Color::VARIANT`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(warmth(Color::RED), 2);
    assert_eq!(warmth(Color::GREEN), 1);
    assert_eq!(warmth(Color::BLUE), 0);
    assert_eq!(is_primary_pair(Color::RED), 1);
    assert_eq!(is_primary_pair(Color::GREEN), 0);
    assert_eq!(is_primary_pair(Color::BLUE), 1);
    assert_eq!(label(Color::RED), 100);
    assert_eq!(label(Color::GREEN), 200);
    assert_eq!(label(Color::BLUE), 300);
}
"#;
    assert_rustc_runs("match_enum", &rust, driver);
}

/// PMAT-502fc (Tranche 2): two-generator list comprehension
/// `[expr for x in a for y in b]` → nested `for` loops appending to the
/// accumulator (previously a hard "single `for` clause" error). Both generators
/// must have plain-Name targets over `list[T]` iterables; per-generator `if`
/// filters wrap their own loop. Works in both return and assignment position.
/// Cross-checked vs python3.
#[test]
fn list_comp_2gen() {
    let rust = xpile_transpile_to_rust("list_comp_2gen.py");
    // Nested loops: an inner `for y` appears inside the outer `for x` body.
    assert!(
        rust.contains("for x in a.iter().cloned()") && rust.contains("for y in b.iter().cloned()"),
        "two generators should desugar to nested for loops:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(cartesian(vec![1, 2, 3], vec![10, 20]), vec![10, 20, 20, 40, 30, 60]);
    assert_eq!(filtered(vec![-1, 2, 3], vec![5, 20, 7]), vec![7, 9, 8, 10]);
    assert_eq!(pairs(vec![10, 20], vec![1, 2, 3]), vec![9, 8, 7, 19, 18, 17]);
}
"#;
    assert_rustc_runs("list_comp_2gen", &rust, driver);
}

/// PMAT-502fd (Tranche 2): two-generator **dict and set** comprehensions
/// `{k: v for x in a for y in b}` / `{e for x in a for y in b}` → nested `for`
/// loops inserting/adding to the accumulator (mirrors the list 2-gen slice via a
/// shared `desugar_comp_2gen` helper). Per-generator `if` filters wrap their own
/// loop. Cross-checked vs python3 (driver sorts the collected entries since
/// HashMap/HashSet iteration order is nondeterministic).
#[test]
fn comp_2gen_dict_set() {
    let rust = xpile_transpile_to_rust("comp_2gen_dict_set.py");
    assert!(
        rust.contains("for x in a.iter().cloned()") && rust.contains("for y in b.iter().cloned()"),
        "two generators should desugar to nested for loops:\n{rust}"
    );
    let driver = r#"
fn main() {
    let g = grid(vec![1, 2], vec![3, 4]);
    let mut gv: Vec<(i64, i64)> = g.into_iter().collect();
    gv.sort();
    assert_eq!(gv, vec![(13, 4), (14, 5), (23, 5), (24, 6)]);
    let s = sums(vec![1, 2], vec![10, 20]);
    let mut sv: Vec<i64> = s.into_iter().collect();
    sv.sort();
    assert_eq!(sv, vec![11, 12, 21, 22]);
    let f = filtered_set(vec![-1, 2, 3], vec![0, 5]);
    let mut fv: Vec<i64> = f.into_iter().collect();
    fv.sort();
    assert_eq!(fv, vec![10, 15]);
}
"#;
    assert_rustc_runs("comp_2gen_dict_set", &rust, driver);
}

/// PMAT-502ev (Tranche 2): `sorted(s)` over a str — sorts the characters into a
/// list of 1-char strings (via `Expr::StrChars`). Completes the `sorted(X)`
/// family (list / dict-keys / str-chars); `reverse=`/`key=` still apply.
/// Cross-checked vs python3.
#[test]
fn sorted_str() {
    let rust = xpile_transpile_to_rust("sorted_str.py");
    assert!(
        rust.contains(".chars()") && rust.contains(".sort()"),
        "sorted(str) should sort the chars:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(first_char("cba".to_string()), "a");
    assert_eq!(sorted_joined("dbca".to_string()), "abcd");
    assert_eq!(first_char_desc("abc".to_string()), "c");
    assert_eq!(char_count("hello".to_string()), 5);
}
"#;
    assert_rustc_runs("sorted_str", &rust, driver);
}

/// PMAT-502eu (Tranche 2): `sorted(d)` over a dict — Python iterates a dict as
/// its keys, so `sorted(d)` returns the sorted key list. Previously this was a
/// silent miscompile (the dict arg fell through to an undefined `sorted(d)`
/// call typed `i64`). It now materializes the keys (`Expr::DictView{Keys}`) and
/// sorts them; `reverse=`/`key=` still apply. Cross-checked vs python3.
#[test]
fn sorted_dict() {
    let rust = xpile_transpile_to_rust("sorted_dict.py");
    assert!(
        rust.contains(".keys()") && !rust.contains(": i64 = sorted("),
        "sorted(dict) should sort the keys, not emit an undefined sorted():\n{rust}"
    );
    let driver = r#"
fn d() -> std::collections::HashMap<i64, i64> {
    let mut m = std::collections::HashMap::new();
    m.insert(3, 1);
    m.insert(1, 1);
    m.insert(2, 1);
    m
}
fn main() {
    assert_eq!(first_key(d()), 1);
    assert_eq!(last_key(d()), 3);
    assert_eq!(first_key_desc(d()), 3);
    assert_eq!(sum_sorted_keys(d()), 6);
}
"#;
    assert_rustc_runs("sorted_dict", &rust, driver);
}

/// PMAT-502et (Tranche 2): set splat literals — `{*a, *b}`, `{*a, x}`. A set
/// literal containing `*`-splat elements is a union: each `*e` contributes the
/// set `e`, each plain `x` a singleton `{x}`, folded through `Expr::SetOp{Union}`
/// (a fresh `HashSet`). A lone `{*a}` is wrapped in `Expr::Clone` (shallow
/// copy, not a move). Parallels the list-splat handling. Cross-checked vs python3.
#[test]
fn set_spread() {
    let rust = xpile_transpile_to_rust("set_spread.py");
    assert!(
        rust.contains(".union(") && rust.contains(".clone()"),
        "set splat should fold to SetOp::Union (+ Clone for a lone splat):\n{rust}"
    );
    let driver = r#"
fn s(xs: &[i64]) -> std::collections::HashSet<i64> { xs.iter().copied().collect() }
fn main() {
    assert_eq!(union_splat(s(&[1, 2]), s(&[2, 3])), 3);
    assert_eq!(splat_with_elem(s(&[1, 2])), 3);
    assert_eq!(elem_then_splat(s(&[1, 2])), 3);
    assert_eq!(lone_splat_is_copy(s(&[1, 2, 3])), 34); // orig 3, copy 4
}
"#;
    assert_rustc_runs("set_spread", &rust, driver);
}

/// PMAT-502es (Tranche 2): list splat literals — `[*a, *b]`, `[x, *a, y]`. A
/// list literal containing `*`-splat elements is a concatenation: each `*e`
/// contributes the list `e`, each plain `x` a singleton `[x]`, folded through
/// `Expr::ListConcat` (a fresh `Vec`). A lone `[*a]` is wrapped in
/// `Expr::Clone` so it copies rather than moving `a`. Cross-checked vs python3.
#[test]
fn list_spread() {
    let rust = xpile_transpile_to_rust("list_spread.py");
    assert!(
        rust.contains(".iter().chain("),
        "list splat should fold to ListConcat (chain):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(concat(vec![1, 2], vec![3, 4, 5]), 5);
    assert_eq!(spread_sum(vec![1, 2], vec![3, 4]), 10);
    assert_eq!(with_ends(vec![5, 6]), 99); // [0,5,6,99] -> 0*100 + 99
    assert_eq!(lone_spread_is_copy(vec![1, 2, 3]), 34); // orig 3, copy 4
    assert_eq!(
        str_spread(vec!["x".to_string()], vec!["y".to_string(), "z".to_string()]),
        3
    );
}
"#;
    assert_rustc_runs("list_spread", &rust, driver);
}

/// PMAT-502er (Tranche 2): 1-arg `min(xs)` / `max(xs)` reduction over a
/// `list[str]` (and `list[bool]`). Previously the reduction was numeric-only;
/// `str`/`bool` are `Ord`, so the type gate is widened and the codegen uses
/// `.iter().cloned().min()/.max()` (not `.copied()`, since `String` isn't
/// `Copy` — `i64`/`bool` are `Clone` too). Cross-checked vs python3.
#[test]
fn min_max_str_list() {
    let rust = xpile_transpile_to_rust("min_max_str_list.py");
    assert!(
        rust.contains(".iter().cloned().min()") || rust.contains(".iter().cloned().max()"),
        "str-list min/max should use cloned():\n{rust}"
    );
    let driver = r#"
fn w() -> Vec<String> {
    vec!["banana".to_string(), "apple".to_string(), "cherry".to_string()]
}
fn main() {
    assert_eq!(min_word(w()), "apple");
    assert_eq!(max_word(w()), "cherry");
    assert_eq!(min_word_default(vec!["b".to_string(), "a".to_string()]), "a");
    assert_eq!(min_int_regression(vec![3, 1, 2]), 1);
}
"#;
    assert_rustc_runs("min_max_str_list", &rust, driver);
}

/// PMAT-704: variadic `max(a, b, key=fn)` / `min(a, b, c, key=fn)` — the args are
/// the elements (not an iterable); was rejected ("passes keyword args to unknown
/// function max"). Builds a list literal + reuses the `ListMinMax` key path; a key
/// tie returns the first arg (Python). Cross-checked vs python3 (HUNT-V8 V8-13).
#[test]
fn max_min_variadic_key() {
    let rust = xpile_transpile_to_rust("max_min_variadic_key.py");
    let driver = r#"
fn main() {
    assert_eq!(longest("cd".to_string(), "abc".to_string()), "abc");
    assert_eq!(shortest3("xy".to_string(), "z".to_string(), "abcd".to_string()), "z");
    assert_eq!(by_abs(-5, 3), -5); // abs(-5)=5 > abs(3)=3
    assert_eq!(first_on_tie("ab".to_string(), "cd".to_string()), "ab"); // tie → first
}
"#;
    assert_rustc_runs("max_min_variadic_key", &rust, driver);
}

/// PMAT-502eq (Tranche 2): shallow copy — `xs.copy()` / `d.copy()` / `s.copy()`
/// over a list / dict / set → `Expr::Clone` (`(<inner>).clone()`). The copy is
/// independent: mutating it leaves the original unchanged. Cross-checked vs
/// python3.
#[test]
fn collection_copy() {
    let rust = xpile_transpile_to_rust("collection_copy.py");
    assert!(
        rust.contains(").clone()"),
        ".copy() should emit (<inner>).clone():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(list_copy(), 34); // original len 3, copy len 4
    assert_eq!(dict_copy(), 12);
    assert_eq!(set_copy(), 23);
    assert_eq!(copy_param(vec![1, 2, 3, 4]), 5);
}
"#;
    assert_rustc_runs("collection_copy", &rust, driver);
}

/// PMAT-502ep (Tranche 2): set predicates — the methods `a.issubset(b)` /
/// `a.issuperset(b)` / `a.isdisjoint(b)` AND the operators `a <= b` / `a < b` /
/// `a >= b` / `a > b` over two sets. The operators were a silent miscompile
/// (they lowered to a plain ordering `BinOp` → `a <= b` on `HashSet`, which
/// rustc rejects). All now lower to a bool-returning `Expr::SetPred`
/// (`is_subset`/`is_superset`/`is_disjoint`; proper variants add `&& a != b`).
/// `==`/`!=` on sets keep the plain `BinOp` (HashSet `PartialEq`).
/// Cross-checked vs python3.
#[test]
fn set_predicates() {
    let rust = xpile_transpile_to_rust("set_predicates.py");
    assert!(
        rust.contains("is_subset(") && rust.contains("is_disjoint("),
        "set predicates should emit HashSet query methods:\n{rust}"
    );
    let driver = r#"
fn s(xs: &[i64]) -> std::collections::HashSet<i64> { xs.iter().copied().collect() }
fn main() {
    assert!(is_subset(s(&[1, 2]), s(&[1, 2, 3])));
    assert!(!is_subset(s(&[1, 9]), s(&[1, 2, 3])));
    assert!(is_superset(s(&[1, 2, 3]), s(&[1, 2])));
    assert!(is_disjoint(s(&[1, 2]), s(&[3, 4])));
    assert!(!is_disjoint(s(&[1, 2]), s(&[2, 3])));
    assert!(subset_op(s(&[1, 2]), s(&[1, 2]))); // <= is non-strict
    assert!(!proper_subset_op(s(&[1, 2]), s(&[1, 2]))); // < is strict
    assert!(proper_subset_op(s(&[1, 2]), s(&[1, 2, 3])));
    assert!(superset_op(s(&[1, 2, 3]), s(&[1, 2])));
    assert_eq!(guard(s(&[1]), s(&[1, 2])), 1);
}
"#;
    assert_rustc_runs("set_predicates", &rust, driver);
}

/// PMAT-652: set relational predicates must not MOVE their operands. They bound
/// `let __l = <set>` (by value), so a set compared and then reused — or
/// self-compared (`a <= a`) — failed with E0382. The fix binds by reference
/// (`let __l = &(...)`). Cross-checked vs python3.
#[test]
fn set_predicates_no_move() {
    let rust = xpile_transpile_to_rust("set_predicates_no_move.py");
    assert!(
        rust.contains("let __l = &(") && rust.contains("let __r = &("),
        "set predicates must bind operands by reference:\n{rust}"
    );
    let driver = r#"
fn s(xs: &[i64]) -> std::collections::HashSet<i64> { xs.iter().copied().collect() }
fn main() {
    assert_eq!(subset_then_reuse(s(&[1, 2]), s(&[1, 2, 3])), 3); // was E0382
    assert_eq!(disjoint_self(s(&[1, 2])), 0);                    // was E0382
    assert_eq!(subset_self(s(&[1, 2])), 1);                      // was E0382
    assert_eq!(issuperset_then_reuse(s(&[1, 2, 3]), s(&[1, 2])), 3);
}
"#;
    assert_rustc_runs("set_predicates_no_move", &rust, driver);
}

/// PMAT-502eo (Tranche 2): set-algebra *methods* — `a.union(b)` /
/// `a.intersection(b)` / `a.difference(b)` / `a.symmetric_difference(b)`, the
/// method forms of the `|`/`&`/`-`/`^` operators, lowered to the same
/// `Expr::SetOp`. Both receiver and argument must be sets. Cross-checked vs
/// python3.
#[test]
fn set_methods() {
    let rust = xpile_transpile_to_rust("set_methods.py");
    assert!(
        rust.contains(".union(") || rust.contains("union") || rust.contains("intersection"),
        "set methods should lower to SetOp:\n{rust}"
    );
    let driver = r#"
fn s(xs: &[i64]) -> std::collections::HashSet<i64> { xs.iter().copied().collect() }
fn main() {
    assert_eq!(union_size(s(&[1, 2, 3]), s(&[2, 3, 4])), 4);
    assert_eq!(intersection_size(s(&[1, 2, 3]), s(&[2, 3, 4])), 2);
    assert_eq!(difference_size(s(&[1, 2, 3]), s(&[2, 3, 4])), 1);
    assert_eq!(sym_diff_size(s(&[1, 2, 3]), s(&[2, 3, 4])), 2);
    assert!(union_contains(s(&[1, 2]), s(&[3, 4]), 4));
}
"#;
    assert_rustc_runs("set_methods", &rust, driver);
}

/// PMAT-502en (Tranche 2): 2-arg `math` float methods — `math.hypot(x, y)`,
/// `math.atan2(y, x)`, and 2-arg `math.log(x, base)`. Each reuses
/// `Expr::FloatBinOp` (new `FloatOp` variants) with both operands coerced to
/// f64, emitting `(a).hypot(b)` / `(a).atan2(b)` / `(a).log(b)`. 1-arg
/// `math.log` is still natural log (`Ln`). Cross-checked vs python3.
#[test]
fn math_2arg() {
    let rust = xpile_transpile_to_rust("math_2arg.py");
    assert!(
        rust.contains(".hypot(") && rust.contains(".atan2(") && rust.contains(".log("),
        "2-arg math should emit the f64 2-arg methods:\n{rust}"
    );
    let driver = r#"
fn approx(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }
fn main() {
    assert_eq!(hypotenuse(3.0, 4.0), 5.0);
    assert!(approx(angle(1.0, 1.0), std::f64::consts::FRAC_PI_4));
    assert!(approx(log_base(8.0, 2.0), 3.0));
    assert!(approx(log_base(100.0, 10.0), 2.0));
    assert!(approx(natural_log(std::f64::consts::E), 1.0));
}
"#;
    assert_rustc_runs("math_2arg", &rust, driver);
}

/// PMAT-502em (Tranche 2): `math.pow(x, y)` and `math.trunc(x)`. `math.pow`
/// always returns `float` (even for int args, unlike the builtin `pow`), so it
/// reuses `FloatBinOp{Pow}` with both operands coerced to f64 → `(x).powf(y)`.
/// `math.trunc` truncates toward zero and returns `int` → `(x).trunc() as i64`
/// (a new `NumBuiltinOp` variant, like `floor`/`ceil`). Cross-checked vs python3.
#[test]
fn math_pow_trunc() {
    let rust = xpile_transpile_to_rust("math_pow_trunc.py");
    assert!(
        rust.contains(".powf(") && rust.contains(").trunc(); if !__mf.is_finite()"),
        "math.pow → powf, math.trunc → guarded trunc:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(power(2.0, 10.0), 1024.0);
    assert_eq!(power_int_args(), 8.0);
    assert_eq!(power_expr(3.0, 2.0), 10.0);
    assert_eq!(trunc_pos(3.7), 3);
    assert_eq!(trunc_neg(-3.7), -3); // toward zero, not floor (-4)
}
"#;
    assert_rustc_runs("math_pow_trunc", &rust, driver);
}

/// PMAT-502el (Tranche 2): more `math` — the constants `math.pi`/`math.e`/
/// `math.tau` (bare attribute reads → `Expr::LitFloat`) and the float functions
/// `sin`/`cos`/`tan`/`exp`/`log`(ln)/`log10`/`log2` (→ `Expr::NumBuiltin`,
/// emitting the matching f64 method). Cross-checked vs python3 (value-tolerant
/// for the transcendental functions).
#[test]
fn math_more() {
    let rust = xpile_transpile_to_rust("math_more.py");
    assert!(
        rust.contains("3.141592653589793") && rust.contains(".sin()") && rust.contains(".ln()"),
        "math constants/functions should emit the pi literal + f64 methods:\n{rust}"
    );
    let driver = r#"
fn approx(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }
fn main() {
    assert_eq!(circle_area(2.0), std::f64::consts::PI * 4.0);
    assert_eq!(e_const(), std::f64::consts::E);
    assert_eq!(tau_const(), std::f64::consts::TAU);
    assert!(approx(sine(0.0), 0.0));
    assert!(approx(cosine(0.0), 1.0));
    assert!(approx(exp_of(0.0), 1.0));
    assert!(approx(ln_of(std::f64::consts::E), 1.0));
    assert!(approx(log10_of(1000.0), 3.0));
    assert!(approx(log2_of(8.0), 3.0));
}
"#;
    assert_rustc_runs("math_more", &rust, driver);
}

/// PMAT-502ek (Tranche 2): `math` module functions — `import math` is accepted
/// (skipped, like the `__future__` preamble) and `math.sqrt` / `math.floor` /
/// `math.ceil` lower to `Expr::NumBuiltin` (reusing all the inference/codegen
/// machinery): `sqrt` → `(x).sqrt()` (float), `floor`/`ceil` → `(x).floor()/
/// .ceil() as i64` (Python returns int). Other `math.*` names error clearly.
/// Cross-checked vs python3.
#[test]
fn math_module() {
    let rust = xpile_transpile_to_rust("math_module.py");
    assert!(
        rust.contains(".sqrt()") && rust.contains(").floor(); if !__mf.is_finite()"),
        "math fns should emit f64 method calls (floor guarded):\n{rust}"
    );
    let driver = r#"
fn main() {
    // assert on the f64 *value* (Display repr of a whole float differs).
    assert_eq!(root(16.0), 4.0);
    assert_eq!(floor_of(3.7), 3);
    assert_eq!(ceil_of(3.2), 4);
    assert_eq!(hypot(3.0, 4.0), 5.0);
    assert_eq!(floor_neg(-2.5), -3);
}
"#;
    assert_rustc_runs("math_module", &rust, driver);
}

/// PMAT-606: math.floor/ceil/trunc guard the rounded value (finite + i64 range)
/// and fail loud, instead of the saturating `as i64` cast (since Rust 1.45 a
/// huge float → i64::MAX, inf → i64::MAX, nan → 0). Python returns a bignum for
/// a huge float and raises OverflowError(inf)/ValueError(nan). Cross-checked vs
/// python3: ordinary values round normally; inf/huge inputs fail loud (caught
/// via catch_unwind), where the old `as i64` silently saturated.
#[test]
fn math_round_overflow() {
    let rust = xpile_transpile_to_rust("math_round_overflow.py");
    assert!(
        rust.contains("out of i64 range") && rust.contains("non-finite float"),
        "floor/ceil/trunc must guard finite + range:\n{rust}"
    );
    let driver = r#"
fn main() {
    // ordinary values round correctly.
    assert_eq!(fl(3.7), 3);
    assert_eq!(ce(3.2), 4);
    assert_eq!(tr(-3.7), -3);
    // inf must fail loud (Python OverflowError), not saturate to i64::MAX.
    assert!(std::panic::catch_unwind(|| fl(mkinf())).is_err());
    assert!(std::panic::catch_unwind(|| ce(mkinf())).is_err());
    // a huge finite float must fail loud (Python bignum), not saturate.
    assert!(std::panic::catch_unwind(|| tr(1e30)).is_err());
}
"#;
    assert_rustc_runs("math_round_overflow", &rust, driver);
}

/// PMAT-502ej (Tranche 2): directly indexing a block-producing collection —
/// `sorted(xs)[0]`, `reversed(xs)[0]` — was a silent rustc-failure: those
/// lower to a Rust block `{ … }`, and `{block}[i]` mis-parses as a block
/// statement followed by an array literal. The `Expr::Index` codegen now
/// parenthesizes a collection that opens with `{` → `({block})[i]`. Plain
/// `xs[i]` / nested `g[i][j]` are unchanged. Cross-checked vs python3.
#[test]
fn block_index() {
    let rust = xpile_transpile_to_rust("block_index.py");
    assert!(
        // PMAT-764: a block-producing collection is bound by-ref before the
        // tagged bounds-check (`&({ …sort block… })`), then indexed via `__lc`.
        rust.contains("__lc = &({") && rust.contains("__lc[__li].clone()"),
        "a block-producing collection should be bound before indexing:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sorted_first(vec![3, 1, 2]), 1);
    assert_eq!(sorted_last(vec![3, 1, 2]), 3);
    assert_eq!(sorted_key_first(vec![-5, 3, -1]), -1);
    assert_eq!(sorted_reverse_first(vec![3, 1, 2]), 3);
    assert_eq!(reversed_first(vec![3, 1, 2]), 2);
}
"#;
    assert_rustc_runs("block_index", &rust, driver);
}

/// PMAT-502ei (Tranche 2): a bare callable name as the `key=` argument of
/// `min`/`max`/`sorted` (`key=abs`, `key=len`, `key=user_fn`) — previously only
/// `key=lambda p: e` was accepted. The bare name is synthesized into the
/// equivalent `lambda __xpile_k: <name>(__xpile_k)` and lowered through the
/// same `SortKey` path. Cross-checked vs python3.
#[test]
fn sort_key_fn() {
    let rust = xpile_transpile_to_rust("sort_key_fn.py");
    assert!(
        rust.contains("min_by_key") && rust.contains("sort_by_key"),
        "key=<fn> should lower to *_by_key like the lambda form:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(min_by_abs(vec![-5, 3, -1]), -1);
    assert_eq!(max_by_abs(vec![-5, 3, -1]), -5);
    assert_eq!(sorted_by_abs(vec![-5, 3, -1]), -1);
    assert_eq!(sorted_by_len(vec!["aaa".to_string(), "b".to_string(), "cc".to_string()]), 1);
    assert_eq!(min_by_user_fn(vec![-3, 2, -1]), -1);
}
"#;
    assert_rustc_runs("sort_key_fn", &rust, driver);
}

/// PMAT-502eh (Tranche 2): `d.setdefault(k, v)` as a bare statement (the
/// value-position `x = d.setdefault(...)` already worked). Reuses the same
/// `Expr::DictSetDefault` lowering, discarding the result via `let _ = …;`; the
/// mutability pre-walk now scans bare expr-statements so `d` is `let mut`.
/// Cross-checked vs python3.
#[test]
fn dict_setdefault_stmt() {
    let rust = xpile_transpile_to_rust("dict_setdefault_stmt.py");
    assert!(
        rust.contains("let mut d") && rust.contains(".entry(") && rust.contains(".or_insert("),
        "setdefault stmt should mark the dict mut and emit entry().or_insert():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(insert_absent(), 20);
    assert_eq!(keep_present(), 10); // setdefault must not overwrite
    assert_eq!(init_in_loop(vec![1, 1, 2, 3]), 4);
    assert_eq!(str_keys(), 2);
}
"#;
    assert_rustc_runs("dict_setdefault_stmt", &rust, driver);
}

/// PMAT-502eg (Tranche 2): `xs.remove(x)` — remove the first list element equal
/// to `x` (a `Stmt::ListRemoveValue`), panicking ≈ Python `ValueError` when
/// absent. Completes the in-place list-mutator surface alongside
/// append/insert/pop/extend/sort/reverse/clear. Distinct from set `.remove`
/// (by key) — the receiver type disambiguates. Cross-checked vs python3.
#[test]
fn list_remove() {
    let rust = xpile_transpile_to_rust("list_remove.py");
    assert!(
        rust.contains(".position(|__e| *__e == __v)") && rust.contains(".remove(__p)"),
        "list.remove should emit position-find + Vec::remove:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(remove_count(), 3);
    assert_eq!(remove_first_only(), 2); // [1,2,3,2] - first 2 -> [1,3,2], [2]==2
    assert_eq!(remove_then_sum(), 40);
    assert_eq!(remove_str(), 2);
    assert_eq!(remove_param(vec![5, 6, 7], 6), 2);
}
"#;
    assert_rustc_runs("list_remove", &rust, driver);
}

/// PMAT-623: interpolating a list in an f-string. `f"{xs}"` emitted
/// `format!("{}", Vec)`, but `Vec` has no `Display` → E0277. Python renders the
/// list as its repr; xpile now desugars to `"[" + ", ".join([repr(e) for e in
/// xs]) + "]"` (recursive for nested lists, reusing Map/join/Concat/repr — no new
/// IR). Correct for int/float/bool/str/nested element types (where Rust's `{:?}`
/// would diverge — `[true]` vs `[True]`, `["a"]` vs `['a']`). Found by hunt #8
/// (H8-2). Cross-checked vs python3.
#[test]
fn list_fstring_repr() {
    let rust = xpile_transpile_to_rust("list_fstring_repr.py");
    assert!(
        rust.contains(".join(&(String::from(\", \"))[..])") && rust.contains("String::from(\"[\")"),
        "list f-string should desugar to a join-based Python repr:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(ints(vec![1, 2, 3]), "v=[1, 2, 3]");
    assert_eq!(floats(vec![1.5, 2.0]), "v=[1.5, 2.0]");
    assert_eq!(bools(vec![true, false, true]), "v=[True, False, True]");
    assert_eq!(strs(vec!["a".to_string(), "b".to_string()]), "v=['a', 'b']");
    assert_eq!(nested(vec![vec![1, 2], vec![3, 4]]), "grid=[[1, 2], [3, 4]]");
    assert_eq!(empty(vec![]), "[]");
}
"#;
    assert_rustc_runs("list_fstring_repr", &rust, driver);
}

/// PMAT-625: a 1-element tuple type/value emitted Rust `(T)` / `(x)` — a
/// parenthesized value, NOT a real 1-tuple `(T,)` / `(x,)` — so `.0`/indexing
/// failed to compile (E0610). Both the type emitter and the tuple-literal emitter
/// now add the trailing comma for a single element; multi-element tuples are
/// unaffected. Also un-blocks 1-tuple f-string repr (PMAT-624). Cross-checked vs
/// python3.
#[test]
fn one_element_tuple() {
    let rust = xpile_transpile_to_rust("one_element_tuple.py");
    assert!(
        // 1-tuple type + literal both carry the trailing comma...
        rust.contains("(i64,)") && rust.contains("(42i64,)")
            // ...but a 2-tuple does not.
            && rust.contains("(i64, i64)"),
        "1-element tuples need a trailing comma, multi-element must not:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(idx(), 42);
    assert_eq!(from_param((7,)), 7);
    assert_eq!(repr1((42,)), "(42,)");
    assert_eq!(pair((3, 4)), 7);
}
"#;
    assert_rustc_runs("one_element_tuple", &rust, driver);
}

/// PMAT-624: interpolating a tuple in an f-string. `f"{p}"` over a tuple emitted
/// `format!("{}", tuple)`, but tuples have no `Display` → E0277. Python renders
/// the tuple repr; xpile now desugars per-position (heterogeneous) to
/// `"(" + repr(p.0) + ", " + repr(p.1) + ... + ")"`, binding the tuple once.
/// Reuses `pyrepr_of` (so nested tuples/lists work). Found by hunt #8 (H8-12).
/// Cross-checked vs python3. PMAT-625 un-blocks the single-element case.
#[test]
fn tuple_fstring_repr() {
    let rust = xpile_transpile_to_rust("tuple_fstring_repr.py");
    assert!(
        rust.contains("let __tp") && rust.contains("String::from(\"(\")"),
        "tuple f-string should desugar to a per-position Python repr:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pair((1, 2)), "p=(1, 2)");
    assert_eq!(mixed((1, "a".to_string(), true)), "(1, 'a', True)");
    assert_eq!(floaty((3.5, 2)), "(3.5, 2)");
    assert_eq!(nested((1, (2, 3))), "(1, (2, 3))");
    assert_eq!(list_of_pairs(vec![(1, 2), (3, 4)]), "[(1, 2), (3, 4)]");
}
"#;
    assert_rustc_runs("tuple_fstring_repr", &rust, driver);
}

/// PMAT-502ef (Tranche 2): a `float` field in an f-string must render Python
/// repr (`3.0`), not Rust's `Display` (`3` for a whole float) — a silent
/// miscompile (`f"v={x}"` produced "v=3"). A float field now reuses the same
/// `Expr::ToStr { of_float: true }` conversion `str(float)` uses, which also
/// un-defers a lone `f"{x}"`. Cross-checked vs python3.
#[test]
fn fstring_float() {
    let rust = xpile_transpile_to_rust("fstring_float.py");
    assert!(
        rust.contains(".fract() == 0.0") && rust.contains("{}.0"),
        "float f-string field should emit the Python-repr conversion:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(value_line(3.0), "v=3.0");
    assert_eq!(lone_float(2.5), "2.5");
    assert_eq!(two_floats(1.0, 2.5), "1.0+2.5");
    assert_eq!(sum_field(1.5, 2.0), "sum=3.5");
    assert_eq!(with_precision(3.14159), "3.14");
}
"#;
    assert_rustc_runs("fstring_float", &rust, driver);
}

/// PMAT-502ee (Tranche 2): a `bool` field in an f-string must render
/// Python-style `True`/`False`, not Rust's lowercase `Display` (`true`/`false`)
/// — a silent miscompile (`f"flag={flag}"` produced "flag=true"). A bool field
/// now desugars to `"True" if b else "False"` (the same conversion `str(bool)`
/// already used), which also un-defers a lone `f"{flag}"`. Cross-checked vs
/// python3.
#[test]
fn fstring_bool() {
    let rust = xpile_transpile_to_rust("fstring_bool.py");
    assert!(
        rust.contains("\"True\"") && rust.contains("\"False\"") && !rust.contains("\"flag=true\""),
        "bool f-string field should render Python-style True/False:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(flag_line(true), "flag=True");
    assert_eq!(flag_line(false), "flag=False");
    assert_eq!(lone_bool(true), "True");
    assert_eq!(two_bools(true, false), "True and False");
    assert_eq!(comparison_field(3, 3), "eq=True");
    assert_eq!(comparison_field(3, 4), "eq=False");
    assert_eq!(mixed(7, true), "n=7 ok=True");
}
"#;
    assert_rustc_runs("fstring_bool", &rust, driver);
}

/// PMAT-502ed (Tranche 2): f-string fixes — (1) a lone `f"{n}"` field (no
/// surrounding text or spec) over an `int` was returning the bare value (typed
/// `i64`, failing the `-> str` check); it now stringifies to `format!("{:}", n)`.
/// (2) integer format specs `:x` / `:X` / `:b` / `:o` (radix), bare width `:5`,
/// and zero-pad `:05` / `:04x` / `:08b` now translate (Rust's int spec syntax
/// matches Python's). Cross-checked vs python3.
#[test]
fn fstring_specs() {
    let rust = xpile_transpile_to_rust("fstring_specs.py");
    assert!(
        rust.contains("format!(\"{:}\"") && rust.contains("{:x}") && rust.contains("{:08b}"),
        "f-string specs should emit stringified lone field + radix/pad specs:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(lone_field(42), "42");
    assert_eq!(hex_lower(255), "ff");
    assert_eq!(hex_upper(255), "FF");
    assert_eq!(binary(5), "101");
    assert_eq!(octal(8), "10");
    assert_eq!(width(42), "[   42]");
    assert_eq!(zero_pad(42), "00042");
    assert_eq!(zero_pad_hex(255), "00ff");
    assert_eq!(zero_pad_binary(5), "00000101");
    assert_eq!(mixed(255), "n=255 hex=0xff");
}
"#;
    assert_rustc_runs("fstring_specs", &rust, driver);
}

/// PMAT-568 (Tranche 2): **correctness** — Python tie/stability semantics for
/// `max(key=)` and `sorted(reverse=True)`. `max(xs, key=)` returns the FIRST
/// maximal element (Rust `max_by_key` returns the last → reverse the iterator);
/// `sorted(key=, reverse=True)` is STABLE (equal keys keep original order —
/// `sort_by_key`+`reverse` flips them → stable descending comparator). `min` is
/// unchanged. Found by the differential hunt. Cross-checked vs python3
/// (3, 3, 3, 135246, 8).
#[test]
fn minmax_sort_tie() {
    let rust = xpile_transpile_to_rust("minmax_sort_tie.py");
    assert!(
        rust.contains(".cloned().rev().max_by_key(") && rust.contains("__kb.cmp(&__ka)"),
        "expected reversed max + stable-descending sort:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(max_first_tie(vec![3, -3, 1]), 3);
    assert_eq!(max_alltie(vec![3, 1, 2]), 3);
    assert_eq!(min_first_tie(vec![3, 1, 2]), 3);
    assert_eq!(
        stable_rev(vec![(1, 1), (1, 3), (1, 5), (0, 2), (0, 4), (0, 6)]),
        135246
    );
    assert_eq!(desc_key(vec![23, 17, 45, 8]), 8);
}
"#;
    assert_rustc_runs("minmax_sort_tie", &rust, driver);
}

/// PMAT-567 (Tranche 2): **correctness** — str slicing `s[a:b]` indexes by
/// Unicode **characters**, not bytes. Was a byte slice → wrong result on
/// non-ASCII AND a char-boundary **panic** (`s[1:3]` on "αβγδ"). Now collects
/// the str to a `Vec<char>` and slices that (`__sl[..].iter().collect()`); list
/// slicing is unchanged. Found by the differential hunt. Cross-checked vs
/// python3 (βγ, hél, βγ, caf, γδ, éllo, ell, 20).
#[test]
fn str_slice_char() {
    let rust = xpile_transpile_to_rust("str_slice_char.py");
    assert!(
        rust.contains("Vec<char> = (") && rust.contains(".iter().collect::<String>()"),
        "str slice should collect to Vec<char>:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(mid("αβγδ".to_string()), "βγ");
    assert_eq!(prefix("héllo".to_string(), 3), "hél");
    assert_eq!(suffix("αβγ".to_string()), "βγ");
    assert_eq!(drop_last("café".to_string()), "caf");
    assert_eq!(from_neg("αβγδ".to_string()), "γδ");
    assert_eq!(oob("héllo".to_string()), "éllo");
    assert_eq!(ascii_slice("hello".to_string()), "ell");
    assert_eq!(list_slice(vec![10, 20, 30, 40]), 20);
}
"#;
    assert_rustc_runs("str_slice_char", &rust, driver);
}

/// PMAT-633: stepped string slices `s[a:b:step]` — str gains full step parity
/// with list slices (previously deferred / rejected). Char-indexed via the
/// `Vec<char>` path (Unicode-correct); a positive step `s[::k]`/`s[a:b:k]`
/// collects every k-th char, an unbounded negative step `s[::-k]` reverses then
/// steps. The `s[::-1]` reverse special case is unaffected. Cross-checked vs
/// python3, including Unicode input.
#[test]
fn str_stepped_slice() {
    let rust = xpile_transpile_to_rust("str_stepped_slice.py");
    // str step collects into String (not Vec) off the Vec<char> path.
    assert!(
        rust.contains(".step_by(2usize).collect::<String>()")
            || rust.contains(".step_by(2).collect::<String>()"),
        "str step should collect to String:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(every_other("abcdefgh".to_string()), "aceg");
    assert_eq!(bounded_step("abcdefgh".to_string()), "bdfh");
    assert_eq!(every_third("abcdefghij".to_string()), "adgj");
    assert_eq!(rev_every_other("abcdef".to_string()), "fdb");
    // Unicode: char-indexed, not byte-indexed.
    assert_eq!(unicode_step("αβγδεζ".to_string()), "αγε");
    assert_eq!(still_reverse("abcde".to_string()), "edcba");
}
"#;
    assert_rustc_runs("str_stepped_slice", &rust, driver);
}

/// PMAT-566 (Tranche 2): **correctness** — `str.find/rfind/index/rindex` return
/// a Python **character** index, not Rust's byte offset. Was
/// `.find(...).map(|i| i as i64)` (byte offset → `"αβγδ".find("γ")` == 4); now a
/// block binds the receiver to a temp and counts chars before the match byte
/// (`__s[..__b].chars().count()`). Found by the differential hunt. Cross-checked
/// vs python3 (2, 3, 2, -1, 2).
#[test]
fn str_find_char_index() {
    let rust = xpile_transpile_to_rust("str_find_char_index.py");
    assert!(
        rust.contains("__s[..__b].chars().count() as i64"),
        "find/index should return a char index:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(find_g("αβγδ".to_string(), "γ".to_string()), 2);
    assert_eq!(rfind_a("αβγα".to_string(), "α".to_string()), 3);
    assert_eq!(index_g("héllo".to_string(), "llo".to_string()), 2);
    assert_eq!(not_found("abc".to_string(), "z".to_string()), -1);
    assert_eq!(ascii_find("hello".to_string(), "l".to_string()), 2);
}
"#;
    assert_rustc_runs("str_find_char_index", &rust, driver);
}

/// PMAT-617: **correctness** — `bool` compared with `int`. Python's bool is an
/// int subtype, so `True == 1` / `True < 2` are valid, but xpile emitted a bare
/// `bool OP i64`, which rustc rejects (E0308) — the comparison half of the
/// bool-as-int story (PMAT-565 fixed the arithmetic half). The bool side now
/// coerces to i64; works for the simple and chained (`a <= b < c`) forms.
/// Found by differential hunt #7. Cross-checked vs python3.
#[test]
fn bool_int_compare() {
    let rust = xpile_transpile_to_rust("bool_int_compare.py");
    assert!(
        // the bool side is cast to i64 in mixed comparisons; both-bool stays bare.
        rust.contains("((a) as i64) == b") && rust.contains("(a < b)"),
        "bool/int comparison should coerce the bool side to i64:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(beq(true, 1), true);
    assert_eq!(beq(true, 0), false);
    assert_eq!(ieqb(1, true), true);
    assert_eq!(blt(true, 2), true);
    assert_eq!(blit(true), true);
    assert_eq!(chained(true, 1, 5), true);   // True <= 1 < 5
    assert_eq!(both_bool(false, true), true); // false < true (bool: Ord)
}
"#;
    assert_rustc_runs("bool_int_compare", &rust, driver);
}

/// PMAT-565 (Tranche 2): **correctness** — `bool` is an `int` subtype. Bool
/// operands in integer arithmetic (`a + b`, `bool + int`), `sum(list[bool])` /
/// `sum(x > 0 for x in xs)` (the counting idiom), and `True in list[int]` all
/// previously emitted invalid Rust (`checked_add` on `bool`, bare `sum()`,
/// `contains(&true)`) or rejected. Now bool operands coerce to i64
/// (`(b) as i64`). Found by the differential hunt. Cross-checked vs python3
/// (2, 6, 1, 2, 3, true).
#[test]
fn bool_as_int() {
    let rust = xpile_transpile_to_rust("bool_as_int.py");
    let driver = r#"
fn main() {
    assert_eq!(add_bools(true, true), 2);
    assert_eq!(bool_plus_int(true, 5), 6);
    assert_eq!(bool_sub(true, false), 1);
    assert_eq!(count_positive(vec![5, -3, 0, 2, -1]), 2);
    assert_eq!(sum_bool_list(vec![true, false, true, true]), 3);
    assert_eq!(has_one(vec![3, 1, 2]), true);
}
"#;
    assert_rustc_runs("bool_as_int", &rust, driver);
}

/// PMAT-589 (Tranche 2): **correctness** — `int()` of an out-of-i64-range finite
/// float. Python returns an exact arbitrary-precision integer (`int(1e30)`), but
/// Rust's `as i64` saturates to `i64::MAX` silently. xpile can't represent the
/// bignum yet, so it now fails loud (panic) instead of returning the wrong
/// value — extending the PMAT-586 non-finite guard with a range check. In-range
/// floats truncate toward zero as before. Found by the differential hunt.
/// Cross-checked vs python3.
#[test]
fn int_cast_range() {
    let rust = xpile_transpile_to_rust("int_cast_range.py");
    let driver = r#"
fn main() {
    assert_eq!(ic(3.7), 3);
    assert_eq!(ic(-3.7), -3);
    assert_eq!(ic(9e18), 9_000_000_000_000_000_000); // < 2^63, in range
    // out of i64 range -> panic (Python returns an exact bignum we can't hold)
    assert!(std::panic::catch_unwind(|| ic(1e30)).is_err());
    assert!(std::panic::catch_unwind(|| ic(-1e30)).is_err());
}
"#;
    assert_rustc_runs("int_cast_range", &rust, driver);
}

/// PMAT-588 (Tranche 2): **correctness** — a non-Copy variable passed by value
/// to a function call and read more than once was moved into the call, leaving
/// the other use a use-after-move (rustc E0382 — `helper(xs) + helper(xs)`). Now
/// such a reused call argument is cloned so the caller's binding survives. Gated
/// on read-count > 1, so single-use args are byte-identical (no clone, no perf
/// cost). Second slice of the ownership/borrow cluster. Found by the
/// differential hunt. Cross-checked vs python3.
#[test]
fn call_arg_reuse() {
    let rust = xpile_transpile_to_rust("call_arg_reuse.py");
    // reused arg is cloned; single-use is not.
    assert!(
        rust.contains("helper((xs).clone())"),
        "reused call arg should clone:\n{rust}"
    );
    assert!(
        rust.contains("helper(xs)"),
        "single-use call arg should NOT clone:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(two_calls(vec![1, 2, 3]), 6);
    assert_eq!(call_then_use(vec![4, 5]), 4); // len 2 + len 2
    assert_eq!(single_use(vec![7, 8, 9]), 3);
}
"#;
    assert_rustc_runs("call_arg_reuse", &rust, driver);
}

/// PMAT-587 (Tranche 2): **correctness** — a class/enum named after a Rust
/// prelude type that xpile emits (`Vec`/`String`/`Option`/`Some`/`None`/
/// `HashMap`/`HashSet`) emits a `struct <Name>` that collides with the prelude:
/// a bare unit struct shadows it, but once the module also uses the generic form
/// (a `list[int]` → `Vec<i64>`) rustc rejects it (E0107) — a transpile-success →
/// invalid-Rust break. Now rejected cleanly at lowering with a rename hint,
/// upholding "transpile-success ⟹ valid Rust". Prelude names xpile does NOT
/// emit (`Result`/`Box`/…) still work by shadowing. Found by the differential hunt.
#[test]
fn prelude_type_name_rejected() {
    let py = fixture("prelude_type_name_rejected.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "a class named `Vec` must be refused, not emitted as a colliding struct"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("collides with a Rust prelude type") && stderr.contains("Vec"),
        "the rejection should name the prelude collision:\n{stderr}"
    );
}

/// PMAT-696: a float set element / dict key lowers to `HashSet<f64>` /
/// `HashMap<f64, _>`, which is invalid Rust (`f64: !Eq`, `!Hash` → E0277) — a
/// transpile success that fails `rustc`. It is now rejected at lowering with a
/// clear message instead of emitting uncompilable Rust. (HUNT-V7 item V7-7.)
#[test]
fn float_hashed_key_rejected() {
    let py = fixture("float_hashed_key_rejected.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "a `set[float]` must be refused (HashSet<f64> is E0277), not emitted"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("float-typed set element") && stderr.contains("not `Eq`/`Hash`"),
        "the rejection should explain the f64-hash limitation:\n{stderr}"
    );
}

/// PMAT-696 (companion): a float dict VALUE (not key) is fine — only the key is
/// hashed — so `dict[str, float]` must still transpile and run. Locks in that the
/// float-key reject does not over-reach to values.
#[test]
fn float_dict_value_ok() {
    let rust = xpile_transpile_to_rust("float_dict_value_ok.py");
    let driver = r#"
fn main() {
    assert_eq!(g(), 4.0); // 1.5 + 2.5; cross-checked vs python3
}
"#;
    assert_rustc_runs("float_dict_value_ok", &rust, driver);
}

/// PMAT-586 (Tranche 2): **correctness** — `int()` of a non-finite float. Python
/// raises `OverflowError` for `int(inf)` and `ValueError` for `int(nan)`, but
/// Rust's `as i64` saturates (`inf` → `i64::MAX`) / zeroes (`nan` → 0) silently.
/// Now `int(float_x)` (tracked via a new `from_float` flag on `NumCast`) guards
/// a non-finite source and panics; `int(int)`/`float(_)` unchanged. (The
/// out-of-range *finite* case `int(1e30)` still saturates — a deferred bigint
/// gap.) Found by the differential hunt. Cross-checked vs python3.
#[test]
fn int_cast_nonfinite() {
    let rust = xpile_transpile_to_rust("int_cast_nonfinite.py");
    let driver = r#"
fn main() {
    assert_eq!(ic(3.7), 3);   // truncate toward zero
    assert_eq!(ic(-3.7), -3);
    assert_eq!(ii(5), 5);     // int(int) identity
    // non-finite -> Python OverflowError/ValueError -> panic
    assert!(std::panic::catch_unwind(|| ic(f64::INFINITY)).is_err());
    assert!(std::panic::catch_unwind(|| ic(f64::NAN)).is_err());
}
"#;
    assert_rustc_runs("int_cast_nonfinite", &rust, driver);
}

/// PMAT-585 (Tranche 2): **correctness** — returning a non-Copy field by value
/// from `&self`. A method/`@property` like `return self.name` (a `String`/list/
/// dict/set/struct field) emitted `(self).name`, which rustc rejects (E0507:
/// move out of a shared reference). Now a non-Copy field read clones; Copy
/// fields (int/float/bool) read by value. Safe to clone unconditionally —
/// a field is never a mutation receiver (`self.items.append(x)` is rejected),
/// so it only appears in read positions. First slice of the ownership/borrow
/// cluster. Found by the differential hunt. Cross-checked vs python3.
#[test]
fn clone_field_read() {
    let rust = xpile_transpile_to_rust("clone_field_read.py");
    let driver = r#"
fn main() {
    let p = P { name: String::from("ab"), tags: vec![1, 2], age: 7 };
    assert_eq!(p.get_name(), "ab");
    assert_eq!(p.get_tags(), vec![1, 2]);
    assert_eq!(p.get_age(), 7);
    // the struct is still usable — the getters cloned, didn't move
    assert_eq!(p.name, "ab");
    assert_eq!(p.tags, vec![1, 2]);
}
"#;
    assert_rustc_runs("clone_field_read", &rust, driver);
}

/// PMAT-584 (Tranche 2): **correctness** — `sum()` over a float list. CPython
/// 3.12+ uses Neumaier compensated summation; xpile's naive `.iter().sum()`
/// diverges on catastrophic cancellation (`sum([1.0, 1e16, 1.0, -1e16])` is
/// `2.0`, not `0.0`; `sum([0.1]*10)` is `1.0`, not `0.9999999999999999`). Now
/// the float `Sum` codegen emits the same compensated fold (int `sum` stays
/// exact). Found by the differential hunt. Cross-checked vs python3.
#[test]
fn float_sum_compensated() {
    let rust = xpile_transpile_to_rust("float_sum_compensated.py");
    let driver = r#"
fn main() {
    // compensated -> exact 2.0 (naive would be 0.0)
    assert_eq!(fsum(vec![1.0, 1e16, 1.0, -1e16]), 2.0);
    // compensated -> exact 1.0 (naive would be 0.9999999999999999)
    assert_eq!(fsum_ones(), 1.0);
    assert_eq!(fsum_start(vec![1.5, 2.5], 10.0), 14.0);
    assert_eq!(isum(vec![1, 2, 3, 4]), 10);
}
"#;
    assert_rustc_runs("float_sum_compensated", &rust, driver);
}

/// PMAT-583 (Tranche 2): **correctness** — float scientific notation. CPython
/// prints a float in scientific notation when its decimal exponent is `< -4` or
/// `>= 16` (`1e16` → `1e+16`, `1e-5` → `1e-05`), but Rust's `{}` spells them out
/// (`10000000000000000`). The float `str`/`repr`/`print`/f-string helper now
/// derives the exponent from `{:e}` (exact) and reformats to Python's `e±NN`
/// style above the threshold, keeping the fixed `.0`-if-whole shape below it.
/// Found by the differential hunt. Cross-checked vs python3.
#[test]
fn float_sci_notation() {
    let rust = xpile_transpile_to_rust("float_sci_notation.py");
    let driver = r#"
fn main() {
    assert_eq!(s(1e16), "1e+16");
    assert_eq!(s(1e15), "1000000000000000.0"); // exp 15 -> fixed
    assert_eq!(s(1.5e16), "1.5e+16");
    assert_eq!(s(1e-5), "1e-05");
    assert_eq!(s(1e-4), "0.0001"); // exp -4 -> fixed
    assert_eq!(s(1e100), "1e+100");
    assert_eq!(s(-3.14e-10), "-3.14e-10");
    assert_eq!(s(2.5), "2.5");
    assert_eq!(s(100.0), "100.0");
    assert_eq!(s(-0.0), "-0.0");
}
"#;
    assert_rustc_runs("float_sci_notation", &rust, driver);
}

/// PMAT-582 (Tranche 2): **correctness** — `repr()`. A `repr(x)` call emitted a
/// bare `repr(...)` (rustc E0423: `repr` is a built-in attribute, not a fn) /
/// was rejected as I64. Now `repr(int/float/bool)` reuses the `str()` lowering
/// and `repr(str)` emits a CPython-style quoted form (single quotes, switching
/// to double if the string has a `'` but no `"`; escapes `\`, the quote,
/// `\n`/`\r`/`\t`). Found by the differential hunt. Cross-checked vs python3.
#[test]
fn repr_builtin() {
    let rust = xpile_transpile_to_rust("repr_builtin.py");
    let driver = r#"
fn main() {
    assert_eq!(rs(String::from("hi")), "'hi'");
    assert_eq!(rs(String::from("a'b")), "\"a'b\"");      // has ' but no " -> double
    assert_eq!(rs(String::from("say \"hi\"")), "'say \"hi\"'");
    assert_eq!(rs(String::from("a\nb\tc")), "'a\\nb\\tc'");
    assert_eq!(rs(String::from("back\\slash")), "'back\\\\slash'");
    assert_eq!(ri(42), "42");
    assert_eq!(rf(3.0), "3.0");
}
"#;
    assert_rustc_runs("repr_builtin", &rust, driver);
}

/// PMAT-581 (Tranche 2): **correctness** — float division by zero. Python raises
/// `ZeroDivisionError` for `a / b`, `a // b`, `a % b` when `b == 0.0` (and for
/// int true-division `a / 0`), but xpile emitted bare IEEE ops yielding
/// `inf`/`nan`. Now each binds the divisor to a temp, checks `== 0.0`, and
/// panics (matching Python's raise). Found by the differential hunt.
/// Cross-checked vs python3.
#[test]
fn float_div_zero() {
    let rust = xpile_transpile_to_rust("float_div_zero.py");
    let driver = r#"
fn main() {
    // valid ops unaffected
    assert!((fdiv(7.0, 2.0) - 3.5).abs() < 1e-9);
    assert!((ffloor(7.0, 2.0) - 3.0).abs() < 1e-9);
    assert!((fmod(7.0, 3.0) - 1.0).abs() < 1e-9);
    assert!((idiv(7, 2) - 3.5).abs() < 1e-9);
    // zero divisor -> ZeroDivisionError -> panic (was inf/nan)
    assert!(std::panic::catch_unwind(|| fdiv(1.0, 0.0)).is_err());
    assert!(std::panic::catch_unwind(|| ffloor(1.0, 0.0)).is_err());
    assert!(std::panic::catch_unwind(|| fmod(1.0, 0.0)).is_err());
    assert!(std::panic::catch_unwind(|| idiv(1, 0)).is_err());
}
"#;
    assert_rustc_runs("float_div_zero", &rust, driver);
}

/// PMAT-580 (Tranche 2): **correctness** — `&`/`|`/`^` over two bools returns a
/// bool in Python (`True & False` is `bool`, not `int`), but xpile inferred the
/// result as `int` and coerced both operands to i64 — so a `-> bool` function
/// was REJECTED ("body produces I64"). Now a both-bool bitwise op infers `Bool`
/// and stays a bool op (`a & b`; Rust's `bool: BitAnd` matches); a mixed
/// bool/int op still coerces to i64. Found by the differential hunt.
/// Cross-checked vs python3.
#[test]
fn bool_bitwise() {
    let rust = xpile_transpile_to_rust("bool_bitwise.py");
    let driver = r#"
fn main() {
    assert_eq!(b_and(true, false), false);
    assert_eq!(b_or(true, false), true);
    assert_eq!(b_xor(true, true), false);
    assert_eq!(b_xor(true, false), true);
    assert_eq!(mixed(true, 5), 1); // bool coerces to i64 in a mixed op
}
"#;
    assert_rustc_runs("bool_bitwise", &rust, driver);
}

/// PMAT-579 (Tranche 2): **correctness / contract integrity** — `abs()` of an
/// i64 wrapped at `i64::MIN`. `(x).abs()` returns `i64::MIN` for `i64::MIN`
/// (no overflow check under `-O`), silently wrapping where the contract
/// promises a panic (Python's `abs` is exact). Now an int `abs` emits
/// `.checked_abs().expect(…)`; an f64 `abs` keeps `.abs()` (never overflows).
/// Found by the differential hunt. Cross-checked vs python3.
#[test]
fn abs_overflow() {
    let rust = xpile_transpile_to_rust("abs_overflow.py");
    let driver = r#"
fn main() {
    assert_eq!(a_int(-5), 5);
    assert_eq!(a_int(5), 5);
    assert!((a_float(-3.5) - 3.5).abs() < 1e-9);
    // abs(i64::MIN) must PANIC (contract), not wrap to i64::MIN.
    assert!(std::panic::catch_unwind(|| a_int(i64::MIN)).is_err());
}
"#;
    assert_rustc_runs("abs_overflow", &rust, driver);
}

/// PMAT-578 (Tranche 2): **correctness** — `sorted()` over a float list emitted
/// `Vec<f64>::sort()`, which fails to compile (`f64: Ord` not satisfied — E0277),
/// so a transpile success produced invalid Rust. Now the keyless float case uses
/// `sort_by(|a, b| a.partial_cmp(b).unwrap())` (descending: `b.partial_cmp(a)`),
/// mirroring the in-place `xs.sort()` path; an int list keeps the plain
/// `.sort()`. (PMAT-616: NaN is comparator-safe — see `sorted_float_nan`.)
/// Found by the differential hunt. Cross-checked vs python3.
#[test]
fn sorted_float() {
    let rust = xpile_transpile_to_rust("sorted_float.py");
    let driver = r#"
fn main() {
    assert_eq!(srt(vec![3.5, 1.2, 2.8, 1.2]), vec![1.2, 1.2, 2.8, 3.5]);
    assert_eq!(srt_rev(vec![3.5, 1.2, 2.8]), vec![3.5, 2.8, 1.2]);
    assert_eq!(srt_int(vec![3, 1, 2]), vec![1, 2, 3]);
}
"#;
    assert_rustc_runs("sorted_float", &rust, driver);
}

/// PMAT-616: **correctness/robustness** — sorting a float list that contains NaN
/// emitted `partial_cmp(...).unwrap()`, which PANICS on NaN. Python's `sorted`
/// does NOT raise on NaN (its order is undefined-but-non-crashing, and not
/// replicable by any deterministic comparator, since Python's comparator isn't a
/// valid total order). The comparator now falls back to `Equal`, so the
/// transpiled code no longer panics. Finite-only sorts are unchanged (identical
/// to `.unwrap()` for all finite floats) and remain python3-exact; for NaN we
/// assert only that it does not panic and preserves every element.
#[test]
fn sorted_float_nan() {
    let rust = xpile_transpile_to_rust("sorted_float_nan.py");
    assert!(
        rust.contains("partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal)"),
        "keyless float sort must use a NaN-safe comparator:\n{rust}"
    );
    let driver = r#"
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    // finite sorts stay python3-exact:
    assert_eq!(sort_floats(vec![3.0, 1.0, 2.0]), vec![1.0, 2.0, 3.0]);
    assert_eq!(sort_desc(vec![3.0, 1.0, 2.0]), vec![3.0, 2.0, 1.0]);
    assert_eq!(sort_inplace(vec![2.5, 0.5, 1.5]), vec![0.5, 1.5, 2.5]);
    // a NaN element must NOT panic; all elements preserved, finite part sorted.
    let r = std::panic::catch_unwind(|| sort_floats(vec![f64::NAN, 3.0, 1.0, 2.0]));
    let v = r.expect("sorting a float list with NaN must not panic");
    assert_eq!(v.len(), 4);
    assert_eq!(v.iter().filter(|x| x.is_nan()).count(), 1);
    let finite: Vec<f64> = v.into_iter().filter(|x| !x.is_nan()).collect();
    assert_eq!(finite, vec![1.0, 2.0, 3.0]);
}
"#;
    assert_rustc_runs("sorted_float_nan", &rust, driver);
}

/// PMAT-622: sorting a list whose element *embeds* a float — a tuple with a
/// float (`list[tuple[float, int]]`) or a nested list of floats
/// (`list[list[float]]`) — emitted `Vec::sort()`, which needs `Ord`, but `f64`
/// is not `Ord` → E0277. The keyless float-sort detection now recurses through
/// `Tuple`/`List`, routing to the `partial_cmp` path; an int-only tuple still
/// uses `.sort()`. Found by differential hunt #8 (H8-13). Cross-checked vs python3.
#[test]
fn sort_float_tuple() {
    let rust = xpile_transpile_to_rust("sort_float_tuple.py");
    assert!(
        // float-embedding sorts use partial_cmp...
        rust.contains("partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal)")
            // ...but the int-only tuple sort stays a plain `.sort()`.
            && rust.contains("__xv.sort(); __xv"),
        "float-embedding sorts need partial_cmp; int-only stays .sort():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(srt_tuple(vec![(2.5, 1), (1.5, 2), (1.5, 0)]), vec![(1.5, 0), (1.5, 2), (2.5, 1)]);
    assert_eq!(srt_nested(vec![vec![2.0], vec![1.0], vec![1.0, 0.0]]), vec![vec![1.0], vec![1.0, 0.0], vec![2.0]]);
    assert_eq!(inplace_tuple(vec![(3.0, 1), (1.0, 2)]), vec![(1.0, 2), (3.0, 1)]);
    assert_eq!(srt_int_tuple(vec![(2, 1), (1, 9), (1, 0)]), vec![(1, 0), (1, 9), (2, 1)]);
}
"#;
    assert_rustc_runs("sort_float_tuple", &rust, driver);
}

/// PMAT-603: sorting by a FLOAT-returning `key=` lambda. The key result is `f64`
/// (no `Ord`), so `sort_by_key` was rejected (E0277); the float-key sort now
/// uses `sort_by(partial_cmp)` (ascending, descending-stable, and the in-place
/// `xs.sort(key=)` form). Distinct from PMAT-578 (a float *list*); here the list
/// is int but the *key* is float. Cross-checked vs python3 (all → [1, 2, 3]).
#[test]
fn sort_float_key() {
    let rust = xpile_transpile_to_rust("sort_float_key.py");
    assert!(
        !rust.contains("sort_by_key") && rust.contains(".partial_cmp(&{ let x = __b"),
        "a float key must use sort_by(partial_cmp), not sort_by_key:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(by_half(vec![3, 1, 2]), vec![1, 2, 3]);
    assert_eq!(by_neg_ratio(vec![3, 1, 2]), vec![1, 2, 3]);
    assert_eq!(in_place_scaled(vec![3, 1, 2]), vec![1, 2, 3]);
}
"#;
    assert_rustc_runs("sort_float_key", &rust, driver);
}

/// PMAT-577 (Tranche 2): **correctness** — `x >> n` for a shift amount `n >= 64`.
/// Python defines it (the result saturates to the sign fill: `0` for `x >= 0`,
/// `-1` for `x < 0`), but `checked_shr` returns `None` for `n >= 64`, so the
/// emitted `.expect` panicked where Python returns a value. Now the amount is
/// clamped to 63 (which yields exactly that sign fill); a negative amount still
/// panics, matching Python's `ValueError: negative shift count`. Found by the
/// differential hunt. Cross-checked vs python3.
#[test]
fn right_shift_large_amount() {
    let rust = xpile_transpile_to_rust("right_shift_large_amount.py");
    let driver = r#"
fn main() {
    assert_eq!(sh(20, 2), 5);     // ordinary
    assert_eq!(sh(5, 64), 0);     // n >= 64, positive -> 0 (was a panic)
    assert_eq!(sh(-5, 64), -1);   // n >= 64, negative -> -1 (sign fill)
    assert_eq!(sh(5, 100), 0);
    assert_eq!(sh(-1, 200), -1);
    // negative shift amount -> Python ValueError -> panic
    assert!(std::panic::catch_unwind(|| sh(5, -1)).is_err());
}
"#;
    assert_rustc_runs("right_shift_large_amount", &rust, driver);
}

/// PMAT-576 (Tranche 2): **correctness** — a chained comparison
/// (`a < b < c`, `a == f() == b`) re-evaluated its shared interior operand(s).
/// The lowering desugars to `(a OP b) && (b OP c)` and previously *cloned* the
/// lowered middle operand into both sub-comparisons — evaluating it twice,
/// diverging from Python's evaluate-each-operand-once semantics: a
/// side-effecting middle (`0 < xs.pop() < 100`) popped twice (wrong result /
/// empty-pop panic). Now each operand is bound to a temp once inside an
/// `Expr::Block`, then the sub-comparisons fold over the temps. Single
/// comparisons are unchanged. Found by the differential hunt. Cross-checked vs
/// python3 (pop_in_range single-pop, 4-term, eq-chain).
#[test]
fn chained_compare_side_effect() {
    let rust = xpile_transpile_to_rust("chained_compare_side_effect.py");
    let driver = r#"
fn main() {
    // [42] -> pops 42 once: 0 < 42 < 100 == true. Double-eval would pop the
    // empty list and panic.
    assert_eq!(pop_in_range(vec![42]), true);
    // [5, 200] -> pops 200 once: 0 < 200 < 100 == false. Double-eval would
    // pop 200 then 5 -> (0<200) && (5<100) == true (the bug).
    assert_eq!(pop_in_range(vec![5, 200]), false);
    assert!(four_term(1, 2, 3, 4));
    assert!(!four_term(1, 2, 2, 4));
    assert!(eq_chain(7, 7, 7));
    assert!(!eq_chain(7, 8, 7));
}
"#;
    assert_rustc_runs("chained_compare_side_effect", &rust, driver);
}

/// PMAT-672 (Tranche 2): **correctness** — a chained comparison must
/// SHORT-CIRCUIT like Python: when an earlier sub-comparison is false, the
/// trailing operands are never evaluated. PMAT-576's flat `let __cmpN` + `&&`
/// fold hoisted EVERY operand up front, so a panic-prone trailing operand ran
/// even when an earlier compare was false. PMAT-672 emits a right-nested form
/// (each shared operand bound inside the prior compare's `if`). Cross-checked
/// vs python3: `guard(2, 0)` short-circuits before `100 // 0` (no panic).
#[test]
fn chain_short_circuit() {
    let rust = xpile_transpile_to_rust("chain_short_circuit.py");
    let driver = r#"
fn main() {
    // 10 < 2 is false -> `100 // dv` never evaluated; dv == 0 does NOT panic.
    // The old eager lowering hoisted `100 // 0` -> divide-by-zero panic.
    assert_eq!(guard(2, 0), false);
    // 10 < 20 true, then 20 < (100 // 5 == 20) -> false.
    assert_eq!(guard(20, 5), false);
    // 10 < 15 true, then 15 < (100 // 5 == 20) -> true.
    assert_eq!(guard(15, 5), true);
    assert!(all_true(1, 2, 3));
    assert!(!all_true(1, 2, 2));
}
"#;
    assert_rustc_runs("chain_short_circuit", &rust, driver);
}

/// PMAT-575 (Tranche 2): **correctness / contract integrity** — left-shift
/// value overflow. `x << n` lowered to `checked_shl`, which only returns `None`
/// when the shift *amount* is ≥ 64 — it does NOT detect lost significant bits,
/// so `1 << 63` silently wrapped to `i64::MIN`, falsifying C-PY-INT-ARITH's
/// overflow guarantee (Python's `<<` is exact, so the contract promises a panic
/// until bigint promotion lands). Now emits a reversibility check
/// `(v << n) >> n == v` and panics on overflow. Valid shifts (incl. `-2 << 62`
/// == i64::MIN, which fits exactly) are unaffected. Found by the differential
/// hunt. Cross-checked vs python3.
#[test]
fn left_shift_overflow() {
    let rust = xpile_transpile_to_rust("left_shift_overflow.py");
    let driver = r#"
fn main() {
    // valid shifts — no overflow, must match python3
    assert_eq!(sh(1, 10), 1024);
    assert_eq!(sh(3, 20), 3_145_728);
    assert_eq!(small(), 1024);
    assert_eq!(fits_exactly(), i64::MIN); // -2 << 62 fits exactly
    // value overflow — must PANIC (contract), not silently wrap to i64::MIN
    assert!(std::panic::catch_unwind(|| sh(1, 63)).is_err());
    assert!(std::panic::catch_unwind(|| sh(3, 62)).is_err());
}
"#;
    assert_rustc_runs("left_shift_overflow", &rust, driver);
}

/// PMAT-574 (Tranche 2): **correctness** — a mutating method (`xs.pop()` /
/// `d.setdefault(...)`) in a *controlling condition* (`while`/`if`/`assert`
/// test) mutates its receiver, but the mutability pre-walk only scanned
/// assignment/return/expr-statement values — so the receiver stayed immutable
/// and `rustc` rejected the emitted code (E0596: cannot borrow `xs` as
/// mutable). Now `count_pop_receivers_in_stmt` also scans the `while`/`if`/
/// `for`/`assert` controlling expression (`while` test gets loop bump). Found
/// by the differential hunt. Cross-checked vs python3 (1, 0, "nine", 2).
#[test]
fn mut_receiver_in_condition() {
    let rust = xpile_transpile_to_rust("mut_receiver_in_condition.py");
    let driver = r#"
fn main() {
    assert_eq!(drain_param(vec![3, 2, 1, -1, 5]), 1);
    assert_eq!(drain_local(), 0);
    assert_eq!(check_if(vec![1, 2, 9]), "nine");
    assert_eq!(assert_pop(vec![5, 6, 7]), 2);
}
"#;
    assert_rustc_runs("mut_receiver_in_condition", &rust, driver);
}

/// PMAT-573 (Tranche 2): **correctness** — a Python identifier that is a Rust
/// keyword but NOT a Python keyword (`type`, `match`, `loop`, `move`, `box`,
/// `final`, `ref`, `do`, `impl`, …) emits as the Rust raw identifier `r#name`
/// instead of breaking `rustc` ("expected identifier, found keyword `type`").
/// Covers fn name, param, let, reassignment, for-var, comprehension binder,
/// method receiver (incl. `mut`), and internal call callee — all rewritten
/// consistently by a single IR pre-pass (`escape_rust_reserved_idents`).
/// Cross-checked vs python3 (12, 10, [20, 40], 9).
#[test]
fn rust_keyword_idents() {
    let rust = xpile_transpile_to_rust("rust_keyword_idents.py");
    // The fn `move` itself is `r#move`; everything else is called by its
    // (escaped) name from within the module.
    let driver = r#"
fn main() {
    assert_eq!(r#move(5), 12);
    assert_eq!(process(vec![1, 2, 3, 4]), 10);
    assert_eq!(transform(vec![10, 20]), vec![20, 40]);
    assert_eq!(mutate(vec![7, 8, 9]), 9);
}
"#;
    assert_rustc_runs("rust_keyword_idents", &rust, driver);
}

/// PMAT-564 (Tranche 2): **correctness** — `len(str)` counts Unicode code
/// points, not UTF-8 bytes. Was `s.len()` (byte length → `len("café")` == 5);
/// now routes to `StrMethodOp::CharCount` → `.chars().count() as i64`. `len()`
/// of a list/dict is unchanged (`.len()`). Found by the differential hunt.
/// Cross-checked vs python3 (4, 4, 3, 2, 11, 0).
#[test]
fn len_str_unicode() {
    let rust = xpile_transpile_to_rust("len_str_unicode.py");
    assert!(
        rust.contains("chars().count() as i64"),
        "len(str) should count chars:\n{rust}"
    );
    assert!(
        rust.contains("xs.len() as i64") && rust.contains("d.len() as i64"),
        "len(list)/len(dict) must stay byte/element .len():\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(slen("café".to_string()), 4);
    assert_eq!(lit_len(), 4);
    assert_eq!(list_len(vec![1, 2, 3]), 3);
    let mut d = std::collections::HashMap::new();
    d.insert("a".to_string(), 1);
    d.insert("b".to_string(), 2);
    assert_eq!(dict_len(d), 2);
    assert_eq!(len_in_expr("héllo".to_string()), 11);
    assert_eq!(empty_str(), 0);
}
"#;
    assert_rustc_runs("len_str_unicode", &rust, driver);
}

/// PMAT-674: `len(s.encode())` is the UTF-8 BYTE length of a str (`s.encode()`
/// defaults to UTF-8), which equals Rust `String::len()` — distinct from the
/// code-point count `len(s)` gives. `.encode()` is otherwise an unsupported
/// method call, so the whole idiom rejected; detected in the `len()` intercept
/// and routed to `Expr::Len` (which emits `.len() as i64`, byte length).
/// Cross-checked vs python3 (`café` → 5 bytes / 4 chars, `€` → 3 bytes).
#[test]
fn len_encode_bytes() {
    let rust = xpile_transpile_to_rust("len_encode_bytes.py");
    assert!(
        rust.contains("fn byte_len(s: String) -> i64 {\n    (s.len() as i64)")
            && rust.contains("fn byte_len_utf8(s: String) -> i64 {\n    (s.len() as i64)"),
        "len(s.encode()) should be byte length s.len():\n{rust}"
    );
    assert!(
        rust.contains("fn char_len(s: String) -> i64 {\n    s.chars().count() as i64"),
        "plain len(s) must still count code points:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(byte_len("café".to_string()), 5);       // 4 chars, 5 UTF-8 bytes
    assert_eq!(byte_len_utf8("café".to_string()), 5);
    assert_eq!(char_len("café".to_string()), 4);
    assert_eq!(byte_len("hello".to_string()), 5);
    assert_eq!(byte_len("€".to_string()), 3);          // 1 char, 3 UTF-8 bytes
}
"#;
    assert_rustc_runs("len_encode_bytes", &rust, driver);
}

/// PMAT-675: str `.find(sub, start[, end])` / `.count(sub, start[, end])` — search
/// within the char-slice `s[start:end]`. `find` returns the CHAR index in the
/// ORIGINAL string (or -1); `count` the number of non-overlapping occurrences.
/// start/end are char indices with Python clamping (negative → +len, clamp to
/// [0, len]); end defaults to len for the 2-arg form. Was rejected (">1 arg").
/// Cross-checked vs python3 incl. non-ASCII char indexing and negative start.
#[test]
fn str_find_count_startend() {
    let rust = xpile_transpile_to_rust("str_find_count_startend.py");
    assert!(
        rust.contains(".chars().skip(__st).take(__en.saturating_sub(__st)).collect()"),
        "find/count with start/end should char-slice the receiver:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(find2("abcabc".to_string(), "bc".to_string(), 2), 4);
    assert_eq!(find3("abcabc".to_string(), "bc".to_string(), 0, 3), 1);
    assert_eq!(find3("abcabc".to_string(), "bc".to_string(), 0, 2), -1); // outside slice
    assert_eq!(count2("abcabcabc".to_string(), "bc".to_string(), 3), 2);
    assert_eq!(count3("abcabcabc".to_string(), "bc".to_string(), 0, 6), 2);
    assert_eq!(find_neg("abcabc".to_string(), "bc".to_string()), 4);     // start = -3
    assert_eq!(find2("café au".to_string(), "au".to_string(), 1), 5);    // non-ASCII char index
}
"#;
    assert_rustc_runs("str_find_count_startend", &rust, driver);
}

/// PMAT-676: a bare radix `str.format` spec (`"{:x}".format(n)`) formats
/// negatives SIGN-MAGNITUDE in Python (`"{:x}".format(-255)` == "-ff"), but
/// Rust's `{:x}` is two's-complement (`ffffffffffffff01`) → silent wrong output.
/// Reuse `IntRadixStr` (as the f-string path does) when the arg is referenced
/// exactly once; multi-reference keeps the embed (correct for non-negatives).
/// Cross-checked vs python3.
#[test]
fn format_radix_neg() {
    let rust = xpile_transpile_to_rust("format_radix_neg.py");
    assert!(
        rust.contains("unsigned_abs()") && rust.contains("if __n < 0 { \"-\" }"),
        "bare-radix str.format should use sign-magnitude IntRadixStr:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(hexfmt(255), "ff");
    assert_eq!(hexfmt(-255), "-ff");      // sign-magnitude, NOT two's-complement
    assert_eq!(octfmt(-255), "-377");
    assert_eq!(binfmt(-255), "-11111111");
    assert_eq!(hexup(-255), "-FF");
    assert_eq!(mixed(-255, -5), "-ff and -101");
    assert_eq!(with_text(-255), "val=0x-ff!");
}
"#;
    assert_rustc_runs("format_radix_neg", &rust, driver);
}

/// PMAT-677: a float format spec combining WIDTH/zero-pad/sign/align WITH
/// precision (`f"{x:8.3f}"`, `{:06.2f}`, `{:+8.2f}`, `{:>8.2f}`, `{:*>8.2f}`) was
/// rejected ("unsupported format spec") — `translate_format_spec` only handled
/// pure `.Nf`. Rust's `{:8.3}` etc. match Python exactly for floats (the `.Nf`
/// precision forces the decimals, avoiding the whole-float repr divergence that
/// defers bare float widths). Fixes both f-string and `.format()`. vs python3.
#[test]
fn float_format_width_precision() {
    let rust = xpile_transpile_to_rust("float_format_width_precision.py");
    assert!(
        rust.contains("format!(\"{:8.3}\", x)") && rust.contains("format!(\"{:06.2}\", x)"),
        "float width+precision spec should translate to Rust {{:8.3}}:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(width(3.14159), "   3.142");
    assert_eq!(zero_pad(3.14159), "003.14");
    assert_eq!(sign(3.14159), "   +3.14");
    assert_eq!(wide(3.14159), "    3.1416");
    assert_eq!(align(3.14159), "    3.14");
    assert_eq!(fill_align(3.14159), "****3.14");
    assert_eq!(via_format(3.14159), "   3.142");
    assert_eq!(plain(3.14159), "3.14");
}
"#;
    assert_rustc_runs("float_format_width_precision", &rust, driver);
}

/// PMAT-678: an identity comprehension `[w for w in words]` over `list[str]`
/// must keep the element type — it previously inferred `List(I64)` (the loop var
/// is unbound during the `Expr::Map` body type-inference), mis-typing the result
/// and rejecting `sorted([w for w in words])` / `max(...)` ("body produces
/// List(I64)"). Fixed by binding the loop var to the iterable's element type in
/// the inference arms (mirroring the lowering binding). Cross-checked vs python3.
#[test]
fn identity_comprehension_str() {
    let rust = xpile_transpile_to_rust("identity_comprehension_str.py");
    assert!(
        rust.contains("fn echo_sorted(words: Vec<String>) -> Vec<String>"),
        "identity str comprehension should keep Vec<String>:\n{rust}"
    );
    let driver = r#"
fn main() {
    use std::collections::HashMap;
    assert_eq!(
        echo_sorted(vec!["pear".to_string(), "apple".to_string(), "kiwi".to_string()]),
        vec!["apple".to_string(), "kiwi".to_string(), "pear".to_string()]
    );
    assert_eq!(
        echo_max(vec!["pear".to_string(), "apple".to_string(), "kiwi".to_string()]),
        "pear".to_string()
    );
    assert_eq!(
        filtered(vec!["b".to_string(), "".to_string(), "a".to_string()]),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(ints_regression(vec![3, 1, 2]), vec![1, 2, 3]);
    let mut d = HashMap::new();
    d.insert("b".to_string(), 1);
    d.insert("a".to_string(), 2);
    assert_eq!(pairs(d), vec!["a".to_string(), "b".to_string()]);
}
"#;
    assert_rustc_runs("identity_comprehension_str", &rust, driver);
}

/// PMAT-679: `sum()` of floats uses Neumaier compensation (CPython 3.12+), but a
/// non-finite partial poisoned the result — once the running total is ±inf the
/// compensation term computes `inf - inf = NaN`. Python `sum([1.0, inf, 2.0])` is
/// `inf`, not `NaN`. Fixed by skipping (resetting) the compensation on a
/// non-finite partial; the finite catastrophic-cancellation behavior is preserved.
#[test]
fn sum_float_inf() {
    let rust = xpile_transpile_to_rust("sum_float_inf.py");
    assert!(
        rust.contains("if __st.is_finite()"),
        "float sum should guard compensation on a finite partial:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(total(vec![1.0, f64::INFINITY, 2.0]).is_infinite());      // was NaN
    assert!(total(vec![f64::INFINITY, 1.0]).is_infinite());
    assert!(total(vec![1e308, 1e308]).is_infinite());                 // overflow → inf
    assert_eq!(total(vec![1.0, 2.0, 3.0]), 6.0);
    // Neumaier compensation preserved for the finite cancellation case:
    assert_eq!(total(vec![1.0, 1e16, 1.0, -1e16]), 2.0);
    assert_eq!(total(vec![0.1; 10]), 1.0);
    assert!(total(vec![f64::INFINITY, f64::NEG_INFINITY]).is_nan());  // matches Python (nan)
}
"#;
    assert_rustc_runs("sum_float_inf", &rust, driver);
}

/// PMAT-680: a bare float-precision format spec over NaN must print "nan"
/// (Python), not Rust's "NaN". The f-string path already guarded this (PMAT-659);
/// `.format()` and `%` inlined the spec into a bare `format!`, bypassing it.
/// Fixed by routing the F64 arg through `Expr::FormatSpec` in both lowering paths
/// (so the codegen NaN-guard fires). Cross-checked vs python3.
#[test]
fn format_float_nan() {
    let rust = xpile_transpile_to_rust("format_float_nan.py");
    assert!(
        rust.contains("if __nf.is_nan() { String::from(\"nan\") }"),
        ".format()/% float spec should NaN-guard via FormatSpec:\n{rust}"
    );
    let driver = r#"
fn main() {
    let n = f64::NAN;
    assert_eq!(via_format(n), "nan");      // was "NaN"
    assert_eq!(via_percent(n), "nan");     // was "NaN"
    assert_eq!(pct_default(n), "nan");     // %f default precision
    assert_eq!(via_fstring(n), "nan");     // already correct (regression)
    assert_eq!(normal(3.14159), "3.14");   // normal value unaffected
}
"#;
    assert_rustc_runs("format_float_nan", &rust, driver);
}

/// PMAT-681: `bool(x)` over a float is `x != 0.0` — was rejected ("float
/// deferred") despite the implicit `if x:` float-truthiness path already doing
/// this. Rust `x != 0.0` matches Python exactly: 0.0 / -0.0 are falsy, NaN/inf
/// are truthy. Cross-checked vs python3.
#[test]
fn bool_float() {
    let rust = xpile_transpile_to_rust("bool_float.py");
    assert!(
        rust.contains("(x != 0f64)"),
        "bool(float) should lower to x != 0.0:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(is_nonzero(0.0), false);
    assert_eq!(is_nonzero(-0.0), false);
    assert_eq!(is_nonzero(1.5), true);
    assert_eq!(is_nonzero(-2.0), true);
    assert_eq!(is_nonzero(f64::NAN), true);       // NaN is truthy in Python
    assert_eq!(is_nonzero(f64::INFINITY), true);
}
"#;
    assert_rustc_runs("bool_float", &rust, driver);
}

/// PMAT-682: a bare WIDTH format spec on a str (`f"{s:5}"` / `"{:8}".format(s)`)
/// LEFT-aligns to the width in Python (`f"{'ab':5}"` == "ab   "); Rust's `{:5}`
/// over a `String` is also left-aligned (Display default), so it passes through.
/// Was rejected ("unsupported format spec :5 for a Str value"). Must NOT reuse
/// the int width path (ints right-align). Cross-checked vs python3.
#[test]
fn str_width_format() {
    let rust = xpile_transpile_to_rust("str_width_format.py");
    assert!(
        rust.contains(r#"format!("{:5}", s)"#) && rust.contains(r#"format!("{:8}", s)"#),
        "str width spec should pass through as left-aligned {{:N}}:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(col("ab".to_string()), "ab   ");          // left-aligned, width 5
    assert_eq!(col10("ab".to_string()), "ab        ");
    assert_eq!(via_format("ab".to_string()), "ab      "); // .format() path, width 8
    assert_eq!(right("ab".to_string()), "   ab");         // explicit right-align (regression)
    assert_eq!(overflow("ab".to_string()), "ab ");
    assert_eq!(overflow("hello".to_string()), "hello");   // wider than width → no truncation
}
"#;
    assert_rustc_runs("str_width_format", &rust, driver);
}

/// PMAT-683: a non-literal chained assignment `a = b = <expr>` over a Copy scalar
/// (int/float/bool) — `a = b = n + 1`, `x = y = z = n * 2` — was rejected ("only a
/// scalar literal is supported"). Now bound once to a temp and copied into each
/// target (a Copy scalar assigns to several targets without a move; non-Copy
/// values still reject). Cross-checked vs python3.
#[test]
fn chained_assign_scalar() {
    let rust = xpile_transpile_to_rust("chained_assign_scalar.py");
    assert!(
        rust.contains("let __chain") && rust.contains("let a: i64 = __chain;"),
        "non-literal chained assign should bind once to a temp:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(chain_int(5), 12);            // a = b = 6 → 12
    assert_eq!(chain_three(4), 24);          // x = y = z = 8 → 24
    assert_eq!(chain_float(2.0), 6.0);       // a = b = 3.0 → 6.0
    assert_eq!(chain_bool(3), true);
    assert_eq!(chain_bool(-1), false);
    assert_eq!(chain_literal(10), 20);       // literal still works
}
"#;
    assert_rustc_runs("chained_assign_scalar", &rust, driver);
}

/// PMAT-684: `enumerate(xs, start)` / `enumerate(xs, start=N)` inside a list
/// comprehension (`[(i, x) for i, x in enumerate(xs, 1)]`) was rejected (only the
/// for-loop form handled a start; the expr form fell through to a misleading
/// "expected an iterable of 2-tuples" error). `Expr::Enumerate` now carries a
/// start offset, added to the index via `checked_add`. Cross-checked vs python3.
#[test]
fn enumerate_start_comprehension() {
    let rust = xpile_transpile_to_rust("enumerate_start_comprehension.py");
    assert!(
        rust.contains("checked_add(1i64)") && rust.contains("checked_add(10i64)"),
        "enumerate start in a comprehension should offset the index:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(
        numbered(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        vec!["1. a".to_string(), "2. b".to_string(), "3. c".to_string()]
    );
    assert_eq!(
        kw_start(vec!["x".to_string(), "y".to_string()]),
        vec!["10:x".to_string(), "11:y".to_string()]
    );
    assert_eq!(no_start(vec!["p".to_string(), "q".to_string(), "r".to_string()]), vec![0, 1, 2]);
    assert_eq!(neg_start(vec![5, 6, 7]), vec![4, 6, 8]);   // start = -1
}
"#;
    assert_rustc_runs("enumerate_start_comprehension", &rust, driver);
}

/// PMAT-685: `xs.extend(s)` over a str arg iterates the string's CHARACTERS in
/// Python (each a 1-char str), appending each. It emitted `(s).iter().cloned()`
/// on a `String` → E0599; now the str arg is converted to its chars list
/// (`Expr::StrChars`). Cross-checked vs python3 (incl. non-ASCII).
#[test]
fn list_extend_str() {
    let rust = xpile_transpile_to_rust("list_extend_str.py");
    assert!(
        rust.contains("(w).chars().map(|__c| __c.to_string())"),
        "list.extend(str) should iterate the string's chars:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(
        add_chars(vec!["a".to_string()], "bcd".to_string()),
        vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()]
    );
    assert_eq!(
        add_chars(vec![], "héllo".to_string()),   // non-ASCII: é is one char
        vec!["h".to_string(), "é".to_string(), "l".to_string(), "l".to_string(), "o".to_string()]
    );
    assert_eq!(list_extend_regression(vec![1, 2], vec![3, 4]), vec![1, 2, 3, 4]);
}
"#;
    assert_rustc_runs("list_extend_str", &rust, driver);
}

/// PMAT-686: an int LITERAL returned where a float is expected (a `-> float`
/// function with a float-returning branch) is emitted as a float literal so it
/// compiles — `return 0` → `0f64`. Previously the mixed-branch body rejected as
/// "produces I64" / would emit `0i64` from an f64 fn (E0308). An int *variable*
/// return is NOT coerced (Python doesn't coerce; stays a reject). vs python3.
#[test]
fn float_return_int_literal() {
    let rust = xpile_transpile_to_rust("float_return_int_literal.py");
    assert!(
        rust.contains("{ 0f64 }") && rust.contains("return 1f64;"),
        "int literal in float-return position should emit a float literal:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(safe_div(10.0, 2.0), 5.0);
    assert_eq!(safe_div(10.0, 0.0), 0.0);   // the `return 0` branch, now 0.0
    assert_eq!(pick(true), 1.0);            // early `return 1` → 1.0
    assert_eq!(pick(false), 2.5);
    assert_eq!(neg_lit(), -3.0);            // negative literal
}
"#;
    assert_rustc_runs("float_return_int_literal", &rust, driver);
}

/// PMAT-687: Python's `for … else:` / `while … else:` — the `else` runs iff the
/// loop completed WITHOUT `break`. Was a clean reject; now desugared via a fresh
/// `__brokeN` flag set before each break (at this loop's level — not nested
/// loops), then `if !__brokeN { else }`. Cross-checked vs python3.
#[test]
fn loop_else() {
    let rust = xpile_transpile_to_rust("loop_else.py");
    assert!(
        rust.contains("let mut __broke") && rust.contains("__broke") && rust.contains("= true;"),
        "loop-else should desugar via a __broke flag:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(find(vec![1, 2, 3], 2), "found@2");   // return in loop, no else
    assert_eq!(find(vec![1, 2], 9), "not found");    // loop completes → else runs
    assert_eq!(has_break(vec![1, 2, 3], 2), "broke");      // break → else skipped
    assert_eq!(has_break(vec![1, 2], 9), "completed");     // no break → else runs
    assert_eq!(while_else(2), "ran out");            // while completes → else
    assert_eq!(while_else(10), "hit 3");             // break at i==3 → else skipped
}
"#;
    assert_rustc_runs("loop_else", &rust, driver);
}

/// PMAT-698: a var bound BEFORE a loop and reassigned ONLY in the loop's `else`
/// clause must be `let mut` — `walk_counts` recursed only into the loop body, so
/// the else-reassignment hit rustc E0384. Now it folds the `orelse` counts.
/// Cross-checked vs python3 (HUNT-V8 item V8-1).
#[test]
fn loop_else_mut() {
    let rust = xpile_transpile_to_rust("loop_else_mut.py");
    assert!(
        rust.contains("let mut r"),
        "an else-clause-reassigned flag must be `let mut`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(find_or(vec![1, 2, 3], 2), "found");   // break → else skipped
    assert_eq!(find_or(vec![1, 2, 3], 9), "missing"); // no break → else reassigns r
    assert_eq!(scan(2), "exhausted");                 // while completes → else
    assert_eq!(scan(10), "ok");                       // break at i==3 → else skipped
}
"#;
    assert_rustc_runs("loop_else_mut", &rust, driver);
}

/// PMAT-705: a nested-tuple `for`-pair target (`for i, (a, b) in enumerate(xs)`,
/// `for k, (a, b) in d.items()`) was rejected ("non-Name for target"). Desugared
/// to a fresh temp + a prepended `(a, b) = __temp` unpack (reusing the pair-loop +
/// tuple-unpack machinery); a nested var may be mutated. Cross-checked vs python3
/// (HUNT-V8 item V8-8).
#[test]
fn for_nested_pair_target() {
    let rust = xpile_transpile_to_rust("for_nested_pair_target.py");
    assert!(
        rust.contains("__xpile_pair") && rust.contains("let (a, b)"),
        "nested for target should bind a temp + destructure:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(enum_nested(vec![(1, 2), (3, 4)]), 11); // (0+1+2)+(1+3+4)
    let mut d = std::collections::HashMap::new();
    d.insert("x".to_string(), (1, 2));
    d.insert("y".to_string(), (3, 4));
    assert_eq!(items_nested(d), 10);
    assert_eq!(mutate_nested(vec![(1, 2), (3, 4)]), 30); // a += 10 each
}
"#;
    assert_rustc_runs("for_nested_pair_target", &rust, driver);
}

/// PMAT-688: a walrus `(t := E)` in an `if` condition (evaluated once) is hoisted
/// to `let mut t = E;` before the `if` (Python leaks `t` to the enclosing scope),
/// so the body / following code can use `t`. Was an opaque `Discriminant(1)`
/// reject. Restricted to unconditional positions (single compare / arithmetic) —
/// short-circuiting `and`/`or` walruses reject cleanly. Cross-checked vs python3.
#[test]
fn walrus_if() {
    let rust = xpile_transpile_to_rust("walrus_if.py");
    assert!(
        rust.contains("let t: i64 =") && rust.contains("if (t > 10i64)"),
        "if-condition walrus should hoist `let t = …` before the if:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(f(20), 21);    // t := 21 > 10 → return t
    assert_eq!(f(5), 0);      // t := 6, not > 10
    assert_eq!(bare(7), 14);  // t := 7 truthy → t*2
    assert_eq!(bare(0), -1);  // t := 0 falsy
    assert_eq!(arith(10), 10);
    assert_eq!(arith(1), 0);  // (1)+5 = 6, not > 8
}
"#;
    assert_rustc_runs("walrus_if", &rust, driver);
}

/// PMAT-689: `any(P(x) for x in xs)` / `all(...)` over a GENERATOR expression must
/// SHORT-CIRCUIT like Python — the predicate was eagerly `.map()`-ed over every
/// element then reduced, panicking on a not-yet-needed element (e.g. div-by-zero)
/// Python never reaches. Fixed by fusing the predicate into the `any`/`all`
/// closure (Rust's any/all short-circuit). A LIST comprehension `any([...])` is
/// eager in Python, so it is NOT fused. Cross-checked vs python3.
#[test]
fn any_all_short_circuit() {
    let rust = xpile_transpile_to_rust("any_all_short_circuit.py");
    assert!(
        rust.contains("fn has_big(xs: Vec<i64>) -> bool {\n    xs.iter().cloned().any(|__k|"),
        "genexpr any() should fuse the predicate into a short-circuiting .any():\n{rust}"
    );
    assert!(
        rust.contains("collect::<Vec<_>>().iter().any"),
        "list-comp any([...]) must stay EAGER (not fused):\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(has_big(vec![1, 0]), true);      // short-circuits at x=1 (no div-by-0 panic)
    assert_eq!(has_big(vec![100, 200]), true);
    assert_eq!(all_short(vec![5, -1, 0]), false); // short-circuits at -1 (never divides by 0)
    assert_eq!(all_short(vec![5, 10]), true);
    assert_eq!(any_truthy(vec![0, 0, 3]), true);
    assert_eq!(any_truthy(vec![0, 0]), false);
    assert_eq!(listcomp_eager(vec![3, 9]), true);  // eager list-comp, no erroring element
    assert_eq!(listcomp_eager(vec![1, 2]), false);
}
"#;
    assert_rustc_runs("any_all_short_circuit", &rust, driver);
}

/// PMAT-690: `None` in value positions over `Optional[T]`, previously rejected
/// ("unsupported constant: Discriminant(0)"). (1) an Optional accumulator
/// `result: Optional[int] = None` lowers to `let mut result: Option<i64> = None;`
/// and a later `result = v` wraps in `Some(v)`; (2) `x == None` / `x != None` map
/// to `.is_none()` / `.is_some()` over an Optional operand. Cross-checked vs python3.
#[test]
fn none_value_literal() {
    let rust = xpile_transpile_to_rust("none_value_literal.py");
    assert!(
        rust.contains("let mut result: Option<i64> = None;")
            && rust.contains("result = Some(x);")
            && rust.contains("(x).is_none()")
            && rust.contains("(x).is_some()"),
        "None value-position lowering:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(first_even(vec![1, 3, 4, 7, 8]), Some(8));   // accumulator, last even
    assert_eq!(first_even(vec![1, 3, 5]), None);            // never assigned
    assert_eq!(is_unset(None), true);                        // == None
    assert_eq!(is_unset(Some(5)), false);
    assert_eq!(is_set(None), false);                         // != None
    assert_eq!(is_set(Some(5)), true);
}
"#;
    assert_rustc_runs("none_value_literal", &rust, driver);
}

/// PMAT-691: `str.strip(chars)` / `lstrip` / `rstrip` with a char-SET arg —
/// Python strips ANY leading/trailing char that is IN the set (not a substring).
/// Was rejected ("expected exactly 0" args); lowered to `trim_matches` /
/// `trim_start_matches` / `trim_end_matches` with a membership closure. The 0-arg
/// whitespace form is unchanged. Cross-checked vs python3.
#[test]
fn str_strip_charset() {
    let rust = xpile_transpile_to_rust("str_strip_charset.py");
    assert!(
        rust.contains(".trim_matches(|__c: char| __cs.contains(__c))")
            && rust.contains(".trim_start_matches(|__c: char| __cs.contains(__c))")
            && rust.contains(".trim_end_matches(|__c: char| __cs.contains(__c))"),
        "charset strip should lower to trim_matches with a membership closure:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(strip_cs("...hi!!".to_string()), "hi");
    assert_eq!(lstrip_cs("xxhixx".to_string()), "hixx");
    assert_eq!(rstrip_cs("hi...".to_string()), "hi");
    assert_eq!(strip_ws("  hi  ".to_string()), "hi");   // 0-arg whitespace (regression)
}
"#;
    assert_rustc_runs("str_strip_charset", &rust, driver);
}

/// PMAT-692: keyless `max()`/`min()` over a `list[tuple[...]]` — a tuple of `Ord`
/// elements is itself `Ord` (Rust derives lexicographic `Ord`, matching Python's
/// tuple comparison). Was rejected ("produces I64") because the keyless gate only
/// admitted scalar (`I64`/`F64`/`Str`/`Bool`) elements. Cross-checked vs python3.
#[test]
fn max_min_tuple() {
    let rust = xpile_transpile_to_rust("max_min_tuple.py");
    assert!(
        rust.contains("fn maxpair(xs: Vec<(i64, i64)>) -> (i64, i64) {\n    xs.iter().cloned().max().unwrap()"),
        "max() over list[tuple] should fold to .max() with the tuple element type:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(maxpair(vec![(1, 9), (3, 1), (3, 0)]), (3, 1));   // lexicographic
    assert_eq!(minpair(vec![(1, 9), (3, 1), (3, 0)]), (1, 9));
    assert_eq!(
        max_str_tuple(vec![("a".to_string(), 2), ("b".to_string(), 1)]),
        ("b".to_string(), 1)
    );
    assert_eq!(max_int_regression(vec![3, 1, 7, 2]), 7);   // scalar regression
}
"#;
    assert_rustc_runs("max_min_tuple", &rust, driver);
}

/// PMAT-557 (Tranche 2): the f-string **sign flag** `:+` — always show a sign.
/// Python's `+` maps 1:1 to Rust's `{:+}`, composing with precision / width /
/// zero-pad / radix (`{:+.2}`, `{:+05}`, `{:+x}`). A bare `:+` is int-only (a
/// bare float `:+` hits the whole-float repr divergence and is deferred); a
/// bare `:d` (decimal) now also lowers to a plain field. Cross-checked vs
/// python3 (+5/-5/+0, +3.14/-2.50, +0042/-0042, [+ff], 7).
#[test]
fn fstring_sign() {
    let rust = xpile_transpile_to_rust("fstring_sign.py");
    assert!(
        rust.contains("{:+}") && rust.contains("{:+.2}") && rust.contains("{:+05}"),
        "f-string sign flag should emit Rust `+` specs:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sign_int(5), "+5");
    assert_eq!(sign_int(-5), "-5");
    assert_eq!(sign_int(0), "+0");
    assert_eq!(sign_float(3.14159), "+3.14");
    assert_eq!(sign_float(-2.5), "-2.50");
    assert_eq!(sign_pad(42), "+0042");
    assert_eq!(sign_pad(-42), "-0042");
    assert_eq!(sign_hex(255), "[+ff]");
    assert_eq!(plain_d(7), "7");
}
"#;
    assert_rustc_runs("fstring_sign", &rust, driver);
}

/// PMAT-558 (Tranche 2): the f-string **percent** spec `:.N%` / `:%` (float) —
/// Python scales by 100, formats with N decimals (bare `%` → default 6), and
/// appends a literal `%`. Lowered to `Concat(FormatSpec((x)*100.0, ".N"), "%")`
/// (no IR change); int receivers reject (whole-int promotion deferred).
/// Cross-checked vs python3 (12.3%, 100%, 50.000000%, share=7.12%).
#[test]
fn fstring_percent() {
    let rust = xpile_transpile_to_rust("fstring_percent.py");
    assert!(
        rust.contains("100f64") && rust.contains("\"%\""),
        "percent spec should scale by 100 and append a literal %:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(rate1(0.1234), "12.3%");
    assert_eq!(rate0(0.999), "100%");
    assert_eq!(rate_default(0.5), "50.000000%");
    assert_eq!(labeled(0.07125), "share=7.12%");
}
"#;
    assert_rustc_runs("fstring_percent", &rust, driver);
}

/// PMAT-502ec (Tranche 2): empty list literal `[]` takes its element type from
/// the declared annotation / return type — `xs: list[int] = []` and
/// `return []` (any element type, incl. `list[str]` / nested) previously errored
/// ("empty list literal requires a type annotation"), while empty `{}` /
/// `set()` already threaded. The trailing-return equality check tolerates an
/// empty literal (which `infer_type` defaults to `list[int]`). Cross-checked
/// vs python3.
#[test]
fn empty_list_annotated() {
    let rust = xpile_transpile_to_rust("empty_list_annotated.py");
    assert!(
        rust.contains("let mut xs: Vec<i64> = vec![]") || rust.contains("vec![]"),
        "annotated empty list should emit `vec![]` with the declared type:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(build_with_append(5), 5);
    assert_eq!(empty_int_list(), Vec::<i64>::new());
    assert_eq!(empty_str_list(), Vec::<String>::new());
    assert_eq!(early_empty(-1), Vec::<i64>::new());
    assert_eq!(early_empty(9), vec![9i64]);
    assert!(empty_dict_return().is_empty());
    assert_eq!(annotated_str_accumulate(), 2);
}
"#;
    assert_rustc_runs("empty_list_annotated", &rust, driver);
}

/// PMAT-502eb (Tranche 2): `xs += ys` over a list is Python's in-place list
/// **extend**, not numeric addition. The augmented-assign handler emitted
/// `combine_aug`'s `checked_add` on a `Vec` (a silent miscompile — no such
/// method); it now emits `Stmt::ListExtend` (same as `xs.extend(ys)`). A
/// non-`+=` augmented op on a list, or `list += <non-list>`, is rejected
/// cleanly. Cross-checked vs python3.
#[test]
fn list_aug_extend() {
    let rust = xpile_transpile_to_rust("list_aug_extend.py");
    assert!(
        rust.contains("xs.extend(") && !rust.contains("checked_add(vec!"),
        "list `+=` should emit .extend(), never checked_add on a Vec:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(extend_literal(), 4);
    assert_eq!(extend_var(vec![7, 8, 9]), 5);
    assert_eq!(extend_sum(), 33); // 1+2+10+20
    assert_eq!(extend_strings(), 5);
}
"#;
    assert_rustc_runs("list_aug_extend", &rust, driver);
}

/// PMAT-502ea (Tranche 2): nested **augmented** subscript assignment
/// `grid[i][j] += v` (2D/ND list grids), desugared to `grid[i][j] = grid[i][j]
/// <op> v` reusing the nested `IndexAssign` write + nested `Index` read. Also
/// fixes the mutability pre-walk so a literal-initialised receiver mutated only
/// through a subscript aug-assign is correctly emitted `let mut` (a latent gap
/// in single-level PMAT-497 `xs[i] += v` and plain nested PMAT-502dy too).
/// Cross-checked vs python3.
#[test]
fn nested_aug_assign() {
    let rust = xpile_transpile_to_rust("nested_aug_assign.py");
    // PMAT-641: nested assign with a runtime (variable) index now wraps each
    // level (`__aidx{n}`) like Python negative indexing.
    assert!(
        rust.contains("grid[__aidx0 as usize][__aidx1 as usize] =")
            && rust.contains("let mut counts")
            && rust.contains("let mut xs"),
        "nested aug-assign should emit nested IndexAssign + mark literal receivers mut:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(diag_accumulate(3), 5); // grid[2][2]=3 + grid[1][1]=2
    assert_eq!(histogram(), 16); // 7 + 9
    assert_eq!(cube_scale(), 7);
    assert_eq!(single_list_aug(), 25); // 20 + 5
    assert_eq!(single_dict_aug(), 107); // 100 + 7
}
"#;
    assert_rustc_runs("nested_aug_assign", &rust, driver);
}

/// PMAT-502dz (Tranche 2): `for _ in range(n)` and `[… for _ in range(n)]` — a
/// `_` loop/comprehension target can't desugar to `let mut _` (Rust rejects a
/// bare `_` binding). The frontend mints a fresh `__xpile_idx{N}` counter and
/// resolves body reads of `_` to it. Covers unused, body-read, nested (distinct
/// counters), and list/set comprehension forms; cross-checked vs python3.
#[test]
fn for_underscore() {
    let rust = xpile_transpile_to_rust("for_underscore.py");
    assert!(
        rust.contains("__xpile_idx0") && !rust.contains("let mut _:"),
        "for-underscore should mint a fresh counter, never `let mut _`:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(repeat_count(10), 20);
    assert_eq!(sum_indices(5), 10); // 0+1+2+3+4
    assert_eq!(nested_grid_count(3, 4), 12); // distinct nested counters
    assert_eq!(comp_const(3), vec![7i64, 7, 7]);
    assert_eq!(comp_read(5), 30); // 0+1+4+9+16
    assert_eq!(set_comp_size(10), 3); // {0,1,2}
}
"#;
    assert_rustc_runs("for_underscore", &rust, driver);
}

/// PMAT-502dx (Tranche 2): mixed `{**a, "k": v}` dict literals (splats + explicit
/// entries) — chained `once()`/`.iter().map()`, later entry wins.
#[test]
fn dict_merge_mixed() {
    let rust = xpile_transpile_to_rust("dict_merge_mixed.py");
    assert!(
        rust.contains("std::iter::once((String::from(\"x\")")
            && rust.contains(".iter().map(|(__k, __v)|"),
        "mixed dict merge:\n{rust}"
    );
    let driver = r#"
fn main() {
    use std::collections::HashMap;
    let a: HashMap<String, i64> =
        [("x", 1), ("y", 2)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    // mixed splat + explicit; override order cross-checked vs python3.
    assert_eq!(override_after(a.clone(), "x".to_string()), 99);  // explicit after splat wins
    assert_eq!(override_before(a.clone(), "x".to_string()), 1);  // splat after explicit wins
    assert_eq!(size_with_extra(a), 2);                           // {x,y} (x already present)
}
"#;
    assert_rustc_runs("dict_merge_mixed", &rust, driver);
}

/// PMAT-502dw (Tranche 2): `{**d1, **d2, …}` dict merge — chained iterators,
/// later dict wins on a key collision (matching Python).
#[test]
fn dict_merge() {
    let rust = xpile_transpile_to_rust("dict_merge.py");
    assert!(
        rust.contains(".chain((b).iter().map(|(__k, __v)|")
            && rust.contains("collect::<std::collections::HashMap<_, _>>()"),
        "dict merge:\n{rust}"
    );
    let driver = r#"
fn main() {
    use std::collections::HashMap;
    let mk = |pairs: &[(&str, i64)]| -> HashMap<String, i64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    };
    // dict merge; cross-checked vs python3 (later dict wins on collision).
    let a = mk(&[("x", 1), ("y", 2)]);
    let b = mk(&[("y", 9), ("z", 3)]);
    assert_eq!(merged_size(a.clone(), b.clone()), 3);
    assert_eq!(merged_get(a.clone(), b.clone(), "y".to_string()), 9);
    assert_eq!(merged_get(a.clone(), b.clone(), "x".to_string()), 1);
    let c = mk(&[("w", 4)]);
    assert_eq!(merge3(a, b, c), 4);
}
"#;
    assert_rustc_runs("dict_merge", &rust, driver);
}

/// PMAT-593: PEP 584 dict union `a | b` (new dict, b wins) and `a |= b`
/// (in-place update). `a | b` reuses the `{**a, **b}` `DictMerge` lowering;
/// `a |= b` reuses the `a.update(b)` `DictUpdate` lowering. Previously both
/// fell through to a generic BitOr → `HashMap | HashMap` (rustc E0369).
/// Cross-checked vs python3: merged()==3051 (x1+y20+z30 + len3*1000),
/// in_place()==320 (y20 + len3*100).
#[test]
fn dict_union() {
    let rust = xpile_transpile_to_rust("dict_union.py");
    assert!(
        !rust.contains(" | "),
        "must not emit HashMap BitOr:\n{rust}"
    );
    assert!(
        rust.contains(".chain((b).iter().map(|(__k, __v)|"),
        "a | b reuses DictMerge:\n{rust}"
    );
    assert!(
        rust.contains("a.extend((b).iter().map(|(__k, __v)|"),
        "a |= b reuses DictUpdate (extend):\n{rust}"
    );
    let driver = r#"
fn main() {
    use std::collections::HashMap;
    let mk = |pairs: &[(&str, i64)]| -> HashMap<String, i64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    };
    let a = mk(&[("x", 1), ("y", 2)]);
    let b = mk(&[("y", 20), ("z", 30)]);
    assert_eq!(merged(a.clone(), b.clone()), 3051);
    assert_eq!(in_place(a, b), 320);
}
"#;
    assert_rustc_runs("dict_union", &rust, driver);
}

/// PMAT-502dv (Tranche 2): expression-position set/dict comprehensions
/// (`len({x for x in xs})`, `len({k: v for x in xs})`) via Map+SetFromList /
/// Map+DictFromPairs.
#[test]
fn set_dict_comp_expr() {
    let rust = xpile_transpile_to_rust("set_dict_comp_expr.py");
    assert!(
        rust.contains("HashSet<_>>().len()") && rust.contains("HashMap<_, _>>().len()"),
        "set/dict comp expr:\n{rust}"
    );
    let driver = r#"
fn main() {
    // set/dict comprehensions in consumer positions; cross-checked vs python3.
    assert_eq!(n_unique(vec![1, 1, 2, 3, 3]), 3);
    assert_eq!(n_pairs(vec![1, 2, 3]), 3);
    assert_eq!(n_positive_unique(vec![-1, 2, -3, 4]), 2);
}
"#;
    assert_rustc_runs("set_dict_comp_expr", &rust, driver);
}

/// PMAT-502du (Tranche 2): an expression-position list comprehension
/// (`sum([x for x in xs])`) lowers through the `Map`/`Filter` machinery.
#[test]
fn list_comp_expr() {
    let rust = xpile_transpile_to_rust("list_comp_expr.py");
    assert!(
        rust.contains(".map(|__k|") && rust.contains(".iter().fold(0i64, |__a, &__x|"),
        "expr-position list comp → map:\n{rust}"
    );
    let driver = r#"
fn main() {
    // list comprehensions in consumer positions; cross-checked vs python3.
    assert_eq!(sum_squares(vec![1, 2, 3]), 14);
    assert_eq!(max_abs(vec![-1, 5, -3]), 5);
    assert_eq!(count_positive(vec![-1, 2, -3, 4]), 2);
}
"#;
    assert_rustc_runs("list_comp_expr", &rust, driver);
}

/// PMAT-502dt (Tranche 2): a multi-statement nested function lowers to a
/// closure with an `Expr::Block` body (leading stmts + trailing value).
#[test]
fn nested_fn_block() {
    let rust = xpile_transpile_to_rust("nested_fn_block.py");
    assert!(
        rust.contains("let helper = |x: i64| { {") && rust.contains("let sq: i64"),
        "multi-stmt nested fn → block-expr closure:\n{rust}"
    );
    let driver = r#"
fn main() {
    // multi-statement nested functions (block-expr closures); vs python3.
    assert_eq!(sq_plus_one(4), 17);
    assert_eq!(clamped(-5), 0);  // early return from the closure
    assert_eq!(clamped(5), 5);
}
"#;
    assert_rustc_runs("nested_fn_block", &rust, driver);
}

/// PMAT-502ds (Tranche 2): `f(*xs)` splat into a variadic param passes the
/// list directly (`f(fixed…, *xs)` → `f(fixed…, xs)`).
#[test]
fn varargs_splat() {
    let rust = xpile_transpile_to_rust("varargs_splat.py");
    assert!(
        rust.contains("total(xs)") && rust.contains("with_prefix(10i64, xs)"),
        "f(*xs) splat:\n{rust}"
    );
    let driver = r#"
fn main() {
    // *-splat into variadic; cross-checked vs python3.
    assert_eq!(forward(vec![1, 2, 3]), 6);
    assert_eq!(forward_prefixed(vec![1, 2, 3]), 16);
    assert_eq!(forward_empty(vec![]), 0);
}
"#;
    assert_rustc_runs("varargs_splat", &rust, driver);
}

/// PMAT-502dr (Tranche 2): a nested `def inner(...): return <expr>` lowers to
/// a closure (`Stmt::ClosureLet`), reusing the lambda machinery.
#[test]
fn nested_fn() {
    let rust = xpile_transpile_to_rust("nested_fn.py");
    assert!(
        rust.contains("let inner = |y: i64|") && rust.contains("let up = |t: String|"),
        "nested fn → closure:\n{rust}"
    );
    let driver = r#"
fn main() {
    // nested functions as closures; cross-checked vs python3.
    assert_eq!(add_one(5), 6);
    assert_eq!(double_twice(5), 20);
    assert_eq!(shout("hi".to_string()), "HI");
}
"#;
    assert_rustc_runs("nested_fn", &rust, driver);
}

/// PMAT-502dq (Tranche 2): varargs `*args` → a `list[elem]` param; call sites
/// collect trailing positional args into a `vec![...]`.
#[test]
fn varargs() {
    let rust = xpile_transpile_to_rust("varargs.py");
    assert!(
        rust.contains("fn total(args: Vec<i64>)") && rust.contains("total(vec![1i64, 2i64, 3i64])"),
        "varargs def + call:\n{rust}"
    );
    let driver = r#"
fn main() {
    // varargs def + call collection; cross-checked vs python3.
    assert_eq!(call_total(), 6);
    assert_eq!(call_empty(), 0);     // total() → vec![]
    assert_eq!(call_one(), 5);
    assert_eq!(call_prefix(), 103);  // with_prefix(100, 1, 2)
    assert_eq!(call_prefix_only(), 100); // with_prefix(100) → vec![]
}
"#;
    assert_rustc_runs("varargs", &rust, driver);
}

/// PMAT-502dp (Tranche 2): printf `%x`/`%X`/`%o` → no-prefix sign-first radix
/// string (`{:x}` is two's-complement for negatives; Python is sign-first).
#[test]
fn percent_format_radix() {
    let rust = xpile_transpile_to_rust("percent_format_radix.py");
    assert!(
        rust.contains("{}{:x}") && rust.contains("{}{:X}") && rust.contains("{}{:o}"),
        "%x/%X/%o radix:\n{rust}"
    );
    let driver = r#"
fn main() {
    // printf radix conversions; cross-checked vs python3 (incl. negatives).
    assert_eq!(to_hex(255), "ff");
    assert_eq!(to_hex(-255), "-ff"); // sign-first, not two's-complement
    assert_eq!(to_hex_upper(255), "FF");
    assert_eq!(to_oct(8), "10");
    assert_eq!(to_oct(-8), "-10");
    assert_eq!(prefixed_hex(255), "0xff");
}
"#;
    assert_rustc_runs("percent_format_radix", &rust, driver);
}

/// PMAT-502do (Tranche 2): `%s` over bool/float str()-converts the arg first
/// (bool → "True"/"False", float → Python repr) before `{}`.
#[test]
fn percent_format_bool_float() {
    let rust = xpile_transpile_to_rust("percent_format_bool_float.py");
    assert!(
        rust.contains("String::from(\"True\")") && rust.contains("__sf.fract() == 0.0"),
        "%s bool/float str-conversion:\n{rust}"
    );
    let driver = r#"
fn main() {
    // %s over bool/float; cross-checked vs python3.
    assert_eq!(show_bool(true), "True");
    assert_eq!(show_bool(false), "False");
    assert_eq!(show_float(3.0), "3.0");
    assert_eq!(show_float(3.14), "3.14");
    assert_eq!(both(true, 2.5), "[True|2.5]");
    assert_eq!(padded_bool(true), "      True");
}
"#;
    assert_rustc_runs("percent_format_bool_float", &rust, driver);
}

/// PMAT-693: an int with a FLOAT-presentation spec (`.Nf`, `.N%`) is coerced to
/// float by Python (`f"{5:.2f}"` == "5.00", `f"{-3:.1f}"` == "-3.0"). xpile casts
/// the int to f64 and routes it through the float-format path. Int-presentation
/// specs (`05d`/radix/width) stay on the int path untouched.
#[test]
fn int_float_format_spec() {
    let rust = xpile_transpile_to_rust("int_float_format_spec.py");
    assert!(
        rust.contains("format!(\"{:.2}\", __nf)")
            && rust.contains("(n) as f64")
            && rust.contains("format!(\"{:8.3}\", ((n) as f64))")
            && rust.contains("format!(\"{:05}\", n)"),
        "int float format spec:\n{rust}"
    );
    let driver = r#"
fn main() {
    // int coerced to float for `.Nf`/`.N%`; cross-checked vs python3.
    assert_eq!(fixed(5), "5.00");
    assert_eq!(one_dp(-3), "-3.0");
    assert_eq!(pct(1), "100%");
    assert_eq!(width_fixed(5), "   5.000");
    assert_eq!(still_int(42), "00042"); // int spec stays int
}
"#;
    assert_rustc_runs("int_float_format_spec", &rust, driver);
}

/// PMAT-694: f-string `!r` (repr) / `!s` (str) conversions + the `{x=}` debug
/// form (parser desugars `{x=}` → `"x=" + {x!r}`). `!r` of a string adds quotes
/// (`'hi'`) via `ReprStr`; `!s` matches the plain field; `{x!r:>10}` formats the
/// repr string. `!a` (ascii) stays declined.
#[test]
fn fstring_debug_repr() {
    let rust = xpile_transpile_to_rust("fstring_debug_repr.py");
    let driver = r#"
fn main() {
    // f-string repr/str conversions + `{x=}` debug; cross-checked vs python3.
    assert_eq!(dbg_int(5), "x=5");
    assert_eq!(dbg_str("hi".to_string()), "s='hi'"); // !r adds quotes
    assert_eq!(dbg_float(3.0), "f=3.0");
    assert_eq!(dbg_bool(true), "b=True done");
    assert_eq!(expl_r("ab".to_string()), "'ab'");
    assert_eq!(expl_s("ab".to_string()), "ab"); // !s = plain str
    assert_eq!(r_with_spec("ab".to_string()), "[      'ab']"); // repr then align
}
"#;
    assert_rustc_runs("fstring_debug_repr", &rust, driver);
}

/// PMAT-502dn (Tranche 2): printf `%`-format width/precision/flags (`%.2f`,
/// `%5d`, `%-5d`, `%05d`, `%5s`, `%+d`) → translated Rust format specs.
#[test]
fn percent_format_spec() {
    let rust = xpile_transpile_to_rust("percent_format_spec.py");
    assert!(
        rust.contains("{:.2}")
            && rust.contains("{:>5}")
            && rust.contains("{:<5}")
            && rust.contains("{:05}")
            && rust.contains("{:+}"),
        "percent specs:\n{rust}"
    );
    let driver = r#"
fn main() {
    // printf width/precision/flags; cross-checked vs python3.
    assert_eq!(money(3.14159), "$3.14");
    assert_eq!(rjust_num(42), "[   42]");
    assert_eq!(ljust_num(42), "[42   ]");
    assert_eq!(zero_pad(42), "00042");
    assert_eq!(zero_pad(-42), "-0042"); // sign-aware zero-pad
    assert_eq!(rjust_str("ab".to_string()), "[   ab]"); // Python right-aligns %Ns
    assert_eq!(width_prec(3.14159), "    3.14");
    assert_eq!(signed(5), "+5");
}
"#;
    assert_rustc_runs("percent_format_spec", &rust, driver);
}

/// PMAT-502dm (Tranche 2): printf-style `"<tmpl>" % args` → `format!` (`%d`,
/// `%s` over int/str, `%f`, `%%`; single value or tuple).
#[test]
fn percent_format() {
    let rust = xpile_transpile_to_rust("percent_format.py");
    // PMAT-680: `%f` now routes the float through the NaN-guarded FormatSpec
    // (`format!("{:.6}", __nf)` inside an `is_nan()` guard) instead of inlining.
    assert!(
        rust.contains("format!(\"{} items\", n)")
            && rust.contains("format!(\"{:.6}\", __nf)")
            && rust.contains("__nf.is_nan()"),
        "percent format:\n{rust}"
    );
    let driver = r#"
fn main() {
    // printf-style % formatting; cross-checked vs python3.
    assert_eq!(items(5), "5 items");
    assert_eq!(kv("k".to_string(), 3), "k=3");
    assert_eq!(frac(1.5), "1.500000");
    assert_eq!(pct(7), "100% of 7");
    assert_eq!(two("a".to_string(), "b".to_string()), "a and b");
}
"#;
    assert_rustc_runs("percent_format", &rust, driver);
}

/// PMAT-502dl (Tranche 2): `str.splitlines()` — splits on Python's full line
/// boundary set (LF/CR/CRLF/…), no trailing empty for a trailing break.
#[test]
fn str_splitlines() {
    let rust = xpile_transpile_to_rust("str_splitlines.py");
    assert!(
        rust.contains("std::mem::take(&mut __cur)") && rust.contains("__it.peek()"),
        "splitlines char-walk:\n{rust}"
    );
    let driver = r#"
fn main() {
    // splitlines edge cases; cross-checked vs python3.
    assert_eq!(lines("a\nb".to_string()), vec!["a", "b"]);
    assert_eq!(lines("a\nb\n".to_string()), vec!["a", "b"]); // no trailing empty
    assert_eq!(lines("a\r\nb".to_string()), vec!["a", "b"]); // CRLF
    assert_eq!(lines("a\rb".to_string()), vec!["a", "b"]);   // lone CR
    assert_eq!(count_lines("".to_string()), 0);              // empty → []
    assert_eq!(lines("a\n\nb".to_string()), vec!["a", "", "b"]); // blank line kept
}
"#;
    assert_rustc_runs("str_splitlines", &rust, driver);
}

/// PMAT-502dk (Tranche 2): `dict(pairs)` materialises a list of 2-tuples into
/// a HashMap — also covers `dict(zip(..))` / `dict(enumerate(..))`.
#[test]
fn dict_from_pairs() {
    let rust = xpile_transpile_to_rust("dict_from_pairs.py");
    assert!(
        rust.contains(".iter().cloned().collect::<std::collections::HashMap<_, _>>()"),
        "dict(pairs):\n{rust}"
    );
    let driver = r#"
fn main() {
    // dict from pairs / zip / enumerate; cross-checked vs python3.
    assert_eq!(from_pairs(3), 4);
    assert_eq!(from_pairs(1), 2);
    assert_eq!(from_zip(vec![1, 2], vec![10, 20], 2), 20);
    assert_eq!(from_enum(vec![100, 200], 1), 200);
}
"#;
    assert_rustc_runs("dict_from_pairs", &rust, driver);
}

/// PMAT-502dj (Tranche 2): `str.partition(sep)` / `.rpartition(sep)` → the
/// 3-tuple `(before, sep, after)` (first/last split; absent-case differs).
#[test]
fn str_partition() {
    let rust = xpile_transpile_to_rust("str_partition.py");
    assert!(
        rust.contains("__ps.split_once(__psep.as_str())")
            && rust.contains("__ps.rsplit_once(__psep.as_str())"),
        "partition/rpartition:\n{rust}"
    );
    let driver = r#"
fn main() {
    // partition/rpartition; cross-checked vs python3.
    assert_eq!(
        part("a.b.c".to_string(), ".".to_string()),
        ("a".to_string(), ".".to_string(), "b.c".to_string())
    );
    assert_eq!(
        rpart("a.b.c".to_string(), ".".to_string()),
        ("a.b".to_string(), ".".to_string(), "c".to_string())
    );
    assert_eq!(
        part("abc".to_string(), ".".to_string()),
        ("abc".to_string(), String::new(), String::new())
    );
    assert_eq!(
        rpart("abc".to_string(), ".".to_string()),
        (String::new(), String::new(), "abc".to_string())
    );
}
"#;
    assert_rustc_runs("str_partition", &rust, driver);
}

/// PMAT-502di (Tranche 2): `str.isupper()` / `.islower()` / `.isalnum()`
/// classification predicates → Bool.
#[test]
fn str_case_predicates() {
    let rust = xpile_transpile_to_rust("str_case_predicates.py");
    assert!(
        rust.contains("is_uppercase()") && rust.contains("is_alphanumeric()"),
        "case predicates:\n{rust}"
    );
    let driver = r#"
fn main() {
    // str case/alnum predicates; cross-checked vs python3.
    assert_eq!(is_up("ABC".to_string()), true);
    assert_eq!(is_up("Abc".to_string()), false);
    assert_eq!(is_up("A1".to_string()), true);
    assert_eq!(is_up("123".to_string()), false);
    assert_eq!(is_up("".to_string()), false);
    assert_eq!(is_low("abc".to_string()), true);
    assert_eq!(is_low("Abc".to_string()), false);
    assert_eq!(is_an("abc123".to_string()), true);
    assert_eq!(is_an("abc!".to_string()), false);
    assert_eq!(is_an("".to_string()), false);
}
"#;
    assert_rustc_runs("str_case_predicates", &rust, driver);
}

/// PMAT-502dh (Tranche 2): `min(xs, default=d)` / `max(xs, default=d)` return
/// `d` on an empty list instead of panicking (`.unwrap_or(d)`).
#[test]
fn minmax_default() {
    let rust = xpile_transpile_to_rust("minmax_default.py");
    assert!(
        rust.contains(".min().unwrap_or(0i64)")
            && rust.contains(".reduce(|__a, __b| if __b < __a { __b } else { __a }).unwrap_or"),
        "min/max default:\n{rust}"
    );
    let driver = r#"
fn main() {
    // min/max with default; cross-checked vs python3.
    assert_eq!(min_or_zero(vec![3, 1, 2]), 1);
    assert_eq!(min_or_zero(vec![]), 0);
    assert_eq!(max_or_neg1(vec![3, 1, 2]), 3);
    assert_eq!(max_or_neg1(vec![]), -1);
    assert_eq!(fmin_or(vec![2.5, 1.5]), 1.5);
    assert_eq!(fmin_or(vec![]), 9.0);
}
"#;
    assert_rustc_runs("minmax_default", &rust, driver);
}

/// PMAT-502dg (Tranche 2): a generator expression with an `if` filter composes
/// `Filter` → `Map` (`sum(x for x in xs if x > 0)`).
#[test]
fn generator_expr_filter() {
    let rust = xpile_transpile_to_rust("generator_expr_filter.py");
    assert!(
        rust.contains(".filter(|__k|") && rust.contains(".map(|__k|"),
        "filtered genexpr → filter + map:\n{rust}"
    );
    let driver = r#"
fn main() {
    // filtered generator expressions; cross-checked vs python3.
    assert_eq!(sum_positive(vec![-1, 2, -3, 4]), 6);
    assert_eq!(sum_even_squares(6), 20);
    assert_eq!(keep_positive(vec![-1, 2, -3, 4]), vec![2, 4]);
}
"#;
    assert_rustc_runs("generator_expr_filter", &rust, driver);
}

/// PMAT-502df (Tranche 2): generator expressions desugar to `Expr::Map`, so
/// `sum`/`max`/`min`/`list` accept them (`sum(i*i for i in range(n))`).
#[test]
fn generator_expr() {
    let rust = xpile_transpile_to_rust("generator_expr.py");
    assert!(
        rust.contains(".map(|__k|") && rust.contains(".iter().fold(0i64, |__a, &__x|"),
        "genexpr → map + sum:\n{rust}"
    );
    let driver = r#"
fn main() {
    // generator expressions into sum/max/list; cross-checked vs python3.
    assert_eq!(sum_squares(5), 30);
    assert_eq!(sum_abs(vec![-1, 2, -3]), 6);
    assert_eq!(max_abs(vec![-1, 5, -3]), 5);
    assert_eq!(doubled(vec![1, 2, 3]), vec![2, 4, 6]);
}
"#;
    assert_rustc_runs("generator_expr", &rust, driver);
}

/// PMAT-502de (Tranche 2): builtins in a subscript index and under unary `-`
/// lower context-aware (`xs[abs(i)]` / `-abs(n)` were silently miscompiled to
/// an undefined `abs(...)`). Closes the ctx-free-position miscompile class.
#[test]
fn index_unary_builtin() {
    let rust = xpile_transpile_to_rust("index_unary_builtin.py");
    assert!(
        !rust.contains("[abs(") && !rust.contains(" abs(") && !rust.contains("(abs("),
        "abs must lower to a method, not a bare call:\n{rust}"
    );
    // PMAT-579: int `abs` emits `.checked_abs()` (overflow-checked).
    assert!(
        rust.contains("(i).checked_abs()") && rust.contains("(n).checked_abs()"),
        "index/unary builtins:\n{rust}"
    );
    let driver = r#"
fn main() {
    // builtins in subscript index + under unary minus; cross-checked vs python3.
    assert_eq!(at_abs(vec![10, 20, 30], -1), 20);
    assert_eq!(at_clamped(vec![10, 20, 30], -5), 10);
    assert_eq!(at_clamped(vec![10, 20, 30], 2), 30);
    assert_eq!(neg_abs(-5), -5);
    assert_eq!(neg_abs(3), -3);
    assert_eq!(neg_max(2, 9), -9);
}
"#;
    assert_rustc_runs("index_unary_builtin", &rust, driver);
}

/// PMAT-502dd (Tranche 2): builtins in collection literals lower context-aware
/// (`[abs(a), abs(b)]` / `{"k": abs(v)}` / `{abs(a), abs(b)}` were silently
/// miscompiled to an undefined `abs(...)`).
#[test]
fn collection_literal_builtin() {
    let rust = xpile_transpile_to_rust("collection_literal_builtin.py");
    assert!(
        !rust.contains("![abs(") && !rust.contains(" abs(") && !rust.contains("(abs("),
        "abs must lower to a method, not a bare call:\n{rust}"
    );
    // PMAT-579: int `abs` emits `.checked_abs()` (overflow-checked).
    assert!(
        rust.contains("(a).checked_abs()") && rust.contains("(n).checked_abs()"),
        "collection-literal builtins:\n{rust}"
    );
    let driver = r#"
fn main() {
    // builtins inside list/dict/set literals; cross-checked vs python3.
    assert_eq!(list_mags(-3, 4), vec![3, 4]);
    let d = dict_mag(-7);
    assert_eq!(d["m"], 7);
    let s = set_mags(-2, 2);
    assert_eq!(s.len(), 1);
    assert!(s.contains(&2));
}
"#;
    assert_rustc_runs("collection_literal_builtin", &rust, driver);
}

/// PMAT-502dc (Tranche 2): builtins in a comparison operand lower context-aware
/// (`abs(n) > 0` was silently miscompiled to an undefined `abs(...)`).
#[test]
fn compare_builtin() {
    let rust = xpile_transpile_to_rust("compare_builtin.py");
    assert!(
        !rust.contains("{ abs(") && !rust.contains(" abs(") && !rust.contains("(abs("),
        "abs must lower to a method, not a bare call:\n{rust}"
    );
    // PMAT-579: int `abs` emits `.checked_abs()` (overflow-checked).
    assert!(
        rust.contains("(n).checked_abs()") && rust.contains("(a).max(b) <= c"),
        "comparison builtins:\n{rust}"
    );
    let driver = r#"
fn main() {
    // builtins in comparison operands; cross-checked vs python3.
    assert_eq!(is_positive_mag(-3), true);
    assert_eq!(is_positive_mag(0), false);
    assert_eq!(max_le(2, 9, 9), true);
    assert_eq!(max_le(2, 9, 5), false);
    assert_eq!(long_enough("abcd".to_string()), true);
    assert_eq!(long_enough("ab".to_string()), false);
    assert_eq!(in_range(5), true);
    assert_eq!(in_range(0), false);
}
"#;
    assert_rustc_runs("compare_builtin", &rust, driver);
}

/// PMAT-502db (Tranche 2): builtins in a ternary branch lower context-aware
/// (`abs(n) if … else …` was silently miscompiled to an undefined `abs(...)`).
#[test]
fn ternary_builtin() {
    let rust = xpile_transpile_to_rust("ternary_builtin.py");
    // The undefined bare calls must be gone — builtins resolve to methods.
    assert!(
        !rust.contains("{ abs(") && !rust.contains(" abs("),
        "abs must lower to a method, not a bare call:\n{rust}"
    );
    // PMAT-579: int `abs` emits `.checked_abs()` (overflow-checked).
    assert!(
        rust.contains(".checked_abs()")
            && rust.contains("(a).max(b)")
            && rust.contains("checked_pow"),
        "ternary builtins:\n{rust}"
    );
    let driver = r#"
fn main() {
    // builtins inside ternary branches; cross-checked vs python3.
    assert_eq!(absval(-5), 5);
    assert_eq!(absval(3), 3);
    assert_eq!(cap(2, 9), 9);
    assert_eq!(cap(-1, 4), 4);
    assert_eq!(sq_or_zero(3), 9);
    assert_eq!(sq_or_zero(-1), 0);
}
"#;
    assert_rustc_runs("ternary_builtin", &rust, driver);
}

/// PMAT-502da (Tranche 2): `int(s, base)` → `i64::from_str_radix`.
/// PMAT-655: the emit now normalizes (prefix + underscores) before parsing.
#[test]
fn int_from_str_radix() {
    let rust = xpile_transpile_to_rust("int_from_str_radix.py");
    assert!(
        rust.contains("i64::from_str_radix(&__rc, 16)")
            && rust.contains("__rb.strip_prefix(\"0x\")"),
        "int(s, 16) → normalized from_str_radix base 16:\n{rust}"
    );
    assert!(
        rust.contains("i64::from_str_radix(&__rc, 2)"),
        "int(s, 2) → from_str_radix base 2:\n{rust}"
    );
    let driver = r#"
fn main() {
    // int(s, base); cross-checked vs python3 (unprefixed digit strings).
    assert_eq!(from_hex("ff".to_string()), 255);
    assert_eq!(from_hex("FF".to_string()), 255);
    assert_eq!(from_bin("101".to_string()), 5);
    assert_eq!(signed_hex("-1a".to_string()), -26);
}
"#;
    assert_rustc_runs("int_from_str_radix", &rust, driver);
}

/// PMAT-655: Python `int(s, base)` accepts a base-matching radix prefix
/// (`0x`/`0o`/`0b`) and PEP-515 underscore grouping; Rust's `from_str_radix`
/// accepts neither, so `int("0xff", 16)` / `int("1_000", 16)` panicked. The
/// emit now strips the prefix and underscores (after the optional sign) before
/// parsing. Cross-checked vs python3.
#[test]
fn int_radix_prefix_and_underscores() {
    let rust = xpile_transpile_to_rust("int_radix_prefix.py");
    let driver = r#"
fn main() {
    assert_eq!(hex_prefix(), 255);          // "0xff" — was a panic
    assert_eq!(oct_prefix(), 15);           // "0o17"
    assert_eq!(bin_prefix(), 5);            // "0b101"
    assert_eq!(neg_hex_prefix(), -26);      // "-0x1a"
    assert_eq!(underscore_grouping(), 4096); // "1_000" base 16
    assert_eq!(no_prefix(), 255);           // "ff" — regression
}
"#;
    assert_rustc_runs("int_radix_prefix", &rust, driver);
}

/// PMAT-669: `sum(t)` / `min(t)` / `max(t)` over a fixed-arity tuple. These
/// emitted a bare undefined `sum(t)` call (E0425) — the builtin
/// argument-materializer had no tuple branch. A tuple now materializes to a list
/// of its elements (literal: directly; variable: `[t.0, t.1, …]`). Cross-checked
/// vs python3.
#[test]
fn tuple_aggregate() {
    let rust = xpile_transpile_to_rust("tuple_aggregate.py");
    assert!(
        !rust.contains(" sum(t)") && !rust.contains(" min(t)") && !rust.contains(" max(t)"),
        "sum/min/max over a tuple must not emit a bare free call:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sum_t((3, 7, 2)), 12);   // was E0425
    assert_eq!(min_t((3, 7, 2)), 2);
    assert_eq!(max_t((3, 7, 2)), 7);
    assert_eq!(sum_lit(), 12);
    assert_eq!(max_float_t((1.5, 2.5)), 1);
    assert_eq!(sum_list_regression(vec![1, 2, 3, 4]), 10);
}
"#;
    assert_rustc_runs("tuple_aggregate", &rust, driver);
}

/// PMAT-656: `max(d)` / `min(d)` / `sum(d)` over a dict iterate its KEYS in
/// Python. A bare dict arg fell through to an undefined free call (`max(d)` →
/// E0425); it now materializes to the keys list, like `max(d.keys())`.
/// Cross-checked vs python3.
#[test]
fn max_min_dict() {
    let rust = xpile_transpile_to_rust("max_min_dict.py");
    assert!(
        !rust.contains("    max(d)") && !rust.contains("    min(d)"),
        "max/min over a dict must not emit a bare free call:\n{rust}"
    );
    let driver = r#"
fn dm(p: &[(i64, i64)]) -> std::collections::HashMap<i64, i64> { p.iter().cloned().collect() }
fn sm(p: &[(&str, i64)]) -> std::collections::HashMap<String, i64> {
    p.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}
fn main() {
    assert_eq!(max_dict(dm(&[(5, 1), (2, 2), (8, 3)])), 8);  // was E0425
    assert_eq!(min_dict(dm(&[(5, 1), (2, 2), (8, 3)])), 2);
    assert_eq!(sum_dict(dm(&[(5, 1), (2, 2), (8, 3)])), 15); // keys: 5+2+8
    assert_eq!(max_str_keys(sm(&[("apple", 1), ("pear", 2), ("fig", 3)])), "pear");
    assert_eq!(max_dict_keys_regression(dm(&[(5, 1), (2, 2), (8, 3)])), 8);
}
"#;
    assert_rustc_runs("max_min_dict", &rust, driver);
}

/// PMAT-668: `sep.join(d)` over a dict joins its KEYS (iterating a dict yields
/// keys). A bare dict arg emitted `d.join(...)` on a HashMap (E0599); the join
/// arg now materializes to the keys, like `sep.join(d.keys())`. (Single-key
/// dict for determinism — multi-key order is the deferred PMAT-537 limitation.)
/// Cross-checked vs python3.
#[test]
fn join_dict_keys() {
    let rust = xpile_transpile_to_rust("join_dict_keys.py");
    assert!(
        rust.contains(".keys().cloned().collect::<Vec<_>>().join("),
        "join over a dict should iterate its keys:\n{rust}"
    );
    let driver = r#"
fn dm(p: &[(&str, i64)]) -> std::collections::HashMap<String, i64> {
    p.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}
fn main() {
    assert_eq!(join_one_key(dm(&[("solo", 1)])), "solo");  // was E0599
    assert_eq!(join_keys_regression(dm(&[("k", 9)])), "k");
    assert_eq!(join_list_regression(vec!["x".to_string(), "y".to_string()]), "x,y");
}
"#;
    assert_rustc_runs("join_dict_keys", &rust, driver);
}

/// PMAT-502cz (Tranche 2): variadic `min`/`max` (`max(a, b, c)`).
#[test]
fn variadic_minmax() {
    let rust = xpile_transpile_to_rust("variadic_minmax.py");
    assert!(
        rust.contains("(a).max(b).max(c)"),
        "max(a, b, c) → chained .max:\n{rust}"
    );
    assert!(
        rust.contains("(a).min(b).min(c).min(d)"),
        "min(a, b, c, d) → chained .min:\n{rust}"
    );
    let driver = r#"
fn main() {
    // variadic min/max; cross-checked vs python3.
    assert_eq!(m3(3, 7, 5), 7);
    assert_eq!(n4(8, 2, 6, 4), 2);
    assert_eq!(fm(1.5, 9.0, 3.0), 9.0);
}
"#;
    assert_rustc_runs("variadic_minmax", &rust, driver);
}

/// PMAT-502cy (Tranche 2): `pow(a, b)` == `a ** b` (reuses the `**` path).
#[test]
fn pow_builtin() {
    let rust = xpile_transpile_to_rust("pow_builtin.py");
    assert!(
        rust.contains("(a).checked_pow("),
        "pow(a, b) int → checked_pow:\n{rust}"
    );
    assert!(
        rust.contains(".powf(__pe)"),
        "pow(a, b) float → guarded powf (PMAT-734b):\n{rust}"
    );
    let driver = r#"
fn main() {
    // pow(a, b) == a ** b; cross-checked vs python3.
    assert_eq!(ipow(2, 10), 1024);
    assert_eq!(ipow(5, 3), 125);
    assert_eq!(fpow(2.0, 3.0), 8.0);
    assert!((root(2.0) - 1.4142135623730951).abs() < 1e-12);
}
"#;
    assert_rustc_runs("pow_builtin", &rust, driver);
}

/// PMAT-502cx (Tranche 2): `sum(xs, start)` prepends `start` to the sum.
#[test]
fn sum_start() {
    let rust = xpile_transpile_to_rust("sum_start.py");
    // PMAT-595: int sum is a checked fold seeded with `start` (contract-safe).
    assert!(
        rust.contains("(xs).iter().fold((base), |__a, &__x|"),
        "sum(xs, base):\n{rust}"
    );
    // PMAT-584: float sum is now a Neumaier compensated fold seeded with start.
    assert!(
        rust.contains("let mut __ss: f64 = (1.5f64)"),
        "sum(xs, 1.5):\n{rust}"
    );
    let driver = r#"
fn main() {
    // sum(xs, start) == start + sum(xs); cross-checked vs python3.
    assert_eq!(tot(vec![1, 2, 3, 4], 10), 20);
    assert_eq!(tot(vec![], 7), 7);
    assert_eq!(fsum(vec![1.5, 2.5, 3.0]), 8.5);
}
"#;
    assert_rustc_runs("sum_start", &rust, driver);
}

/// PMAT-703: `sum(xs, start)` promotes int+float like Python — `sum(list[int],
/// 0.0)` and `sum(list[float], 0)` are both floats (was rejected: "start type
/// must match the list element type"). xpile maps whichever side is int up to f64.
/// Cross-checked vs python3 (HUNT-V8 item V8-14).
#[test]
fn sum_start_promote() {
    let rust = xpile_transpile_to_rust("sum_start_promote.py");
    let driver = r#"
fn main() {
    assert_eq!(int_floatstart(vec![1, 2, 3]), 6.0);   // int list + float start
    assert_eq!(float_intstart(vec![1.5, 2.5]), 4.0);  // float list + int start
}
"#;
    assert_rustc_runs("sum_start_promote", &rust, driver);
}

/// PMAT-502b (Tranche 2): `str.replace(old, new)` →
/// `.replace(&(old)[..], &(new)[..])`.
#[test]
fn str_replace_method() {
    let rust = xpile_transpile_to_rust("str_replace.py");
    assert!(
        rust.contains(".replace(&("),
        "expected .replace emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(censor(String::from("a bad word")), String::from("a *** word"));
    assert_eq!(swap(String::from("foobar"), String::from("o"), String::from("0")), String::from("f00bar"));
}
"#;
    assert_rustc_runs("str_replace", &rust, driver);
}

/// PMAT-501b (Tranche 2): set comprehension `{e for x in xs}` —
/// materialises to `s = set()` + `for x in xs { s.add(e) }`.
#[test]
fn set_comprehension_materialises() {
    let rust = xpile_transpile_to_rust("set_comp.py");
    let driver = r#"
fn main() {
    assert_eq!(distinct_doubles(vec![1, 2, 2, 3]), 3);
    assert!(has_square(vec![1, 2, 3], 4));
    assert!(!has_square(vec![1, 2, 3], 5));
}
"#;
    assert_rustc_runs("set_comp", &rust, driver);
}

/// PMAT-502g (Tranche 2): set algebra — `a | b` / `a & b` / `a - b` /
/// `a ^ b` over `set[int]` → `(a).union(&(b)).cloned().collect::<…>()` etc.
#[test]
fn set_ops_algebra() {
    let rust = xpile_transpile_to_rust("set_ops.py");
    assert!(
        rust.contains(".union(&(")
            && rust.contains(".intersection(&(")
            && rust.contains(".difference(&(")
            && rust.contains(".symmetric_difference(&("),
        "expected set-algebra method emission, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashSet;
fn main() {
    let a: HashSet<i64> = [1, 2, 3].into_iter().collect();
    let b: HashSet<i64> = [2, 3, 4].into_iter().collect();
    assert_eq!(union_op(a.clone(), b.clone()), [1, 2, 3, 4].into_iter().collect::<HashSet<i64>>());
    assert_eq!(intersect_op(a.clone(), b.clone()), [2, 3].into_iter().collect::<HashSet<i64>>());
    assert_eq!(diff_op(a.clone(), b.clone()), [1].into_iter().collect::<HashSet<i64>>());
    assert_eq!(symdiff_op(a.clone(), b.clone()), [1, 4].into_iter().collect::<HashSet<i64>>());
}
"#;
    assert_rustc_runs("set_ops", &rust, driver);
}

/// PMAT-502i (Tranche 2): empty collection constructors `set()` / `dict()` /
/// `list()` → empty `HashSet::new()` / `HashMap::new()` / `vec![]`, typed by
/// a binding annotation or a subsequent `.add()`/`.append()`.
#[test]
fn empty_constructors() {
    let rust = xpile_transpile_to_rust("empty_constructors.py");
    assert!(
        rust.contains("std::collections::HashSet::new()")
            && rust.contains("std::collections::HashMap::new()")
            && rust.contains("Vec<i64> = vec![]"),
        "expected empty-constructor emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(set_then_add(), 2);
    assert_eq!(set_annotated(), 0);
    assert_eq!(list_then_append(), 2);
    assert_eq!(dict_annotated(), 0);
}
"#;
    assert_rustc_runs("empty_constructors", &rust, driver);
}

/// PMAT-500b (Tranche 2): set `.add()` mutation → `s.insert(x)` (the
/// receiver is marked `mut` by the pre-pass), straight-line + in a loop.
#[test]
fn set_add_mutation() {
    let rust = xpile_transpile_to_rust("set_add.py");
    assert!(
        rust.contains(".insert(") && rust.contains("let mut s"),
        "expected set .insert() on a mut binding, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(has_after_add(5, 5));
    assert!(!has_after_add(5, 9));
    assert!(has_after_add(0, 1));
    assert!(loop_contains(vec![1, 2, 3], 2));
    assert!(!loop_contains(vec![1, 2, 3], 9));
}
"#;
    assert_rustc_runs("set_add", &rust, driver);
}

/// PMAT-501 (Tranche 2): dict comprehension `{k: v for x in xs}` —
/// materialises to `acc = {}` + `for x in xs { acc[k] = v }` (return +
/// assignment position).
#[test]
fn dict_comprehension_materialises() {
    let rust = xpile_transpile_to_rust("dict_comp.py");
    let driver = r#"
fn main() {
    let m = squares(vec![1, 2, 3]);
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&4));
    assert_eq!(m.get(&3), Some(&9));
    let n = lengths(vec![String::from("a"), String::from("bb")]);
    assert_eq!(n.get("a"), Some(&1));
    assert_eq!(n.get("bb"), Some(&2));
}
"#;
    assert_rustc_runs("dict_comp", &rust, driver);
}

/// PMAT-599: a dict comprehension reusing a non-Copy loop var in both the key
/// and the value (`{w: w …}`, `{w: w + "!" …}`, `{k: len(k) …}`) moved it into
/// the tuple before the value could use it (E0382). The key is now cloned
/// (gated on read-count>1 + non-Copy). Cross-checked vs python3:
/// `identity(["a","b","a"])==2`, `with_suffix(["a","b"])=="a!"`,
/// `key_lengths(["abc","de"])==3`.
#[test]
fn dict_comp_key_reuse() {
    let rust = xpile_transpile_to_rust("dict_comp_key_reuse.py");
    // The bare-binder key is cloned so the value keeps a live value.
    assert!(
        rust.contains("((w).clone(), w)"),
        "identity dict comp must clone the key:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(identity(vec!["a".to_string(), "b".to_string(), "a".to_string()]), 2);
    assert_eq!(with_suffix(vec!["a".to_string(), "b".to_string()]), "a!");
    assert_eq!(key_lengths(vec!["abc".to_string(), "de".to_string()]), 3);
}
"#;
    assert_rustc_runs("dict_comp_key_reuse", &rust, driver);
}

/// PMAT-500 (Tranche 2): sets — literal `{a, b, c}` → `HashSet`-init block,
/// `x in s` / `x not in s` → `s.contains(&(x))`.
#[test]
fn sets_literal_and_membership() {
    let rust = xpile_transpile_to_rust("sets.py");
    assert!(
        rust.contains("HashSet") && rust.contains(".contains(&("),
        "expected HashSet literal + membership, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert!(is_vowel(String::from("a")));
    assert!(!is_vowel(String::from("z")));
    assert!(is_small(2));
    assert!(!is_small(9));
    assert!(not_member(5));
    assert!(!not_member(20));
}
"#;
    assert_rustc_runs("sets", &rust, driver);
}

/// PMAT-502 (Tranche 2): general `Stmt::If` with side-effecting branches.
/// The canonical histogram (`if w in freq: freq[w] += 1 else: freq[w] = 1`)
/// — branches mutate a dict, which the if-as-let form rejected.
#[test]
fn general_if_side_effecting_branches_histogram() {
    let rust = xpile_transpile_to_rust("histogram_if.py");
    assert!(
        rust.contains("if ") && rust.contains("} else {"),
        "expected a real if/else statement, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let m = word_freq(vec![String::from("a"), String::from("b"), String::from("a")]);
    assert_eq!(m.get("a"), Some(&2));
    assert_eq!(m.get("b"), Some(&1));
    assert_eq!(m.get("z"), None);
}
"#;
    assert_rustc_runs("histogram_if", &rust, driver);
}

/// PMAT-498b (Tranche 2): `sum(xs)` over a numeric list — an int sum is a
/// checked fold (`.fold(0i64, … checked_add …)`, C-PY-INT-ARITH, PMAT-595); a
/// float sum is the Neumaier compensated fold (PMAT-584).
#[test]
fn sum_builtin_int_and_float() {
    let rust = xpile_transpile_to_rust("sum_builtin.py");
    // PMAT-595: int sum is a checked fold (`.fold(0i64, … checked_add …)`,
    // C-PY-INT-ARITH); PMAT-584: float sum is a Neumaier compensated fold
    // (`__sc += …`), not naive `.iter().sum::<f64>()`.
    assert!(
        rust.contains(".iter().fold(0i64, |__a, &__x|") && rust.contains("__sc += "),
        "expected typed sum emissions, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(total(vec![1, 2, 3, 4]), 10);
    assert!((ftotal(vec![1.5, 2.5]) - 4.0).abs() < 1e-9);
}
"#;
    assert_rustc_runs("sum_builtin", &rust, driver);
}

/// PMAT-595: integer `sum()` / `sum(xs, start)` / `enumerate(xs, start)` must
/// honor the C-PY-INT-ARITH overflow contract (checked add + fail-loud), like
/// every other int-arith path, instead of a bare `.iter().sum::<i64>()` / `+`
/// that silently wraps under `-O`. Cross-checked vs python3 for the normal
/// cases (`sum([1,2,3])==6`, `sum([1,2,3],10)==16`, enumerate start 10 → last
/// index 12); the overflow cases fail loud (caught via `catch_unwind`).
#[test]
fn int_sum_overflow() {
    let rust = xpile_transpile_to_rust("int_sum_overflow.py");
    assert!(
        rust.contains(".iter().fold(0i64, |__a, &__x| __a.checked_add(__x)")
            && rust.contains(".iter().fold((base), |__a, &__x| __a.checked_add(__x)")
            && rust.contains("(__i as i64).checked_add(9223372036854775807i64)"),
        "int sum/enumerate must emit checked adds:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(total(vec![1, 2, 3]), 6);
    assert_eq!(total_from(vec![1, 2, 3], 10), 16);
    assert_eq!(enum_last(vec![5, 5, 5]), 12);
    // i64::MAX + 1 in the running sum must PANIC (contract), not wrap.
    assert!(std::panic::catch_unwind(|| total(vec![i64::MAX, 1])).is_err());
    // enumerate start at i64::MAX overflows on the 2nd index -> panic.
    assert!(std::panic::catch_unwind(|| enum_overflow(vec![0, 0])).is_err());
}
"#;
    assert_rustc_runs("int_sum_overflow", &rust, driver);
}

/// PMAT-498 (Tranche 2): scalar numeric builtins `abs`/`min`/`max` →
/// `(x).abs()` / `(a).min(b)` / `(a).max(b)`. `clamp` via `min(max(...))`.
/// PMAT-579: an `int` `abs` now emits `.checked_abs()` (overflow-panics on
/// `i64::MIN` per C-PY-INT-ARITH) rather than the wrapping `.abs()`.
#[test]
fn num_builtins_abs_min_max() {
    let rust = xpile_transpile_to_rust("num_builtins.py");
    assert!(
        rust.contains(".checked_abs()") && rust.contains(".min(") && rust.contains(".max("),
        "expected abs/min/max emissions, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(clamp(5, 0, 10), 5);
    assert_eq!(clamp(15, 0, 10), 10);
    assert_eq!(clamp(-3, 0, 10), 0);
    assert_eq!(magnitude(-7), 7);
    assert_eq!(magnitude(7), 7);
}
"#;
    assert_rustc_runs("num_builtins", &rust, driver);
}

/// PMAT-497 (Tranche 2): augmented subscript assignment `d[k] += v` /
/// `xs[i] += v` — desugars to `d[k] = d[k] <op> v` reusing DictSet /
/// IndexAssign. Exercised via the canonical histogram idiom + a list bump.
#[test]
fn aug_subscript_assign_dict_and_list() {
    let rust = xpile_transpile_to_rust("aug_subscript.py");
    let driver = r#"
fn main() {
    let m = counts();
    assert_eq!(m.get("a"), Some(&6));
    assert_eq!(bump(vec![1, 2, 3]), vec![11, 12, 13]);
}
"#;
    assert_rustc_runs("aug_subscript", &rust, driver);
}

/// PMAT-604: `grid[i] += [..]` (and nested `cube[i][j] += [..]`) over a nested
/// list is list CONCATENATION; the subscript aug-assign routed `+` through
/// integer `checked_add` on a Vec (E0599). `combine_aug` now emits `ListConcat`
/// for list+list `+`. Cross-checked vs python3 (append_row→310, extend_inner→3).
#[test]
fn subscript_list_concat_aug() {
    let rust = xpile_transpile_to_rust("subscript_list_concat_aug.py");
    // The inner-list `+=` concatenates (chain/collect), never checked_add on Vec.
    assert!(
        rust.contains(".iter().chain((vec![10i64, 20i64]).iter()).cloned().collect::<Vec<_>>()"),
        "grid[i] += [..] must concatenate:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(append_row(vec![vec![1], vec![2]]), 310);
    assert_eq!(extend_inner(vec![vec![vec![0], vec![5]]]), 3);
}
"#;
    assert_rustc_runs("subscript_list_concat_aug", &rust, driver);
}

/// PMAT-495 (sprint): enumerate / zip in for-loops → `Stmt::ForEachPair`,
/// emitting `for (i, x) in xs.iter().cloned().enumerate().map(...)` /
/// `for (a, b) in xs.iter().cloned().zip(ys.iter().cloned())`.
#[test]
fn enumerate_zip_emitted_rust_paired_loops() {
    let rust = xpile_transpile_to_rust("enumerate_zip.py");
    assert!(
        rust.contains(".enumerate().map(") && rust.contains(".zip("),
        "expected enumerate/zip paired-loop emissions, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sum_indexed(vec![10, 20, 30]), 80);
    assert_eq!(dot(vec![1, 2, 3], vec![4, 5, 6]), 32);
}
"#;
    assert_rustc_runs("enumerate_zip", &rust, driver);
}

/// PMAT-450 — v0.2.0 Track 1.A: str-typed parameter passthrough.
/// `def echo(name: str) -> str: return name` transpiles to
/// `pub fn echo(name: String) -> String { name }`, exercises the
/// parameter-position Type::Str path (the foundation PR PMAT-449
/// enabled return-position; this locks in the param case via the
/// new C-XLATE-PY-STR-TO-RUST-STRING contract substrate).
#[test]
fn echo_name_emitted_rust_returns_param() {
    let rust = xpile_transpile_to_rust("echo_name.py");
    let driver = r#"
fn main() {
    assert_eq!(echo(String::from("world")), String::from("world"));
    assert_eq!(echo(String::from("")), String::from(""));
    assert_eq!(echo(String::from("hello, xpile")), String::from("hello, xpile"));
}
"#;
    assert_rustc_runs("echo_name", &rust, driver);
}

/// PMAT-449 — v0.2.0 Track 1.A foundation: the first end-to-end
/// `Type::Str` round-trip. `def greet() -> str: return "hello"`
/// transpiles to Rust `pub fn greet() -> String { String::from("hello") }`
/// and the result compiles + computes the right value at runtime.
#[test]
fn greet_lit_emitted_rust_returns_hello_string() {
    let rust = xpile_transpile_to_rust("greet_lit.py");
    let driver = r#"
fn main() {
    assert_eq!(greet(), String::from("hello"));
}
"#;
    assert_rustc_runs("greet_lit", &rust, driver);
}

/// Binary recursion — fib makes two recursive calls per invocation
/// (factorial makes one). Validates that `f(n-1) + f(n-2)` style
/// patterns work, not just `n * f(n-1)`.
#[test]
fn fib_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("fib.py");
    let driver = r#"
fn main() {
    assert_eq!(fib(0), 0);
    assert_eq!(fib(1), 1);
    assert_eq!(fib(2), 1);
    assert_eq!(fib(3), 2);
    assert_eq!(fib(4), 3);
    assert_eq!(fib(5), 5);
    assert_eq!(fib(10), 55);
    assert_eq!(fib(15), 610);
}
"#;
    assert_rustc_runs("fib", &rust, driver);
}

/// `if / elif / else` chain — recursive lowering produces nested
/// `IfExpr` in meta-HIR, semantically equivalent to Rust `else if`.
#[test]
fn sign_if_elif_else_chain_computes_correct_values() {
    let rust = xpile_transpile_to_rust("sign.py");
    // Post PMAT-002: `-1` lowers to `(1i64).checked_neg().expect(...)`,
    // so assert on the surrounding else-if chain shape without
    // pinning the exact negation form.
    assert!(
        rust.contains("let s: i64 = if (x > 0i64) { 1i64 } else if (x < 0i64) {"),
        "expected flattened else-if chain, got:\n{rust}"
    );
    assert!(
        rust.contains("} else { 0i64 };"),
        "expected terminal 0i64 branch, got:\n{rust}"
    );
    assert!(
        rust.contains("checked_neg"),
        "expected `-1` to lower to checked_neg, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sign(5), 1);
    assert_eq!(sign(0), 0);
    assert_eq!(sign(-5), -1);
    assert_eq!(sign(i64::MAX), 1);
    assert_eq!(sign(i64::MIN), -1);
}
"#;
    assert_rustc_runs("sign", &rust, driver);
}

/// Statement-level `if/else` lifted to a `let = if cond { ... } else { ... }`
/// expression. Validates the v0.1.0 lowering pattern for the most common
/// if-statement shape (both branches: single assignment to same name).
#[test]
fn abs_val_if_else_lifts_to_let_with_if_expr() {
    let rust = xpile_transpile_to_rust("abs_val.py");
    // Post PMAT-002: `-x` lowers to `(x).checked_neg().expect(...)`.
    // Assert on the let-with-if-expr lifting + presence of checked_neg
    // without pinning the exact panic message.
    assert!(
        rust.contains("let y: i64 = if (x < 0i64) {"),
        "expected if-as-let lowering, got:\n{rust}"
    );
    assert!(
        rust.contains("} else { x };"),
        "expected terminal `x` branch, got:\n{rust}"
    );
    assert!(
        rust.contains("checked_neg"),
        "expected `-x` to lower to checked_neg, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(abs_val(5), 5);
    assert_eq!(abs_val(-5), 5);
    assert_eq!(abs_val(0), 0);
    assert_eq!(abs_val(-100), 100);
}
"#;
    assert_rustc_runs("abs_val", &rust, driver);
}

/// Tail-recursive Euclidean GCD. Exercises:
///   - Multiple-arg recursion (gcd(b, a % b))
///   - Python `%` lowering to Rust `rem_euclid` (load-bearing: plain
///     `%` would diverge from Python on negative operands)
#[test]
fn gcd_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("gcd.py");
    let driver = r#"
fn main() {
    assert_eq!(gcd(12, 18), 6);
    assert_eq!(gcd(100, 75), 25);
    assert_eq!(gcd(17, 13), 1);    // coprime
    assert_eq!(gcd(48, 36), 12);
    assert_eq!(gcd(0, 5), 5);
    assert_eq!(gcd(5, 0), 5);
}
"#;
    assert_rustc_runs("gcd", &rust, driver);
}

/// PMAT-013: validates implicit BigInt promotion. The user annotates
/// only the return type as `BigInt`; the frontend auto-promotes every
/// `int`-typed param. `factorial(n: int) -> BigInt` emits the full
/// BigInt-mode body, including `.clone()` on every Ident reference
/// (since BigInt isn't `Copy`).
///
/// This is the canonical case the C-PY-INT-ARITH slow path was always
/// pointing at via panic messages.
#[test]
fn bigint_implicit_promotion_factorial_emits_bigint_mode() {
    let rust = xpile_transpile_to_rust("bigint_factorial.py");
    // Param `n` was annotated as `int` but the return is BigInt, so
    // implicit promotion lifts `n` to BigInt automatically.
    assert!(
        rust.contains("pub fn factorial(n: xpile_bigint::BigInt) -> xpile_bigint::BigInt"),
        "expected param n implicitly promoted to BigInt, got:\n{rust}"
    );
    // Body uses `n.clone()` because BigInt isn't `Copy` and `n` is
    // referenced in cond + multiplication + subtraction.
    assert!(
        rust.contains("n.clone()"),
        "expected .clone() on BigInt Ident references, got:\n{rust}"
    );
    // Plain infix `*` and `-` — no checked_mul / checked_sub.
    assert!(
        !rust.contains("checked_mul") && !rust.contains("checked_sub"),
        "BigInt mode must not emit checked_* arithmetic:\n{rust}"
    );

    let shim = r#"
mod xpile_bigint {
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BigInt(pub i64);
    impl From<i64> for BigInt {
        fn from(v: i64) -> Self { BigInt(v) }
    }
    impl std::ops::Add for BigInt {
        type Output = BigInt;
        fn add(self, o: BigInt) -> BigInt { BigInt(self.0 + o.0) }
    }
    impl std::ops::Sub for BigInt {
        type Output = BigInt;
        fn sub(self, o: BigInt) -> BigInt { BigInt(self.0 - o.0) }
    }
    impl std::ops::Mul for BigInt {
        type Output = BigInt;
        fn mul(self, o: BigInt) -> BigInt { BigInt(self.0 * o.0) }
    }
}
"#;
    let driver = r#"
fn main() {
    use xpile_bigint::BigInt;
    assert_eq!(factorial(BigInt::from(0)), BigInt::from(1));
    assert_eq!(factorial(BigInt::from(1)), BigInt::from(1));
    assert_eq!(factorial(BigInt::from(5)), BigInt::from(120));
    assert_eq!(factorial(BigInt::from(10)), BigInt::from(3628800));
}
"#;
    assert_rustc_runs("bigint_factorial", &format!("{shim}\n{rust}"), driver);
}

/// PMAT-012: validates the BigInt slow path. `big_sum(a: BigInt, b: BigInt)
/// -> BigInt: return a + b` emits with `xpile_bigint::BigInt` typing and
/// plain infix `+` (no `.checked_add().expect()` — BigInt never overflows).
///
/// Runtime verification uses a minimal inline `xpile_bigint` shim with
/// the same surface as the real crate (which depends on `num-bigint`).
/// The shim is just `i64` underneath, so for small inputs it agrees;
/// real BigInt behavior is covered by the bigint crate's own tests.
#[test]
fn bigint_function_emits_bigint_type_and_infix() {
    let rust = xpile_transpile_to_rust("big_sum.py");
    assert!(
        rust.contains("// xpile-contract: C-PY-INT-ARITH"),
        "expected contract citation (slow path is still under the contract):\n{rust}"
    );
    assert!(
        rust.contains("pub fn big_sum(a: xpile_bigint::BigInt, b: xpile_bigint::BigInt) -> xpile_bigint::BigInt"),
        "expected BigInt sig, got:\n{rust}"
    );
    // PMAT-013 added `.clone()` on BigInt Idents (since BigInt isn't
    // `Copy`); the infix shape is preserved otherwise.
    assert!(
        rust.contains("(a.clone() + b.clone())"),
        "expected infix + with clones (BigInt mode), got:\n{rust}"
    );
    assert!(
        !rust.contains("checked_add"),
        "BigInt mode must not emit checked_add:\n{rust}"
    );

    // Inline shim so `rustc` doesn't need a num-bigint dependency.
    let shim = r#"
mod xpile_bigint {
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BigInt(pub i64);
    impl From<i64> for BigInt {
        fn from(v: i64) -> Self { BigInt(v) }
    }
    impl std::ops::Add for BigInt {
        type Output = BigInt;
        fn add(self, other: BigInt) -> BigInt { BigInt(self.0 + other.0) }
    }
}
"#;
    let driver = r#"
fn main() {
    use xpile_bigint::BigInt;
    assert_eq!(big_sum(BigInt::from(2), BigInt::from(3)), BigInt::from(5));
    assert_eq!(big_sum(BigInt::from(100), BigInt::from(50)), BigInt::from(150));
    assert_eq!(big_sum(BigInt::from(-7), BigInt::from(7)), BigInt::from(0));
}
"#;
    assert_rustc_runs("big_sum", &format!("{shim}\n{rust}"), driver);
}

#[test]
fn bigint_lean_uses_int_directly() {
    let py = fixture("big_sum.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Lean's Int is unbounded — Type::BigInt maps to Int, identical
    // to Type::I64. The same source produces the same Lean output
    // regardless of which type annotation the Python source used.
    assert!(
        stdout.contains("def big_sum (a : Int) (b : Int) : Int :="),
        "expected Lean Int sig, got:\n{stdout}"
    );
    assert!(
        stdout.contains("(a + b)"),
        "expected plain infix +, got:\n{stdout}"
    );
}

// PMAT-025 (PMAT-012-FOLLOWUP): Ruchy backend now supports BigInt
// mode end-to-end, mirroring the Rust pattern from PMAT-012/013. The
// emission shape is identical to Rust except for the `fun` vs
// `pub fn` signature keyword. Replaces the previous bait test
// `bigint_ruchy_errors_with_pmat_012_message` which asserted Ruchy
// would error — it now succeeds.
#[test]
fn bigint_ruchy_emits_bigint_type_and_clones() {
    let py = fixture("big_sum.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(
        out.status.success(),
        "Ruchy + BigInt should now succeed (PMAT-025)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("// xpile-contract: C-PY-INT-ARITH"),
        "expected citation, got:\n{stdout}"
    );
    assert!(
        stdout.contains(
            "fun big_sum(a: xpile_bigint::BigInt, b: xpile_bigint::BigInt) -> xpile_bigint::BigInt"
        ),
        "expected Ruchy `fun` sig with BigInt params + return, got:\n{stdout}"
    );
    assert!(
        stdout.contains("(a.clone() + b.clone())"),
        "expected infix + with .clone() (BigInt mode), got:\n{stdout}"
    );
    assert!(
        !stdout.contains("checked_add"),
        "BigInt mode must NOT emit checked_add:\n{stdout}"
    );
}

// PMAT-026 / PMAT-013-FOLLOWUP: BigInt bitwise + shift + power.
// `bigint_bits.py` exercises `& | ^ << >>` in BigInt mode via the
// implicit-promotion path. Emission uses `xpile_bigint::shl/shr`
// helpers (num-bigint's shift takes usize rhs, so we route through
// helpers that handle the BigInt→usize conversion) and infix `& | ^`
// (num-bigint impls these directly on BigInt operands).
#[test]
fn bigint_bitwise_shifts_emit_helpers_and_infix() {
    let py = fixture("bigint_bits.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("xpile_bigint::shl("),
        "expected BigInt shl helper, got:\n{stdout}"
    );
    assert!(
        stdout.contains("xpile_bigint::shr("),
        "expected BigInt shr helper, got:\n{stdout}"
    );
    // Infix bitwise (num-bigint impls these on BigInt operands).
    for op in &[" & ", " | ", " ^ "] {
        assert!(
            stdout.contains(op),
            "expected infix `{op}` in BigInt-mode emission, got:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("checked_shl") && !stdout.contains("checked_shr"),
        "BigInt mode must not emit checked_shl/checked_shr:\n{stdout}"
    );
}

#[test]
fn bigint_bitwise_emits_via_ruchy_target_too() {
    let py = fixture("bigint_bits.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(
        out.status.success(),
        "Ruchy + BigInt bitwise should succeed (PMAT-026); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("xpile_bigint::shl(") && stdout.contains("xpile_bigint::shr("),
        "Ruchy emission must mirror Rust's BigInt shift helpers, got:\n{stdout}"
    );
}

#[test]
fn bigint_implicit_promotion_ruchy_emits_full_factorial() {
    // Mirror of the PMAT-013 implicit-promotion test but on the Ruchy
    // target. Same fixture, same param-promotion + .clone() emission,
    // different signature keyword (`fun` vs `pub fn`).
    let py = fixture("bigint_factorial.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(
        out.status.success(),
        "Ruchy + implicit BigInt promotion should succeed (PMAT-025); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("fun factorial(n: xpile_bigint::BigInt) -> xpile_bigint::BigInt"),
        "expected n implicitly promoted to BigInt in Ruchy sig, got:\n{stdout}"
    );
    assert!(
        stdout.contains("n.clone()"),
        "expected .clone() on BigInt Ident references, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("checked_mul") && !stdout.contains("checked_sub"),
        "BigInt mode must not emit checked_* arithmetic:\n{stdout}"
    );
}

/// PMAT-011: validates contract citations are emitted next to functions
/// whose body uses ops the contract governs, and *not* emitted next to
/// pure comparison / logical functions.
///
/// Each host language uses the form named in
/// `sub/contract-frontend-trait.md`'s citation grid:
///   * Rust + Ruchy: `// xpile-contract: C-PY-INT-ARITH`
///   * Lean: `@[xpile_contract "C-PY-INT-ARITH"]`
// PMAT-015 / XPILE-FALSIFY-001: validates the `xpile audit` CLI
// surface. Runs the audit against the fixture corpus, parses both
// text and JSON outputs, asserts the F1 metric is computed and
// reported. The exact percentage is not pinned (it moves as fixtures
// are added) — just that the gate produces structured output with the
// expected fields.
#[test]
fn audit_command_reports_f1_on_fixture_corpus() {
    let out = run_xpile(&["audit", "crates/xpile/tests/fixtures"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "audit should succeed:\nstdout={stdout}\nstderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("F1 (Layer-1 contract citation coverage)"),
        "expected F1 header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("coverage (F1)"),
        "expected coverage line, got:\n{stdout}"
    );
    // The fixture corpus contains both arithmetic functions (with
    // citation) and comparison-only functions (without). F1 must
    // therefore be > 0% (at least factorial / add are cited) and
    // < 100% (cmp / pick are correctly uncited).
    assert!(
        stdout.contains("[OK]") || stdout.contains("[WARN]") || stdout.contains("[FAIL]"),
        "expected status tag, got:\n{stdout}"
    );
}

#[test]
fn audit_command_json_output_has_required_fields() {
    let out = run_xpile(&["audit", "crates/xpile/tests/fixtures", "--json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    // Hand-rolled JSON — verify each required field appears in order.
    // XPILE-FALSIFY-002 added `functions_requiring_citation` +
    // `over_citations` to the schema.
    for field in &[
        "\"target\":",
        "\"files_scanned\":",
        "\"functions_emitted\":",
        "\"functions_requiring_citation\":",
        "\"functions_with_citation\":",
        "\"over_citations\":",
        "\"f1_pct\":",
        "\"f1_status\":",
        "\"errors\":",
    ] {
        assert!(
            stdout.contains(field),
            "missing JSON field `{field}` in: {stdout}"
        );
    }
}

// XPILE-FALSIFY-002: F1 should now report 100% on the fixture corpus
// because comparison-only functions are correctly excluded from the
// denominator. The metric is the load-bearing claim of the audit; we
// pin it to the exact expected value (with rounding tolerance) so a
// regression that misses a citation OR mis-classifies a function shows
// up as a numeric drop, not a vibes-based "looks worse".
#[test]
fn audit_command_f1_is_100_percent_on_current_fixture_corpus_rust() {
    let out = run_xpile(&["audit", "crates/xpile/tests/fixtures", "--json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        stdout.contains("\"f1_pct\":100.0"),
        "expected F1 = 100.0% on current corpus (PMAT-023 applicable-contracts denominator), got: {stdout}"
    );
    assert!(
        stdout.contains("\"f1_status\":\"OK\""),
        "expected F1 status OK, got: {stdout}"
    );
    assert!(
        stdout.contains("\"over_citations\":0"),
        "expected zero over-citations (codegen would be wrongly citing a comparison-only fn), got: {stdout}"
    );
}

// PMAT-027 / PMAT-009-FOLLOWUP: Lean target now handles
// `Stmt::Assert` via recursive `if cond then <rest> else panic!`
// emission. The asserted.py fixture used to fail Lean lowering;
// now it produces valid Lean syntax.
#[test]
fn assert_lean_emits_if_then_panic_chain() {
    let py = fixture("asserted.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(
        out.status.success(),
        "Lean + assert should succeed (PMAT-027); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Two asserts → two nested `if (cond) then` openers + two
    // closing `else panic!` tails, with the original body inside.
    assert!(
        stdout.contains("if ((b != (0: Int))) then"),
        "expected outer `if b != 0 then`, got:\n{stdout}"
    );
    assert!(
        stdout.contains("if ((a >= (0: Int))) then"),
        "expected nested `if a >= 0 then`, got:\n{stdout}"
    );
    let panic_count = stdout.matches("else panic!").count();
    assert_eq!(
        panic_count, 2,
        "expected 2 `else panic!` tails (one per assert), got {panic_count} in:\n{stdout}"
    );
    assert!(
        stdout.contains("(Int.fdiv a b)"),
        "expected the original trailing return to remain in the innermost then-branch, got:\n{stdout}"
    );
}

#[test]
fn audit_command_supports_lean_target() {
    // XPILE-FALSIFY-002 added Lean target support. Lean's citation
    // form is `@[xpile_contract "..."]` (structured attribute parsed
    // by Lean's elaborator); the audit recognises it alongside
    // Rust/Ruchy's `// xpile-contract:` comment form.
    let out = run_xpile(&[
        "audit",
        "crates/xpile/tests/fixtures",
        "--target",
        "lean",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "Lean target now supported (XPILE-FALSIFY-002); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        stdout.contains("\"target\":\"Lean\""),
        "expected Lean target in JSON, got: {stdout}"
    );
    assert!(
        stdout.contains("\"f1_status\":\"OK\""),
        "expected F1 OK for Lean (all arithmetic functions carry @[xpile_contract \"...\"]), got: {stdout}"
    );
}

#[test]
fn arithmetic_function_emits_contract_citation_rust() {
    let rust = xpile_transpile_to_rust("add.py");
    assert!(
        rust.contains("// xpile-contract: C-PY-INT-ARITH\npub fn add"),
        "expected citation directly before fn signature, got:\n{rust}"
    );
}

#[test]
fn arithmetic_function_emits_contract_citation_ruchy() {
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "ruchy"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("// xpile-contract: C-PY-INT-ARITH\nfun add"),
        "expected citation directly before fun signature, got:\n{stdout}"
    );
}

#[test]
fn arithmetic_function_emits_contract_citation_lean() {
    let py = fixture("add.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("@[xpile_contract \"C-PY-INT-ARITH\"]\ndef add"),
        "expected Lean structured attribute before def, got:\n{stdout}"
    );
}

#[test]
fn comparison_only_function_omits_contract_citation() {
    // `le(a, b) -> bool` uses only BinOp::LtEq — a comparison, not under
    // the C-PY-INT-ARITH contract — so codegen should NOT emit the
    // citation. This is the negative test that proves
    // `applicable_contracts()` is data-driven, not unconditional.
    let rust = xpile_transpile_to_rust("cmp.py");
    assert!(
        !rust.contains("xpile-contract:"),
        "comparison-only fn should have no citation, got:\n{rust}"
    );
}

#[test]
fn while_function_citation_appears_on_helper_too_lean() {
    // The partial-def helper executes the same arithmetic constructs
    // as its outer function, so it must carry the same citation.
    let py = fixture("sum_to.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let citation = "@[xpile_contract \"C-PY-INT-ARITH\"]";
    let count = stdout.matches(citation).count();
    assert_eq!(
        count, 2,
        "expected exactly 2 citations (helper + outer fn), got {count} in:\n{stdout}"
    );
}

/// PMAT-009: validates `assert cond` lowers to `assert!(cond)` in Rust.
/// `safe_div` asserts both args are valid before performing floor-div.
#[test]
fn assert_emitted_rust_panics_on_violation() {
    let rust = xpile_transpile_to_rust("asserted.py");
    assert!(
        rust.contains("assert!((b != 0i64));"),
        "expected b != 0 assert, got:\n{rust}"
    );
    assert!(
        rust.contains("assert!((a >= 0i64));"),
        "expected a >= 0 assert, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    // happy path: both asserts satisfied
    assert_eq!(safe_div(10, 2), 5);
    assert_eq!(safe_div(0, 7), 0);
    assert_eq!(safe_div(100, 25), 4);
}
"#;
    assert_rustc_runs("asserted", &rust, driver);
}

/// PMAT-008 + PMAT-036 + PMAT-634: validates negative-step `range(...)` under
/// BigInt mode. `factorial_iter(n)` counts down via `for i in
/// range(n, 0, -1): acc *= i`. The lowering must flip the cond from
/// `<` to `>` AND emit the loop with a BigInt-typed counter (PMAT-036 — the
/// counter's binding type follows the enclosing function's return type).
/// PMAT-634: a fresh synthetic counter `__forc{N}` drives the loop, not the
/// user variable. Tail uses BigInt arithmetic.
#[test]
fn for_range_negative_step_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("countdown.py");
    assert!(
        rust.contains("let mut __forc0: xpile_bigint::BigInt = n"),
        "expected BigInt counter init (PMAT-036/634), got:\n{rust}"
    );
    assert!(
        rust.contains("while (__forc0.clone() > xpile_bigint::BigInt::from(0i64))"),
        "expected `__forc0 > 0` cond comparing BigInt operands, got:\n{rust}"
    );
    assert!(
        rust.contains("__forc0 = (__forc0.clone() + xpile_bigint::BigInt::from(-1i64))"),
        "expected BigInt-mode negative-step tail, got:\n{rust}"
    );
    // Inline xpile_bigint shim so the rustc-compiled driver doesn't
    // need to link the real crate. Same pattern as the BigInt
    // factorial test above. Adds `PartialOrd` (loop cond `> 0`).
    let shim = r#"
mod xpile_bigint {
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BigInt(pub i64);
    impl From<i64> for BigInt {
        fn from(v: i64) -> Self { BigInt(v) }
    }
    impl std::ops::Add for BigInt {
        type Output = BigInt;
        fn add(self, o: BigInt) -> BigInt { BigInt(self.0 + o.0) }
    }
    impl std::ops::Mul for BigInt {
        type Output = BigInt;
        fn mul(self, o: BigInt) -> BigInt { BigInt(self.0 * o.0) }
    }
}
"#;
    let driver = r#"
fn main() {
    use xpile_bigint::BigInt;
    assert_eq!(factorial_iter(BigInt::from(0i64)), BigInt::from(1i64));  // empty product
    assert_eq!(factorial_iter(BigInt::from(1i64)), BigInt::from(1i64));  // 1
    assert_eq!(factorial_iter(BigInt::from(5i64)), BigInt::from(120i64));
    assert_eq!(factorial_iter(BigInt::from(10i64)), BigInt::from(3628800i64));
}
"#;
    assert_rustc_runs("countdown", &format!("{shim}\n{rust}"), driver);
}

/// PMAT-634: **correctness** — for-range loop-variable semantics. The desugar
/// uses a fresh synthetic counter `__forc{N}` (not the user variable), which
/// fixes three silent miscompiles: (1) nested same-name loops (`for i: for i:`)
/// no longer clobber the outer counter, (2) the post-loop value is the last
/// iteration value (Python leak) not `stop`, (3) an empty range leaves a
/// pre-existing variable untouched. Cross-checked vs python3.
#[test]
fn for_loop_var_semantics() {
    let rust = xpile_transpile_to_rust("for_loop_var_semantics.py");
    // The counter is synthetic, not the user variable.
    assert!(
        rust.contains("__forc0"),
        "expected synthetic loop counter:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(nested_same_var(), 6);  // was 2 (inner clobbered outer)
    assert_eq!(postloop_leak(), 2);    // was 3 (counter overran to stop)
    assert_eq!(empty_range_keep(), 99); // was 0 (empty range clobbered)
    assert_eq!(nested_diff(), 9);
}
"#;
    assert_rustc_runs("for_loop_var_semantics", &rust, driver);
}

/// PMAT-007 / PMAT-634: validates `for i in range(...)` desugaring. The three
/// `range` shapes (stop, start+stop, start+stop+step) all lower to a fresh
/// synthetic counter `let mut __forc{N}` + `while __forc{N} <cmp> <stop>` +
/// `__forc{N} = __forc{N} + <step>` tail, with the user variable assigned from
/// the counter inside the body (PMAT-634 — the counter is no longer the user
/// variable itself).
#[test]
fn for_range_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("for_sum.py");
    assert!(
        rust.contains("let mut __forc0: i64 = 0i64"),
        "expected counter init at 0:\n{rust}"
    );
    assert!(
        rust.contains("let mut __forc0: i64 = a"),
        "expected counter init at a:\n{rust}"
    );
    assert!(
        rust.contains("__forc0 = (__forc0).checked_add(2i64)"),
        "expected step=2 tail:\n{rust}"
    );
    assert!(
        rust.contains("while (__forc0 < n)"),
        "expected while-cond:\n{rust}"
    );
    // PMAT-634: the user variable is assigned from the counter inside the body.
    assert!(
        rust.contains("i = __forc0"),
        "expected user-var assigned from counter:\n{rust}"
    );
    let driver = r#"
fn main() {
    // for_sum(n): 0 + 1 + ... + (n-1)
    assert_eq!(for_sum(0), 0);
    assert_eq!(for_sum(1), 0);  // i=0 only
    assert_eq!(for_sum(5), 10); // 0+1+2+3+4
    assert_eq!(for_sum(10), 45);

    // range_with_start(a, b): a + (a+1) + ... + (b-1)
    assert_eq!(range_with_start(3, 7), 18); // 3+4+5+6
    assert_eq!(range_with_start(0, 4), 6);  // 0+1+2+3
    assert_eq!(range_with_start(5, 5), 0);  // empty

    // range_with_step(stop): 0 + 2 + 4 + ... < stop
    assert_eq!(range_with_step(10), 20); // 0+2+4+6+8
    assert_eq!(range_with_step(0), 0);
    assert_eq!(range_with_step(1), 0);
    assert_eq!(range_with_step(11), 30); // 0+2+4+6+8+10
}
"#;
    assert_rustc_runs("for_sum", &rust, driver);
}

/// PMAT-010: validates that while-having functions transpile to Lean
/// as `partial def` helpers with threaded state. The transformation:
///   * loop_state names (assigned in body) → helper params
///   * free vars (referenced but not assigned) → also helper params
///   * recursive call with updated values
///   * else-branch returns the variable named by the function's
///     trailing return
#[test]
fn transpile_sum_to_py_to_lean_uses_partial_def_helper() {
    let py = fixture("sum_to.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("partial def sum_to_loop_0 (total : Int) (i : Int) (n : Int) : Int :="),
        "expected helper signature, got:\n{stdout}"
    );
    assert!(
        stdout.contains("if (i <= n) then"),
        "expected lifted cond, got:\n{stdout}"
    );
    assert!(
        stdout.contains("sum_to_loop_0 total i n"),
        "expected recursive call, got:\n{stdout}"
    );
    // Outer function body: pre-stmts then helper call.
    assert!(
        stdout.contains("def sum_to (n : Int) : Int :="),
        "expected sum_to signature, got:\n{stdout}"
    );
    assert!(
        stdout.contains("let total := (0: Int)") && stdout.contains("let i := (1: Int)"),
        "expected pre-stmt lets, got:\n{stdout}"
    );
}

#[test]
fn transpile_countdown_py_to_lean_with_negative_step() {
    let py = fixture("countdown.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // PMAT-634: a fresh synthetic counter `__forc0` drives the loop, so the
    // helper threads `i`, `acc`, and `__forc0`; the cond and increment are on
    // `__forc0`, and `i` is rebound from it each iteration.
    assert!(
        stdout.contains(
            "partial def factorial_iter_loop_0 (i : Int) (acc : Int) (__forc0 : Int) : Int :="
        ),
        "expected helper signature (counter threaded), got:\n{stdout}"
    );
    assert!(
        stdout.contains("if (__forc0 > (0: Int)) then"),
        "expected negative-step cond on counter, got:\n{stdout}"
    );
    assert!(
        stdout.contains("let __forc0 := (__forc0 + (-1: Int))"),
        "expected negative-step counter update, got:\n{stdout}"
    );
}

/// PMAT-006: validates while-loops + mutable rebinding. `sum_to(n)` =
/// 1 + 2 + ... + n via an iterative accumulator. Triggers Stmt::While,
/// Stmt::Assign for the rebindings, and Stmt::Let { mutable: true } for
/// the initial bindings (because the pre-walk sees them reassigned
/// inside the loop body).
#[test]
fn sum_to_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("sum_to.py");
    assert!(
        rust.contains("let mut total: i64"),
        "expected `let mut total` (mutable initial binding), got:\n{rust}"
    );
    assert!(
        rust.contains("let mut i: i64"),
        "expected `let mut i`, got:\n{rust}"
    );
    assert!(
        rust.contains("while (i <= n)"),
        "expected `while` loop emission, got:\n{rust}"
    );
    assert!(
        rust.contains("i = (i).checked_add"),
        "expected `i = i + 1` lowered to reassignment with checked_add, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(sum_to(0), 0);
    assert_eq!(sum_to(1), 1);
    assert_eq!(sum_to(10), 55);
    assert_eq!(sum_to(100), 5050);
    assert_eq!(sum_to(1000), 500500);
}
"#;
    assert_rustc_runs("sum_to", &rust, driver);
}

/// PMAT-005: validates multi-assignment if-branches lift to one `Let`
/// per assigned name, each sharing the same condition.
/// `range_size(a, b)` = |a - b| via min-max sorting in both branches.
#[test]
fn multi_branch_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("multi_branch.py");
    // Two `let` lifts, one for each assigned name.
    assert!(
        rust.contains("let lo: i64 = if "),
        "expected `let lo` lift, got:\n{rust}"
    );
    assert!(
        rust.contains("let hi: i64 = if "),
        "expected `let hi` lift, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(range_size(3, 7), 4);   // hi=7, lo=3
    assert_eq!(range_size(7, 3), 4);   // hi=7, lo=3
    assert_eq!(range_size(5, 5), 0);
    assert_eq!(range_size(-10, 10), 20);
    assert_eq!(range_size(10, -10), 20);
}
"#;
    assert_rustc_runs("multi_branch", &rust, driver);
}

/// PMAT-004: validates power op (`**`) emits checked_pow with u32 cast
/// of the exponent and matches CPython semantics on non-negative ints.
/// `square_plus(a, b) == a**b + a**1 == a**b + a`.
#[test]
fn pow_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("pow.py");
    assert!(
        rust.contains("checked_pow"),
        "expected checked_pow, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(square_plus(2, 3), 10);  // 8 + 2
    assert_eq!(square_plus(3, 2), 12);  // 9 + 3
    assert_eq!(square_plus(5, 0), 6);   // 1 + 5
    assert_eq!(square_plus(1, 10), 2);  // 1 + 1
    assert_eq!(square_plus(10, 4), 10010); // 10000 + 10
}
"#;
    assert_rustc_runs("pow", &rust, driver);
}

/// PMAT-003: validates the bitwise BinOps (`&`, `|`, `^`, `<<`, `>>`)
/// produce semantically correct Rust output that matches CPython on
/// `bits(a, b) = ((a & b) | (a ^ b)) << 2 >> 1`.
///
/// For `bits(5, 3)`:
///   a & b = 1, a ^ b = 6, 1 | 6 = 7, 7 << 2 = 28, 28 >> 1 = 14
/// For `bits(0, 0)`: all-zero short-circuit -> 0
/// For `bits(255, 16)`: 255&16=16, 255^16=239, 16|239=255, 255<<2=1020,
///                      1020>>1=510
#[test]
fn bits_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("bits.py");
    assert!(
        rust.contains("checked_shl"),
        "expected checked_shl, got:\n{rust}"
    );
    // PMAT-577: right-shift lowers to a clamp-the-amount block (Python defines
    // `x >> n` for n >= 64 as the sign fill; `checked_shr` would panic), so the
    // emitted form is `__shr_v >> __shr_amt`, not a bare `checked_shr`.
    assert!(
        rust.contains("__shr_v >> __shr_amt"),
        "expected right-shift clamp block, got:\n{rust}"
    );
    assert!(
        rust.contains(" & "),
        "expected infix & for BitAnd, got:\n{rust}"
    );
    assert!(
        rust.contains(" | "),
        "expected infix | for BitOr, got:\n{rust}"
    );
    assert!(
        rust.contains(" ^ "),
        "expected infix ^ for BitXor, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(bits(5, 3), 14);
    assert_eq!(bits(0, 0), 0);
    assert_eq!(bits(255, 16), 510);
    assert_eq!(bits(1, 2), 6);
}
"#;
    assert_rustc_runs("bits", &rust, driver);
}

#[test]
fn transpile_let_sum_py_to_lean_uses_multi_let_form() {
    let py = fixture("let_sum.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "lean"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("def let_sum (a : Int) (b : Int) : Int :="));
    assert!(stdout.contains("let s := (a + b)"));
    assert!(stdout.contains("let t := (s * (2: Int))"));
    assert!(stdout.contains("\n  t\n"));
}

/// PMAT-040 / XPILE-BASHRS-MERGER-001 v0.3.0 falsifier evidence:
/// Python `subprocess.run([str, ...])` lowers to meta-HIR `Stmt::Cmd`
/// via depyler-frontend, then bashrs-backend emits real POSIX shell.
/// This is the LOAD-BEARING cross-domain test — without it, the
/// `sub/bashrs-merger.md` v0.3.0 check-back's "at least one
/// cross-domain consumer must ship" precondition isn't satisfied
/// and XPILE-UNMERGE-001 would eventually trigger.
#[test]
fn transpile_python_subprocess_run_to_shell_via_bashrs_backend() {
    let py = fixture("subprocess_demo.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "shell"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "xpile failed: stderr={stderr} stdout={stdout}"
    );
    // Header invariants: POSIX shebang + bashrs contract citation +
    // module name from the .py file stem.
    assert!(
        stdout.starts_with("#!/bin/sh\n"),
        "expected POSIX shebang at line 1:\n{stdout}"
    );
    assert!(
        stdout.contains("# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE"),
        "missing bashrs contract citation:\n{stdout}"
    );
    assert!(
        stdout.contains("# module: subprocess_demo"),
        "missing module name:\n{stdout}"
    );
    // The 4 subprocess.run([...]) calls lower to 4 shell command lines,
    // emitted in source order. Each line is exactly the args joined
    // by spaces (no quoting yet — XPILE-BASHRS-MERGER-003+).
    for needle in &[
        "\necho starting\n",
        "\nls /tmp\n",
        "\npwd\n",
        "\necho done\n",
    ] {
        assert!(
            stdout.contains(needle),
            "missing emitted command line `{}`:\n{stdout}",
            needle.trim()
        );
    }
    // Per-function divider — emitted because the source function
    // (`build`) is NOT named `main` (the synthesised name reserved
    // for bashrs-frontend's flat-script case).
    assert!(
        stdout.contains("# function: build"),
        "expected per-function divider for `build`:\n{stdout}"
    );
}

/// PMAT-040 negative: subprocess.run shapes that aren't the canonical
/// list-of-string-literals form fail with a precise error so users
/// understand what's supported.
#[test]
fn transpile_python_subprocess_run_with_non_list_arg_fails_with_clear_error() {
    // Build a fixture in /tmp to keep the test self-contained.
    let tmp = std::env::temp_dir().join("xpile-pmat-040-bad.py");
    std::fs::write(
        &tmp,
        "def f() -> int:\n    subprocess.run(cmd)\n    return 0\n",
    )
    .unwrap();
    let out = run_xpile(&["transpile", tmp.to_str().unwrap(), "--target", "shell"]);
    let _ = std::fs::remove_file(&tmp);
    assert!(!out.status.success(), "expected failure for non-list arg");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("subprocess.run") && stderr.contains("list literal"),
        "error should explain the supported shape; got: {stderr}"
    );
}

/// PMAT-730 (HUNT-V10 V10-7): nested subscript assignment with a dict level —
/// `d["a"]["b"] = v`, `d["a"][i] = v` (mixed), `d["a"]["b"][0] = v` (3-level) —
/// now transpiles (was rejected; only `list[list[…]]` nesting was supported). The
/// all-list nest still routes through `IndexAssign` (regression-checked). Negative
/// list indices in the path wrap like Python.
#[test]
fn nested_dict_assign_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("nested_dict_assign.py");
    assert!(
        rust.contains("get_mut(&(String::from(\"a\"))).unwrap()"),
        "expected get_mut navigation for the dict level, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(dict_dict(99), 99);
    assert_eq!(dict_list(50), 50);
    assert_eq!(dict_list_neg(7), 7);
    assert_eq!(three_level(42), 42);
    assert_eq!(list_list_regression(8), 8);
}
"#;
    assert_rustc_runs("nested_dict_assign", &rust, driver);
}

/// PMAT-744 (HUNT-V13 exc-flow-01/02): a typed `except ValueError:` no longer
/// silently swallows an out-of-bounds list IndexError. A runtime list-index read
/// now panics with the `xpile: IndexError:` TAG, so the typed-`except`
/// discrimination (PMAT-731) re-raises it from a non-matching except (Python
/// propagates the IndexError) and catches it under `except IndexError:`. Was a
/// silent-wrong: native untagged bounds panics matched no re-raise pattern.
#[test]
fn except_wrong_type_does_not_swallow_index_error() {
    let rust = xpile_transpile_to_rust("except_wrong_type_index.py");
    assert!(
        rust.contains("xpile: IndexError: list index out of range"),
        "expected a tagged IndexError on list-index OOB, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let xs = vec![1i64, 2, 3];
    // except IndexError catches it (returns -1), like Python:
    assert_eq!(right_except(xs.clone(), 0), 1);
    assert_eq!(right_except(xs.clone(), 99), -1);
    // except ValueError must NOT catch it — the IndexError propagates (panics):
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(move || wrong_except(xs, 99));
    assert!(r.is_err(), "except ValueError must not swallow IndexError (Python propagates)");
}
"#;
    assert_rustc_runs("except_wrong_type_index", &rust, driver);
}

/// PMAT-743 (HUNT-V12 V12-8): inserting a NEW key during dict iteration
/// (`for k in d: d[k+100] = 1`) — a size change — now panics like Python's
/// `RuntimeError: dictionary changed size during iteration`, instead of silently
/// growing the snapshot (the silent-wrong that PMAT-742's key-materialization
/// would otherwise have introduced). A runtime size guard captures `d.len()`
/// before the loop and panics after any body that changes it. (A size-stable
/// value-update — PMAT-742 — leaves the length unchanged, so it stays silent.)
#[test]
fn dict_insert_during_iter_panics_like_runtimeerror() {
    let rust = xpile_transpile_to_rust("dict_insert_during_iter.py");
    assert!(
        rust.contains("__dg_n0") && rust.contains("changed size during iteration"),
        "expected a dict size-change guard, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(1i64, 1i64);
    d.insert(2i64, 1i64);
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(move || grow(d));
    assert!(r.is_err(), "size change during iteration must panic (Python RuntimeError)");
}
"#;
    assert_rustc_runs("dict_insert_during_iter", &rust, driver);
}

/// PMAT-742 (HUNT-V12 V12-2): `for k in d: d[k] = d[k] * 2` — updating dict
/// *values* during iteration (size-stable, legal Python) — now compiles. The
/// lazy `d.keys().cloned()` borrowed `d` for the whole loop, conflicting with the
/// in-body `d.insert(...)` (rustc E0502); when the iterated dict is mutated, its
/// keys are now materialized to an owned `Vec` first, leaving `d` free to mutate.
/// Cross-checked vs python3: `double_vals({1:10,2:20,3:30}) == 120`.
#[test]
fn dict_iter_value_update_compiles_and_runs() {
    let rust = xpile_transpile_to_rust("dict_iter_value_update.py");
    assert!(
        rust.contains("d.keys().cloned().collect::<Vec<_>>()"),
        "expected the mutated dict's keys materialized to an owned Vec, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let mut d = std::collections::HashMap::new();
    d.insert(1i64, 10i64);
    d.insert(2i64, 20i64);
    d.insert(3i64, 30i64);
    assert_eq!(double_vals(d), 120);   // (20 + 40 + 60)
}
"#;
    assert_rustc_runs("dict_iter_value_update", &rust, driver);
}

/// PMAT-741 (HUNT-V12 V12-9/10/11): a mutable-collection default (`[..]` / `{k:v}`
/// / `{..}`) that is MUTATED in the body is rejected with a clear message. Python
/// shares the def-time instance across calls (mutations accumulate); xpile fills
/// a fresh value per call site, which silently diverges. (dict/set use the same
/// path as this list case.)
#[test]
fn mutable_default_mutated_rejected_with_clear_message() {
    let py = fixture("mutable_default_mutated.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        !out.status.success(),
        "a mutated mutable default should reject"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("mutable default argument") && stderr.contains("acc"),
        "expected a clear mutable-default message naming `acc`; got: {stderr}"
    );
}

/// PMAT-741 (HUNT-V12 V12-9/10/11): a READ-ONLY mutable default is unaffected —
/// a fresh copy per call is observably identical to Python's shared instance when
/// never mutated, so it still transpiles. Guards against over-rejection.
/// `run() == pick(1) + pick(2) == 20 + 30 == 50` (cross-checked vs python3).
#[test]
fn readonly_mutable_default_still_transpiles() {
    let rust = xpile_transpile_to_rust("readonly_default.py");
    let driver = "fn main() { assert_eq!(run(), 50); }\n";
    assert_rustc_runs("readonly_default", &rust, driver);
}

/// PMAT-740 (HUNT-V12 V12-24): `(a * b) % m` no longer panics on the
/// intermediate product overflow. The product + floor-mod are widened to i128
/// (the result `(a*b) mod m` fits i64), so the common modular-arithmetic idiom
/// computes the right value even when `a*b` exceeds i64. Cross-checked vs
/// python3: `mod_mul(i64::MAX, i64::MAX, 1e9+7) == 737564071`, floor-mod sign
/// (`mod_mul(-7,3,5) == 4`), non-overflow unchanged (`mod_mul(10,20,7) == 4`).
#[test]
fn mod_mul_i128_no_intermediate_overflow() {
    let rust = xpile_transpile_to_rust("mod_mul_i128.py");
    assert!(
        rust.contains("i128") && rust.contains("as i64"),
        "expected the (a*b)%m product widened to i128, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(mod_mul(9223372036854775807, 9223372036854775807, 1000000007), 737564071);
    assert_eq!(mod_mul(-7, 3, 5), 4);     // Python floor-mod: -21 % 5 == 4
    assert_eq!(mod_mul(10, 20, 7), 4);    // non-overflow case unchanged
}
"#;
    assert_rustc_runs("mod_mul_i128", &rust, driver);
}

/// PMAT-739 (HUNT-V12 V12-31): the integer literal `-9223372036854775808`
/// (exactly `i64::MIN`, fully representable) now transpiles. Python parses it as
/// `USub(Constant(9223372036854775808))`; the bare magnitude `2^63` doesn't fit
/// i64, so it was rejected before the unary minus was applied. The frontend now
/// folds `-<int literal>` in i128 first. Cross-checked vs python3:
/// `min_val() == -9223372036854775808`, `near_min(5) == -9223372036854775803`.
#[test]
fn int_min_literal_transpiles() {
    let rust = xpile_transpile_to_rust("int_min_literal.py");
    assert!(
        rust.contains("-9223372036854775808i64"),
        "expected the i64::MIN literal folded into a single negative literal, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(min_val(), -9223372036854775808i64);    // i64::MIN
    assert_eq!(near_min(5), -9223372036854775803i64);  // MIN + 5
}
"#;
    assert_rustc_runs("int_min_literal", &rust, driver);
}

/// PMAT-739 (HUNT-V12 V12-31): a magnitude past `i64::MIN`
/// (`-9223372036854775809`) still rejects with the bigint-not-implemented
/// message — the fold accepts exactly the i64 range, no more.
#[test]
fn int_past_min_literal_still_rejected() {
    let py = fixture("int_past_min_literal.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        !out.status.success(),
        "a literal past i64::MIN should reject"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does not fit in i64"),
        "expected a bigint-not-implemented message; got: {stderr}"
    );
}

/// PMAT-738 (HUNT-V12 V12-5): a function-local that shadows a module-level
/// constant (`LIMIT = 6` inside a function with a module `LIMIT`) is rejected
/// with a clear message instead of emitting a reassignment of the Rust `const`
/// (E0070) — Rust can't express the shadow (the name is a constant pattern,
/// E0005). A parameter sharing a const name (E0530) is likewise rejected.
#[test]
fn local_shadows_const_rejected_with_clear_message() {
    let py = fixture("local_shadows_const.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        !out.status.success(),
        "a local shadowing a const should reject"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("shadows the module-level constant") && stderr.contains("LIMIT"),
        "expected a clear const-shadow message naming LIMIT; got: {stderr}"
    );
}

/// PMAT-738 (HUNT-V12 V12-5, companion): a parameter sharing a name with a
/// module-level const can't shadow it in Rust (E0530) — rejected with a clear
/// message.
#[test]
fn param_shadows_const_rejected_with_clear_message() {
    let py = fixture("param_shadows_const.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        !out.status.success(),
        "a param shadowing a const should reject"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("parameter `LIMIT` that shadows the module-level constant"),
        "expected a clear param-shadow message; got: {stderr}"
    );
}

/// PMAT-738 (HUNT-V12 V12-5): a function that only READS a module const is
/// unaffected — it resolves to the `const` and round-trips. Guards the regression
/// (the shadow reject must not break ordinary const reads). `scaled(6) == 49`.
#[test]
fn const_read_only_still_resolves() {
    let rust = xpile_transpile_to_rust("const_read_only.py");
    let driver = "fn main() { assert_eq!(scaled(6), 49); }\n";
    assert_rustc_runs("const_read_only", &rust, driver);
}

/// PMAT-737 (HUNT-V12 V12-3): `for x in list(xs): xs.remove(x)` — the canonical
/// "iterate a copy so I can mutate the original" idiom — now compiles. `list(xs)`
/// over a list emits an explicit owned clone (`(xs).clone()`) instead of the bare
/// `xs`, so the loop iterates a temporary and the body is free to mutate `xs`
/// (was rustc E0502: mutable borrow while immutably borrowed). Cross-checked vs
/// python3: `evens_removed([1,2,3,4,5]) == 3`, `copy_independent([1,2,3]) == 7`.
#[test]
fn list_copy_iter_mutate_compiles_and_runs() {
    let rust = xpile_transpile_to_rust("list_copy_iter_mutate.py");
    assert!(
        rust.contains("(xs).clone().iter().cloned()"),
        "expected `list(xs)` to emit an owned clone in iterable position, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(evens_removed(vec![1, 2, 3, 4, 5]), 3);   // 2 and 4 removed
    assert_eq!(copy_independent(vec![1, 2, 3]), 7);      // 3 + 4 (independent copy)
}
"#;
    assert_rustc_runs("list_copy_iter_mutate", &rust, driver);
}

/// PMAT-736 (HUNT-V11 V11-6): a self-referential nested `def` now lowers to a
/// NAMED inner `fn` (which can recurse by name) instead of a `let` closure
/// (which can't reference its own binding → rustc E0425). Cross-checked vs
/// python3: `fact(6) == 720`, `fib(10) == 55`.
#[test]
fn nested_recursive_fn_emits_named_inner_fn() {
    let rust = xpile_transpile_to_rust("nested_recursive_fn.py");
    assert!(
        rust.contains("fn go(k: i64) -> i64") && !rust.contains("let go ="),
        "expected a named inner `fn go` (not a closure binding), got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(fact(6), 720);   // 6! == 720
    assert_eq!(fib(10), 55);    // fib(10) == 55
}
"#;
    assert_rustc_runs("nested_recursive_fn", &rust, driver);
}

/// PMAT-736 (HUNT-V11 V11-6): a recursive nested def that ALSO captures an
/// enclosing local is rejected with a clear message (a named inner `fn` can
/// recurse but cannot capture; a closure can capture but cannot self-call) —
/// not emitted as invalid Rust (E0425/E0434).
#[test]
fn nested_recursive_capturing_rejected_with_clear_message() {
    let py = fixture("nested_recursive_capturing.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        !out.status.success(),
        "a recursive-capturing def should reject"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("recursive nested function")
            && stderr.contains("captures enclosing variable")
            && stderr.contains("base"),
        "expected a clear recursive-capture message naming `base`; got: {stderr}"
    );
}

/// PMAT-735 (HUNT-V11 V11-12): a lambda stored in a list / comprehension (a
/// first-class callable value) is rejected with a clear message naming `lambda`,
/// not the opaque `unsupported expression: Discriminant(4)`. Supported lambda
/// positions (`name = lambda`, `key=`/`map`/`filter`) are unaffected.
#[test]
fn lambda_in_comprehension_rejected_with_clear_message() {
    let py = fixture("lambda_in_comprehension.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(!out.status.success(), "a lambda value should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("lambda expressions are only supported")
            && !stderr.contains("Discriminant"),
        "expected a clear lambda message (no opaque discriminant); got: {stderr}"
    );
}

/// PMAT-734b (HUNT-V11 V11-10): float `b ** e` overflow now raises OverflowError
/// (like Python) instead of silently returning `inf`; `0.0 ** <neg>` raises
/// ZeroDivisionError. Finite results and an already-infinite base are unchanged.
#[test]
fn float_pow_overflow_matches_cpython() {
    let rust = xpile_transpile_to_rust("float_pow_overflow.py");
    assert!(
        rust.contains("OverflowError") && rust.contains("is_infinite() && __pb.is_finite()"),
        "expected the float-pow overflow guard, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(fpow(2.0, 10.0), 1024.0);
    assert_eq!(fpow(1.5, 3.0), 3.375);
    assert!(fpow(f64::INFINITY, 2.0).is_infinite()); // inf base stays inf, no panic
    std::panic::set_hook(Box::new(|_| {}));
    // finite-base overflow -> OverflowError
    let r = std::panic::catch_unwind(|| fpow(2.0, 2000.0));
    assert!(r.is_err());
    // 0.0 ** negative -> ZeroDivisionError
    let z = std::panic::catch_unwind(|| fpow(0.0, -1.0));
    assert!(z.is_err());
}
"#;
    assert_rustc_runs("float_pow_overflow", &rust, driver);
}

/// PMAT-734 (HUNT-V11 V11-5): `all(d)` / `any(d)` over a dict iterate its KEYS and
/// apply per-key truthiness (matching Python), instead of being rejected (bool-
/// annotated) or emitting an undefined `all(d)` (int-annotated). The dict arg is
/// materialized to its keys view (→ `List(K)`) and routed through the existing
/// `BoolReduce` path.
#[test]
fn all_any_dict_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("all_any_dict.py");
    let driver = r#"
fn main() {
    assert_eq!(all_keys(0), false); // a key is 0 -> falsy
    assert_eq!(all_keys(5), true);
    assert_eq!(any_keys(7), true);
    assert_eq!(any_keys(0), false); // both keys 0 -> falsy
    assert_eq!(all_str_keys("".to_string()), false); // an empty-string key
    assert_eq!(all_str_keys("y".to_string()), true);
}
"#;
    assert_rustc_runs("all_any_dict", &rust, driver);
}

/// PMAT-733 (HUNT-V11 V11-3 + V11-4): a `str` is iterable — `max(s)` / `min(s)`
/// (over the code points) and `list(s)` (split into 1-char strings) now transpile
/// (they were rejected as "body produces I64"), via `Expr::StrChars` (→ `List(Str)`),
/// the same view `sorted(s)` uses.
#[test]
fn str_as_iterable_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("str_as_iterable.py");
    assert!(
        rust.contains(".chars().map(|__c| __c.to_string()).collect::<Vec<String>>()"),
        "expected StrChars materialization, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(maxchar("héllo".to_string()), "é");
    assert_eq!(minchar("héllo".to_string()), "h");
    assert_eq!(chars("abc".to_string()), vec!["a", "b", "c"]);
    assert_eq!(joined("xyz".to_string()), "x-y-z");
    assert_eq!(char_count("héllo".to_string()), 5);
}
"#;
    assert_rustc_runs("str_as_iterable", &rust, driver);
}

/// PMAT-732 (HUNT-V10 V10-4): a function returning a nested function / closure now
/// rejects with a clear message (it used to silently mis-infer the closure's
/// call-result type as the return type → rustc E0308), matching the existing
/// `-> Callable[...]` reject. A nested closure USED locally still transpiles.
#[test]
fn return_nested_fn_rejected_with_clear_message() {
    let py = fixture("return_nested_fn.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(
        !out.status.success(),
        "returning a closure should be rejected"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("returns a nested function / closure") && !stderr.contains("Discriminant"),
        "expected a clear returned-callable message; got: {stderr}"
    );
}

/// PMAT-732 regression: a nested closure used LOCALLY (called, not returned) still
/// transpiles and computes correctly.
#[test]
fn local_closure_used_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("local_closure_used.py");
    let driver = r#"
fn main() {
    assert_eq!(adder(5), 12);
}
"#;
    assert_rustc_runs("local_closure_used", &rust, driver);
}

/// PMAT-731 (HUNT-V10 typed-exceptions): a SPECIFIC `except T:` no longer swallows
/// a DIFFERENT exception. `except ValueError:` around `100 // 0` now lets the
/// ZeroDivisionError propagate (it re-raises non-matching known builtin panics);
/// `except Exception:` stays catch-all; `except KeyError:` catches the KeyError.
#[test]
fn typed_except_discrimination_matches_cpython() {
    let rust = xpile_transpile_to_rust("typed_except.py");
    assert!(
        rust.contains("resume_unwind") && rust.contains("xpile: ZeroDivisionError: "),
        "expected re-raise discrimination for the typed except, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    assert_eq!(catch_value(5), 20);
    assert_eq!(catch_value(-1), -1); // ValueError caught
    // ZeroDivisionError must NOT be caught by `except ValueError:` — it propagates.
    assert!(std::panic::catch_unwind(|| catch_value(0)).is_err());
    // `except Exception:` is catch-all.
    assert_eq!(catch_all(0), -99);
    assert_eq!(catch_all(-1), -99);
    // `except KeyError:` catches the missing-key panic.
    let mut a = HashMap::new();
    a.insert("a".to_string(), 7);
    assert_eq!(lookup(a, "a".to_string()), 7);
    assert_eq!(lookup(HashMap::new(), "z".to_string()), -1);
}
"#;
    assert_rustc_runs("typed_except", &rust, driver);
}

/// PMAT-728 (HUNT-V10 typed-exceptions sub-slice): integer `//` / `%` by zero
/// now panics with a `ZeroDivisionError` message (matching Python) instead of the
/// misleading `i64 floor-div overflow` — `checked_div`/`checked_rem` return None
/// for both a zero divisor and the `i64::MIN/-1` overflow, so the divisor is now
/// guarded first. (Prerequisite for `except ZeroDivisionError:` discrimination.)
#[test]
fn int_div_zero_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("int_div_zero.py");
    assert!(
        rust.contains("ZeroDivisionError: integer division or modulo by zero")
            && rust.contains("ZeroDivisionError: integer modulo by zero"),
        "expected ZeroDivisionError guards, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    // Normal values (cross-checked vs python3): Python floor/mod sign follows divisor.
    assert_eq!(fd(7, 2), 3);
    assert_eq!(fd(-7, 2), -4);
    assert_eq!(md(-7, 2), 1);
    assert_eq!(md(7, 3), 1);
    std::panic::set_hook(Box::new(|_| {}));
    for (r, name) in [
        (std::panic::catch_unwind(|| fd(10, 0)), "fd"),
        (std::panic::catch_unwind(|| md(10, 0)), "md"),
    ] {
        let e = r.expect_err("div/mod by zero must panic");
        let msg = e
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("");
        assert!(
            msg.contains("ZeroDivisionError"),
            "{name}: expected ZeroDivisionError, got: {msg}"
        );
    }
}
"#;
    assert_rustc_runs("int_div_zero", &rust, driver);
}

/// PMAT-727 (HUNT-V10 V10-8): the grouping idiom `d.setdefault(k, []).append(v)`
/// now transpiles (was rejected with a misleading subprocess error). Lowers to
/// `d.entry(k).or_insert_with(|| <default>).push(v)` — creating the key's list on
/// first use. The empty-list default threads its element type (PMAT-717).
#[test]
fn setdefault_append_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("setdefault_append.py");
    assert!(
        rust.contains(".entry(k).or_insert_with(|| ") && rust.contains(").push(v)"),
        "expected entry().or_insert_with().push() lowering, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let g = group(vec![
        ("a".to_string(), 1),
        ("b".to_string(), 2),
        ("a".to_string(), 3),
    ]);
    assert_eq!(g.get("a"), Some(&vec![1, 3]));
    assert_eq!(g.get("b"), Some(&vec![2]));
    let g2 = group_nonempty(vec![("a".to_string(), 1), ("a".to_string(), 2)]);
    assert_eq!(g2.get("a"), Some(&vec![0, 1, 2]));
}
"#;
    assert_rustc_runs("setdefault_append", &rust, driver);
}

/// PMAT-726 (HUNT-V10 V10-1): `s.partition(sep)` / `s.rpartition(sep)` with an
/// EMPTY separator now raises ValueError at the call (like Python) instead of
/// silently returning a bogus 3-tuple (the old `split_once("")` returned
/// `Some(("", s))`). Valid separators and the absent case are unchanged.
#[test]
fn partition_empty_sep_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("partition_empty_sep.py");
    assert!(
        rust.contains("__psep.is_empty()") && rust.contains("empty separator"),
        "expected the empty-separator ValueError guard, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(part_ok("a-b-c".to_string()), "a|-|b-c");
    assert_eq!(rpart_ok("a-b-c".to_string()), "a-b|-|c");
    assert_eq!(part_absent("abc".to_string()), "abc||");
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(|| part_empty("abc".to_string(), "".to_string()));
    assert!(r.is_err(), "empty separator must raise (ValueError)");
}
"#;
    assert_rustc_runs("partition_empty_sep", &rust, driver);
}

/// PMAT-725 (HUNT-V10 V10-2): `ord(s[0])` (ord of a string-index expression) now
/// compiles. The index lowering ends in a `.to_string()` temporary; calling
/// `.chars()` directly on it borrowed a value dropped at statement end (rustc
/// E0716). Binding `let __os = &(...)` lifetime-extends the temporary (and borrows
/// — not moves — a String variable, so `ord(s)` and later uses of `s` still work).
#[test]
fn ord_index_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("ord_index.py");
    assert!(
        rust.contains("let __os = &("),
        "expected the bound-temp ord lowering, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(ord_first("Z".to_string()), 90);
    assert_eq!(ord_first("é".to_string()), 233);
    assert_eq!(ord_arith("c".to_string()), 2);
    assert_eq!(ord_var("A".to_string()), 65);
    assert_eq!(ord_reuse("hi".to_string()), 106);
}
"#;
    assert_rustc_runs("ord_index", &rust, driver);
}

/// PMAT-724 (HUNT-V9 V9-19): `x or default` where `x` is `Optional[T]` and
/// `default` is `T` returns the inner value when `x` is truthy, else `default` —
/// `config.get(k) or "x"`. Lowers to `(x).filter(|v| truthy(v)).unwrap_or_else(||
/// default)` (the operand is evaluated once; the default stays lazy). Was rejected.
#[test]
fn or_default_optional_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("or_default_optional.py");
    assert!(
        rust.contains(".filter(|") && rust.contains(".unwrap_or_else(|| "),
        "expected filter+unwrap_or_else lowering, got:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    assert_eq!(or_int(None), -1);
    assert_eq!(or_int(Some(0)), -1);
    assert_eq!(or_int(Some(5)), 5);
    assert_eq!(or_str(None), "default");
    assert_eq!(or_str(Some("".to_string())), "default");
    assert_eq!(or_str(Some("hi".to_string())), "hi");
    let mut a = HashMap::new();
    a.insert("a".to_string(), 7);
    let mut b = HashMap::new();
    b.insert("a".to_string(), 0);
    assert_eq!(or_via_get(a, "a".to_string()), 7);
    assert_eq!(or_via_get(b, "a".to_string()), 99);
    assert_eq!(or_via_get(HashMap::new(), "z".to_string()), 99);
}
"#;
    assert_rustc_runs("or_default_optional", &rust, driver);
}

/// PMAT-723 (HUNT-V9 V9-18 follow-up): `if x:` over a non-reassigned `Optional[T]`
/// now narrows `x` to `Some` in the then-body, so a read of `x` there unwraps to
/// `T` (`if x: return x + "!"`). A truthy Optional is necessarily `Some`. Mirrors
/// the existing `if x is not None:` then-body narrowing.
#[test]
fn if_truthy_narrow_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("if_truthy_narrow.py");
    assert!(
        rust.contains("(x).unwrap()"),
        "expected then-body unwrap of the narrowed Optional, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(use_after_str(None), "none");
    assert_eq!(use_after_str(Some("".to_string())), "none");
    assert_eq!(use_after_str(Some("hi".to_string())), "hi!");
    assert_eq!(use_after_int(None), -1);
    assert_eq!(use_after_int(Some(0)), -1);
    assert_eq!(use_after_int(Some(5)), 105);
    assert_eq!(double_use(None), 0);
    assert_eq!(double_use(Some(0)), 0);
    assert_eq!(double_use(Some(4)), 16);
}
"#;
    assert_rustc_runs("if_truthy_narrow", &rust, driver);
}

/// PMAT-722 (HUNT-V9 V9-18 follow-up): `not x` over an `Optional[T]` is the
/// negation of its truthiness — `not None` is True, `not Some(v)` is
/// `not <truthy(v)>`. Reuses the `OptionTruthy` lowering wrapped in `UnOp::Not`.
#[test]
fn not_optional_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("not_optional.py");
    assert!(
        rust.contains("!(x).is_some_and") || rust.contains("!(x).as_ref().is_some_and"),
        "expected negated is_some_and, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(not_int(None), true);
    assert_eq!(not_int(Some(0)), true);
    assert_eq!(not_int(Some(5)), false);
    assert_eq!(not_str(None), true);
    assert_eq!(not_str(Some("".to_string())), true);
    assert_eq!(not_str(Some("hi".to_string())), false);
    assert_eq!(not_list(None), true);
    assert_eq!(not_list(Some(vec![])), true);
    assert_eq!(not_list(Some(vec![1])), false);
}
"#;
    assert_rustc_runs("not_optional", &rust, driver);
}

/// PMAT-721 (HUNT-V9 V9-18): Python truthiness of an `Optional[T]` value in a
/// boolean context — `if x:`, `while x:`, `assert x`, a ternary condition — now
/// transpiles (was rejected as "condition does not type as bool"). None is falsy;
/// `Some(v)` is truthy iff `v` is, via `(x)[.as_ref()].is_some_and(|__v| <truthy>)`.
/// (Using the *narrowed* value inside the truthy branch — `if x: return x + …` —
/// still needs flow-narrowing, a separate follow-up; this covers the condition.)
#[test]
fn optional_truthiness_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("optional_truthiness.py");
    assert!(
        rust.contains("is_some_and(|__v|"),
        "expected is_some_and truthiness lowering, got:\n{rust}"
    );
    assert!(
        rust.contains(".as_ref().is_some_and"),
        "expected as_ref() for the non-Copy str/list inners, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(if_int(None), "F");
    assert_eq!(if_int(Some(0)), "F");
    assert_eq!(if_int(Some(5)), "T");
    assert_eq!(if_str(None), "F");
    assert_eq!(if_str(Some("".to_string())), "F");
    assert_eq!(if_str(Some("hi".to_string())), "T");
    assert_eq!(if_list(None), "F");
    assert_eq!(if_list(Some(vec![])), "F");
    assert_eq!(if_list(Some(vec![1])), "T");
    assert_eq!(ternary(None), -1);
    assert_eq!(ternary(Some(0)), -1);
    assert_eq!(ternary(Some(7)), 100);
    assert_eq!(while_opt(None), 0);
    assert_eq!(while_opt(Some(7)), 1);
    std::panic::set_hook(Box::new(|_| {}));
    assert!(std::panic::catch_unwind(|| assert_opt(Some(0))).is_err());
    assert_eq!(assert_opt(Some(9)), 42);
}
"#;
    assert_rustc_runs("optional_truthiness", &rust, driver);
}

/// PMAT-720 (HUNT-V8 V8-EXTRA): a dict literal whose key/value is a bare variable
/// named the same as the codegen accumulator (`{m: 1}` where the param is `m`) now
/// compiles and binds the right key. The accumulator is `__xpile_map`, not `m`, so
/// the inner `let mut` no longer shadows the user's `m` (which made the key
/// reference the HashMap — inserting the map into itself, E0275).
#[test]
fn dict_literal_var_key_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("dict_literal_var_key.py");
    assert!(
        rust.contains("__xpile_map"),
        "expected collision-proof accumulator name, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(f(5), 3);
    assert_eq!(g("x".to_string(), 7), 9);
}
"#;
    assert_rustc_runs("dict_literal_var_key", &rust, driver);
}

/// PMAT-719 (HUNT-V9 V9-13): set algebra over dict key views — `a.keys() & b.keys()`
/// (and `|`/`-`/`^`, plus a key-view against a plain set) now transpiles to a Rust
/// `HashSet` set operation instead of `Vec & Vec` (E0369). The result agrees with
/// CPython (the views are materialized as sets, then intersected / unioned / etc.).
#[test]
fn dict_view_setops_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("dict_view_setops.py");
    assert!(
        rust.contains(".intersection(") && rust.contains(".union("),
        "expected HashSet set ops, got:\n{rust}"
    );
    let driver = r#"
use std::collections::{HashMap, HashSet};
fn d(p: &[(&str, i64)]) -> HashMap<String, i64> {
    p.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}
fn main() {
    let a = d(&[("x", 1), ("y", 2), ("z", 3)]);
    let b = d(&[("y", 3), ("z", 4), ("w", 5)]);
    let s: HashSet<String> = ["x", "w"].iter().map(|x| x.to_string()).collect();
    assert_eq!(inter(a.clone(), b.clone()), 2);
    assert_eq!(uni(a.clone(), b.clone()), 4);
    assert_eq!(diff(a.clone(), b.clone()), 1);
    assert_eq!(sym(a.clone(), b.clone()), 2);
    assert_eq!(key_vs_set(a.clone(), s), 1);
}
"#;
    assert_rustc_runs("dict_view_setops", &rust, driver);
}

/// PMAT-718 (HUNT-V9 V9-3): `int(s, base)` now validates PEP-515 underscore
/// placement before stripping — a leading / trailing / doubled underscore raises
/// ValueError like Python, instead of silently stripping and returning a wrong
/// value (`int("1__0", 16)` gave 16). A legal underscore right after the base
/// prefix (`int("0x_ff", 16)` → 255) and between digits (`int("1_000", 16)` →
/// 4096) still parse.
#[test]
fn radix_underscore_validation_matches_cpython() {
    let rust = xpile_transpile_to_rust("radix_underscore.py");
    assert!(
        rust.contains("contains(\"__\")"),
        "expected doubled-underscore guard, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(parse_hex("1_000".to_string()), 4096);
    assert_eq!(parse_hex("0x_ff".to_string()), 255);
    assert_eq!(parse_hex("ff".to_string()), 255);
    std::panic::set_hook(Box::new(|_| {}));
    for bad in ["1__0", "_10", "10_", "0x__ff"] {
        let r = std::panic::catch_unwind(|| parse_hex(bad.to_string()));
        assert!(r.is_err(), "expected ValueError panic for {bad}");
    }
}
"#;
    assert_rustc_runs("radix_underscore", &rust, driver);
}

/// PMAT-717 (HUNT-V9 V9-20): a non-empty dict/list literal whose declared value
/// (or element) type is a known collection and at least one value is itself an
/// empty collection — `{0: [], 1: []}` over `dict[int, list[int]]`, `[[], [1]]`
/// over `list[list[int]]`. The declared type is threaded into the empty values so
/// they get their element type instead of rejecting ("empty list literal requires
/// a type annotation"). The emitted Rust agrees with CPython.
#[test]
fn nested_empty_literal_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("nested_empty_literal.py");
    assert!(
        rust.contains("HashMap<i64, Vec<i64>>"),
        "expected dict-of-lists binding, got:\n{rust}"
    );
    assert!(
        rust.contains("vec![vec![], vec![1i64], vec![]]"),
        "expected threaded list-of-lists literal, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    let g = groups();
    assert_eq!(g.get(&0), Some(&vec![5]));
    assert_eq!(g.get(&1), Some(&vec![9, 8]));
    assert_eq!(matrix(), vec![vec![7], vec![1], vec![]]);
    let s = str_groups();
    assert_eq!(s.get("a"), Some(&vec!["hello".to_string()]));
    assert_eq!(s.get("b"), Some(&vec!["x".to_string()]));
}
"#;
    assert_rustc_runs("nested_empty_literal", &rust, driver);
}

/// PMAT-716 (HUNT-V9 V9-38): a bare `None` literal in value position — a tuple
/// element `(a, None)`, an all-`None` tuple, or a dict value — lowers to
/// `Option::None` instead of rejecting with "unsupported constant:
/// Discriminant(0)". The emitted Rust agrees with CPython:
///   pair_none(5) == (5, None); all_none() == (None, None);
///   dict_none() == {"a": None}.
#[test]
fn none_literal_in_tuple_emitted_rust_matches_cpython() {
    let rust = xpile_transpile_to_rust("none_in_tuple.py");
    assert!(
        rust.contains("(a, None)"),
        "expected bare `None` tuple element, got:\n{rust}"
    );
    assert!(
        rust.contains("(None, None)"),
        "expected all-`None` tuple, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pair_none(5), (5, None));
    assert_eq!(all_none(), (None, None));
    let d = dict_none();
    assert_eq!(d.get("a"), Some(&None));
}
"#;
    assert_rustc_runs("none_in_tuple", &rust, driver);
}

/// PMAT-729 (HUNT-V10 V10-5): a `bytes` literal (`b"..."`) is rejected with a
/// clear message naming `bytes`, not the opaque `unsupported constant:
/// Discriminant(3)`. (python3 accepts bytes; xpile defers them — a clearly
/// messaged capability gap.)
#[test]
fn bytes_literal_rejected_with_clear_message() {
    let py = fixture("bytes_literal.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
    assert!(!out.status.success(), "bytes literal should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bytes literals") && !stderr.contains("Discriminant"),
        "expected a clear bytes message (no opaque discriminant); got: {stderr}"
    );
}

/// PMAT-729 (HUNT-V10 V10-6): slice assignment (`xs[a:b] = ...`) and slice
/// deletion (`del xs[a:b]`) are rejected with a clear message, not the opaque
/// `unsupported expression: Discriminant(26)`. Slice READS still transpile.
#[test]
fn slice_assign_and_del_rejected_with_clear_message() {
    for fx in ["slice_assign.py", "del_slice.py"] {
        let py = fixture(fx);
        let out = run_xpile(&["transpile", py.to_str().unwrap(), "--target", "rust"]);
        assert!(
            !out.status.success(),
            "{fx}: slice target should be rejected"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("slice assignment") && !stderr.contains("Discriminant"),
            "{fx}: expected a clear slice message (no opaque discriminant); got: {stderr}"
        );
    }
}

/// PMAT-745 (HUNT-V13 intfloat-cmp-precision): Python compares an `int` and a
/// `float` EXACTLY — it never rounds the int operand to `f64`. xpile used to
/// cast the int side to f64 before comparing (`(n) as f64 == f`), losing
/// precision above 2^53 and able to invert the ordering. The fix emits an exact
/// comparison (strict ordering via `n as f64`, equality tie broken in `i128`).
/// One node covers all six operators in either operand order. Cross-checked vs
/// python3 (the boundary cases below print exactly what CPython does).
#[test]
fn intfloat_cmp_precision() {
    let rust = xpile_transpile_to_rust("intfloat_cmp_precision.py");
    assert!(
        // the int operand is no longer rounded — it appears in an i128 tiebreak.
        rust.contains("(__cn as i128)") && rust.contains("__cnf = __cn as f64"),
        "int/float comparison should be exact (i128 tiebreak), not a lossy f64 cast:\n{rust}"
    );
    let driver = r#"
fn main() {
    // 2^53 + 1 vs 2^53.0: the int is NOT representable in f64, so a lossy cast
    // would conflate them. Python: 993 != 992.0, 993 > 992.0.
    let big: i64 = 9007199254740993;     // 2^53 + 1
    let bigf: f64 = 9007199254740992.0;  // 2^53
    assert_eq!(eq_if(big, bigf), 0);     // not equal
    assert_eq!(ne_if(big, bigf), 1);
    assert_eq!(lt_if(big, bigf), 0);
    assert_eq!(gt_if(big, bigf), 1);
    assert_eq!(ge_if(big, bigf), 1);
    // i64::MAX (= 2^63 - 1) vs 2^63.0 — the int rounds UP to 2^63 in f64.
    let mx: i64 = 9223372036854775807;
    let p63: f64 = 9223372036854775808.0;
    assert_eq!(lt_if(mx, p63), 1);       // i64::MAX < 2^63
    assert_eq!(eq_if(mx, p63), 0);
    assert_eq!(gt_if(mx, p63), 0);
    // exact small values still agree
    assert_eq!(eq_if(5, 5.0), 1);
    assert_eq!(lt_if(5, 6.0), 1);
    // float-on-left flips correctly: f <= n
    assert_eq!(fle_n(5.0, 5), 1);
    assert_eq!(fle_n(5.5, 5), 0);
    // NaN: Python `n != nan` is True, all others False.
    assert_eq!(eq_if(5, f64::NAN), 0);
    assert_eq!(ne_if(5, f64::NAN), 1);
    assert_eq!(lt_if(5, f64::NAN), 0);
    assert_eq!(gt_if(5, f64::NAN), 0);
    assert_eq!(ge_if(5, f64::NAN), 0);
    // infinities
    assert_eq!(lt_if(5, f64::INFINITY), 1);
    assert_eq!(gt_if(5, f64::NEG_INFINITY), 1);
}
"#;
    assert_rustc_runs("intfloat_cmp_precision", &rust, driver);
}

/// PMAT-746 (HUNT-V14 bool-augadd-i64-coerce): Python's `bool` is an `int`
/// subtype, so an augmented assignment with a bool operand into an int
/// accumulator is integer arithmetic (`count += a == b` counts matches). The
/// plain `total + (a == b)` path coerced the bool to i64 (PMAT-565), but the
/// AUGMENTED path emitted `(count).checked_add(<bool>)` with no cast → rustc
/// E0308. The fix mirrors the plain arm's `to_i64_operand` coercion; `&`/`|`/`^`
/// over two bools stays a bool op (PMAT-580). Cross-checked vs python3.
#[test]
fn bool_augassign_int() {
    let rust = xpile_transpile_to_rust("bool_augassign_int.py");
    assert!(
        // the bool RHS is now cast to i64 inside the checked op…
        rust.contains("checked_add((((ch == String::from(\" \"))) as i64))")
            // …while bitwise &= over two bools stays a bare bool op (no cast).
            && rust.contains("flag = (flag & c)"),
        "bool augmented-assign into an int must coerce the bool to i64; bool &= bool must stay bool:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(count_spaces("a b c d".to_string()), 3);
    assert_eq!(acc_mul(7, true), 7);
    assert_eq!(acc_mul(7, false), 0);
    assert_eq!(sub_bool(10, true), 9);
    assert_eq!(subscript_bool(true), 11);
    assert_eq!(bool_and(true, false), false);
    assert_eq!(bool_and(true, true), true);
}
"#;
    assert_rustc_runs("bool_augassign_int", &rust, driver);
}

/// PMAT-747 (HUNT-V14 #2 exc-untagged-panic-swallowed): a dict-index miss
/// (KeyError), an empty `list.pop()` / absent `dict.pop(k)` emitted UNTAGGED
/// native panics (HashMap `Index`, `Option::unwrap`), so the typed-`except`
/// re-raise filter (PMAT-731 — only re-raises `xpile: <KnownExc>:`-tagged
/// panics) let an unrelated `except` SILENTLY SWALLOW them where Python
/// propagates. Each container-access panic is now tagged; the matching `except`
/// catches it. Cross-checked vs python3 (continues PMAT-743/744).
#[test]
fn container_panic_typed() {
    let rust = xpile_transpile_to_rust("container_panic_typed.py");
    assert!(
        // dict-index miss is now a tagged KeyError (no bare `d[&(...)]` Index)…
        rust.contains("panic!(\"xpile: KeyError: key not found\")")
            // …and an empty list.pop() is a tagged IndexError.
            && rust.contains("xpile: IndexError: pop from empty list"),
        "container access must panic with an xpile: tag for typed-except discrimination:\n{rust}"
    );
    let driver = r#"
use std::collections::HashMap;
fn main() {
    let mut d: HashMap<String, i64> = HashMap::new();
    d.insert("a".to_string(), 5);
    assert_eq!(dict_miss_right(d.clone()), -7);
    assert_eq!(empty_pop_right(vec![]), -3);
    assert_eq!(dict_pop_miss_right(d.clone()), -9);
    assert_eq!(dict_hit(d.clone()), 5);
    // an unrelated `except ValueError` must NOT swallow the KeyError — the
    // GENERATED discrimination must re-raise it (Python propagates), so calling
    // dict_miss_wrong panics rather than returning -1.
    let propagated = std::panic::catch_unwind(|| {
        let mut d2: HashMap<String, i64> = HashMap::new();
        d2.insert("a".to_string(), 5);
        dict_miss_wrong(d2)
    });
    assert!(propagated.is_err(), "KeyError must propagate through except ValueError, not be swallowed");
}
"#;
    assert_rustc_runs("container_panic_typed", &rust, driver);
}

/// PMAT-748 (HUNT-V14 #3 str-literal-control-byte-escape): a Python string
/// literal with control bytes must round-trip exactly. xpile emitted the raw
/// bytes into the Rust literal — a bare CR is a rustc error and a raw CRLF gets
/// normalized to LF (the CR silently dropped, wrong `len`). Control bytes are
/// now escaped in both plain literals and f-string literal segments.
/// Cross-checked vs python3.
#[test]
fn str_control_byte_escape() {
    let rust = xpile_transpile_to_rust("str_control_byte_escape.py");
    assert!(
        // the CR is now an escape sequence, never a raw byte…
        rust.contains("String::from(\"data\\r\")")
            // …and an f-string literal segment escapes its CRLF too.
            && rust.contains("String::from(\"row\\r\\n\")"),
        "string-literal control bytes must be escaped (not raw):\n{rust}"
    );
    // belt-and-braces: no RAW CR byte (0x0d) leaked into the emitted source.
    assert!(
        !rust.contains('\r'),
        "emitted Rust must contain no raw CR byte:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(cr_len(), 5);    // "data\r" — 5 chars (was a compile error)
    assert_eq!(crlf_len(), 4);  // "x\r\ny" — CR preserved (was 3)
    assert_eq!(tab_len(), 3);
    assert_eq!(nul_len(), 3);
    assert_eq!(esc_in_fstring_len(7), 6);  // "row\r\n7"
}
"#;
    assert_rustc_runs("str_control_byte_escape", &rust, driver);
}

/// PMAT-749 (HUNT-V14 #4 nf-reassign-immutable): a nested function reassigning
/// a local, an accumulator, or a parameter emitted an immutable `let` /
/// closure-arg → rustc E0384. The outer mutability analysis doesn't descend
/// into nested defs, so the nested scope's reassigned names were never marked
/// `mut` (though the same code at top level works). The fix computes the nested
/// scope's mutable set: reassigned locals get `let mut`, a reassigned parameter
/// gets `|mut p|`. Cross-checked vs python3.
#[test]
fn nested_fn_reassign_mut() {
    let rust = xpile_transpile_to_rust("nested_fn_reassign_mut.py");
    assert!(
        // a reassigned nested local is now `let mut`…
        rust.contains("let mut r: i64 = n")
            // …and a reassigned nested parameter is now `|mut n|`.
            && rust.contains("|mut n: i64|"),
        "nested-fn reassigned local/param must be mut:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(doubler(), 42);
    assert_eq!(accumulate(vec![1, 2, 3, 4]), 10);
    assert_eq!(reassign_param(), 6);
}
"#;
    assert_rustc_runs("nested_fn_reassign_mut", &rust, driver);
}

/// PMAT-750 (HUNT-V14 #6 dc-order-sort-no-ord): `@dataclass(order=True)` derived
/// only `PartialOrd`, but `.sort()`/`sorted()` need `Ord` → rustc E0277. When
/// every field is Ord-able (int/bool/str) the dataclass now also derives `Ord`
/// (+`Eq`); a float field keeps `PartialOrd` only (f64 is not `Ord`).
/// Cross-checked vs python3.
#[test]
fn dataclass_order_sort() {
    let rust = xpile_transpile_to_rust("dataclass_order_sort.py");
    assert!(
        // an all-int order=True dataclass now derives Ord (+ Eq) so it can sort.
        rust.contains("#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]"),
        "order=True dataclass with Ord-able fields must derive Ord:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(first_after_sort(), 2);  // (1,2) sorts first → y == 2
    assert_eq!(sorted_builtin(), 1);    // x sorted ascending → first x == 1
    assert_eq!(compare_still_works(), 1);
}
"#;
    assert_rustc_runs("dataclass_order_sort", &rust, driver);
}

/// PMAT-751 (HUNT-V14 #5 bool-dict-key-int-keyed): indexing an INT-keyed dict
/// with a bool (`d[True]`) emitted `.get(&true)` over a `HashMap<i64, _>` →
/// rustc E0308. Python's `bool` is an `int` subtype (`hash(True) == hash(1)`),
/// so the bool key is now coerced to i64 when the dict key type is int; a
/// genuinely `dict[bool, V]` keeps its bool key. Cross-checked vs python3.
#[test]
fn bool_dict_key() {
    let rust = xpile_transpile_to_rust("bool_dict_key.py");
    assert!(
        // bool key into an int dict is coerced…
        rust.contains("get(&(((true) as i64)))")
            // …while a bool-keyed dict keeps the bare bool key.
            && rust.contains("get(&(b))"),
        "bool key into an int dict must coerce to i64; a bool-keyed dict must not:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(idx_true(), 100);
    assert_eq!(idx_false(), 200);
    assert_eq!(bool_keyed(true), 5);
    assert_eq!(bool_keyed(false), 9);
}
"#;
    assert_rustc_runs("bool_dict_key", &rust, driver);
}

/// PMAT-752 (HUNT-V14 #16 dc-frozen-mutation-not-enforced): a `@dataclass(
/// frozen=True)` instance is immutable — Python raises FrozenInstanceError on a
/// field assignment. xpile compiled `p.x = 99` and silently mutated. It's now
/// rejected at transpile time with a clear message.
#[test]
fn frozen_dataclass_mutation_rejected() {
    let py = fixture("frozen_dataclass_mutation.py");
    let out = run_xpile(&["transpile", py.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "mutating a frozen dataclass field must be rejected, not silently emitted"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("frozen dataclass") && stderr.contains("FrozenInstanceError"),
        "the rejection should name the frozen-dataclass divergence:\n{stderr}"
    );
}

/// PMAT-752 companion: the reject must not over-reject — a frozen field READ
/// and a NON-frozen dataclass mutation still transpile. Cross-checked vs python3.
#[test]
fn frozen_dataclass_read_ok() {
    let rust = xpile_transpile_to_rust("frozen_dataclass_read_ok.py");
    let driver = r#"
fn main() {
    assert_eq!(mut_ok(), 5);
    assert_eq!(frozen_read(), 7);
}
"#;
    assert_rustc_runs("frozen_dataclass_read_ok", &rust, driver);
}

/// PMAT-753 (HUNT-V15 ONF-1/ONF-2): a concrete value passed to an `Optional[T]`
/// parameter (`f(5)`), or assigned to an `Optional[T]` local (`y: Optional[int]
/// = 5`), was emitted as a bare `T` against a Rust `Option<T>` slot → rustc
/// E0308. The value is now wrapped in `Some(...)` (via the callee's param types
/// at the call site, and the declared type at a let-init); `None` and an
/// already-Optional value pass through. Cross-checked vs python3.
#[test]
fn optional_some_wrap() {
    let rust = xpile_transpile_to_rust("optional_some_wrap.py");
    assert!(
        // a literal call arg is wrapped, and an annotated-local init is wrapped…
        rust.contains("f(Some(5i64))") && rust.contains("let y: Option<i64> = Some(5i64)")
            // …while None passes through unwrapped.
            && rust.contains("f(None)"),
        "concrete value into an Optional slot must be Some-wrapped; None passes through:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(via_lit(), 5);
    assert_eq!(via_none(), 0);
    assert_eq!(local_init(), 5);
}
"#;
    assert_rustc_runs("optional_some_wrap", &rust, driver);
}

/// PMAT-754 (HUNT-V15 #1 COMP-SHADOW-RANGE-LIST): a nested comprehension whose
/// inner loop-var shadows an enclosing comp var of the same name resolved to the
/// leaked outer `__forcN` while-counter instead of the inner binding —
/// `[[x for x in range(5)] for x in range(2)]` made every inner row `[counter;
/// 5]` (sum 5 vs Python 20, silent-wrong; rustc warned `unused variable: x`).
/// The fix clears the active rename when an inner comp re-binds the shadowed
/// name. Cross-checked vs python3.
#[test]
fn nested_comp_shadow() {
    let rust = xpile_transpile_to_rust("nested_comp_shadow.py");
    assert!(
        // the inner element now returns the local binding `x`, not `__forc0`.
        rust.contains("let x = __k.clone(); x }"),
        "inner comp element must resolve to its own binding, not the outer counter:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(nested_sum(), 20);
    assert_eq!(distinct_names(), 6);
    assert_eq!(shadow_over_list(), 50);
}
"#;
    assert_rustc_runs("nested_comp_shadow", &rust, driver);
}

/// PMAT-755 (HUNT-V15 #2 AUG-1): an augmented subscript assignment with a
/// side-effecting index/key double-evaluated it (once for the write target,
/// once for the implicit current-value read) → a `.pop()` index ran twice
/// (wrong slot, silent-wrong); it also missed marking the index's pop-receiver
/// `mut` (E0596). The index/key is now bound to one temp and reused; a pure
/// index is unchanged. Cross-checked vs python3.
#[test]
fn aug_subscript_index_once() {
    let rust = xpile_transpile_to_rust("aug_subscript_index_once.py");
    assert!(
        // the side-effecting index is bound to one `__augi` temp (evaluated once)…
        rust.contains("let __augi0: i64 = (q).remove(")
            // …while a pure index `i + 1` is duplicated inline (no temp).
            && rust.contains("(i).checked_add(1i64)"),
        "side-effecting subscript-aug index must be bound once; a pure index stays inline:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(list_idx(), 110);
    assert_eq!(dict_key(), 101);
    assert_eq!(pure_idx(0), 25);
}
"#;
    assert_rustc_runs("aug_subscript_index_once", &rust, driver);
}

/// PMAT-756 (HUNT-V15 #9 STR-MOVE-RECEIVER): a str method that moves the
/// receiver in codegen (zfill/center/rjust/removeprefix/…) made reusing the
/// source variable a use-after-move (rustc E0382). The receiver is now cloned
/// when reused; a single use and borrow-only methods are unchanged.
/// Cross-checked vs python3.
#[test]
fn str_method_move_receiver() {
    let rust = xpile_transpile_to_rust("str_method_move_receiver.py");
    assert!(
        // a reused receiver is cloned…
        rust.contains("let __s = ((s).clone())")
            // …while a single-use receiver stays bare (no churn).
            && rust.contains("{ let __s = (s); let __w"),
        "a reused move-receiver str method must clone; a single use must not:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(pad_and_len("12".to_string()), 10);
    assert_eq!(strip_prefix_reuse("foobar".to_string()), 9);
    assert_eq!(count_reuse("banana".to_string()), 3 + 6);
    assert_eq!(single_use("12".to_string()), 8);
}
"#;
    assert_rustc_runs("str_method_move_receiver", &rust, driver);
}

/// PMAT-757 (HUNT-V15 #8): `isinstance(x, T)` was emitted verbatim → rustc E0425
/// (`isinstance`/`int` undefined). xpile is statically typed, so it now folds to
/// a const bool (incl. Python's `bool`-is-a-`int` rule and the type-tuple form).
/// Cross-checked vs python3.
#[test]
fn isinstance_fold() {
    let rust = xpile_transpile_to_rust("isinstance_fold.py");
    assert!(
        // folded to const bools — no bare `isinstance(` / `int)` call remains.
        !rust.contains("isinstance("),
        "isinstance must fold to a const bool, not emit verbatim:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(check_int(5), 1);
    assert_eq!(str_is_not_int("x".to_string()), 0);
    assert_eq!(bool_is_int(true), 1);    // bool is-a int (Python)
    assert_eq!(int_is_not_bool(5), 0);   // int is NOT bool
    assert_eq!(tuple_types(1.5), 1);     // float matches (int, float)
    assert_eq!(list_check(vec![1, 2]), 1);
}
"#;
    assert_rustc_runs("isinstance_fold", &rust, driver);
}

/// PMAT-758 (HUNT-V15 CME-2): a string forward-reference annotation
/// (`-> "Counter"`, `x: "Counter"`) — idiomatic for a method returning its own
/// class — was rejected, so a classmethod alternative-constructor's result
/// couldn't be used (E0308 / non-struct). A bare-identifier string annotation
/// now resolves via the name→Type mapping. Cross-checked vs python3.
#[test]
fn string_forward_ref_annotation() {
    let rust = xpile_transpile_to_rust("string_forward_ref_annotation.py");
    assert!(
        // the classmethod's `-> "Counter"` resolved to the struct type.
        rust.contains("fn zero() -> Counter") && rust.contains("let c: Counter = Counter::zero()"),
        "a string forward-ref annotation must resolve to the class type:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(make_zero(), 0);
    assert_eq!(make_of(), 7);
    assert_eq!(takes_fwdref(Counter { n: 3 }), 3);
}
"#;
    assert_rustc_runs("string_forward_ref_annotation", &rust, driver);
}

/// PMAT-759 (HUNT-V15 ONF-4): Optional flow-narrowing through a compound
/// `x is not None and <rest>` — `<rest>` kept x as Option (E0308) and the body
/// kept x un-narrowed. The `is not None` conjunct now narrows x for the later
/// `and` operands and the then-body. Cross-checked vs python3.
#[test]
fn optional_narrow_compound() {
    let rust = xpile_transpile_to_rust("optional_narrow_compound.py");
    assert!(
        // x narrowed in the `and` operand and in the body.
        rust.contains("(x).is_some() && ((x).unwrap() > 0i64)")
            && rust.contains("((x).unwrap()).checked_add(1i64)"),
        "Optional must narrow through `is not None and ...` in both the condition and body:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(guard_and_use(Some(5)), 6);
    assert_eq!(guard_and_use(Some(-3)), -1);
    assert_eq!(guard_and_use(None), -1);
    assert_eq!(guard_chain(Some(5)), 10);
    assert_eq!(guard_chain(Some(200)), -1);
    assert_eq!(guard_chain(None), -1);
    assert_eq!(simple_still_works(Some(5)), 15);
    assert_eq!(simple_still_works(None), 0);
}
"#;
    assert_rustc_runs("optional_narrow_compound", &rust, driver);
}

/// PMAT-760 (HUNT-V15 #6): a dataclass instance in an f-string emitted
/// `format!("{}", obj)` but the struct only derives Debug → rustc E0277. The
/// backend now generates a Python-repr `Display` for an all-int/bool dataclass,
/// so it renders as `ClassName(f=v, …)`. Cross-checked vs python3.
#[test]
fn dataclass_fstring_repr() {
    let rust = xpile_transpile_to_rust("dataclass_fstring_repr.py");
    assert!(
        rust.contains("impl std::fmt::Display for P")
            && rust.contains("write!(__f, \"P(x={}, y={})\", self.x, self.y)")
            // bool field renders True/False (Python repr), not Rust true/false.
            && rust.contains("if self.a { \"True\" } else { \"False\" }"),
        "an all-int/bool dataclass must generate a Python-repr Display:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(lone(), "P(x=1, y=2)");
    assert_eq!(multi(), "pt=P(x=3, y=4)");
    assert_eq!(with_bool(), "flags=Flags(a=True, n=5)");
}
"#;
    assert_rustc_runs("dataclass_fstring_repr", &rust, driver);
}

/// PMAT-761 (HUNT-V16 CFD-3): `len(x) < N` emitted `x.len() as i64 < N`, and
/// rustc reads `i64 <` as a turbofish → a hard parse error (only `<` triggered
/// it). Parenthesizing the cast — `(x.len() as i64)` — disambiguates it
/// everywhere. Cross-checked vs python3.
#[test]
fn len_cmp_paren() {
    let rust = xpile_transpile_to_rust("len_cmp_paren.py");
    assert!(
        rust.contains("((xs.len() as i64) < 6i64)"),
        "the len() cast must be parenthesized so `< N` isn't read as a turbofish:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(count_while_small(vec![1, 2, 3]), 3);
    assert_eq!(while_len_lt(4), 4);
    assert_eq!(len_le_and_gt(vec![1, 2]), 11);
}
"#;
    assert_rustc_runs("len_cmp_paren", &rust, driver);
}

/// PMAT-762 (HUNT-V16 DD-01): a dataclass with a custom `__eq__` got
/// `#[derive(PartialEq)]` AND a dead `__eq__`, so `==` used the structural
/// derive (all fields) — silently overriding the user's semantics. The
/// structural derive is now suppressed and `==` delegates to `__eq__`; this
/// also makes `x in list` use the right equality. Cross-checked vs python3.
#[test]
fn dataclass_custom_eq() {
    let rust = xpile_transpile_to_rust("dataclass_custom_eq.py");
    assert!(
        // no structural PartialEq derive…
        rust.contains("#[derive(Clone, Debug)]")
            // …a delegating impl instead.
            && rust.contains("impl PartialEq for Pt")
            && rust.contains("self.__eq__(__other.clone())"),
        "a custom __eq__ must suppress the derive and delegate via impl PartialEq:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(cmp_eq(), 1);   // (1,2)==(1,9) true (only .x)
    assert_eq!(cmp_ne(), 0);   // (1,2)==(2,2) false
    assert_eq!(in_list(), 1);  // Pt(1,99) in [Pt(1,5), Pt(3,7)] via custom ==
}
"#;
    assert_rustc_runs("dataclass_custom_eq", &rust, driver);
}

/// PMAT-763 (HUNT-V16 #3): a tuple `except (A, B):` emitted a bare `Err(_)`
/// catch-all (no type guard), swallowing ANY exception where Python only
/// catches the listed types. The tuple form now builds the same `xpile: <T>:`
/// re-raise denylist the single-named arm has. Cross-checked vs python3.
#[test]
fn tuple_except_typed() {
    let rust = xpile_transpile_to_rust("tuple_except_typed.py");
    assert!(
        // the tuple-except now discriminates (re-raises ZeroDivisionError, not in
        // the (KeyError, ValueError) set) instead of a bare catch-all.
        rust.contains("starts_with(\"xpile: ZeroDivisionError: \")")
            && !rust.contains("Ok(__xpile_try) => __xpile_try, Err(_) =>"),
        "tuple-except must re-raise an unlisted exception, not catch-all:\n{rust}"
    );
    let driver = r#"
fn main() {
    // ZeroDivisionError not in (KeyError, ValueError) → propagates (panics).
    let w = std::panic::catch_unwind(|| wrong_tuple());
    assert!(w.is_err(), "ZeroDivisionError must propagate past except (KeyError, ValueError)");
    // ValueError is listed → caught.
    assert_eq!(right_tuple(), -7);
}
"#;
    assert_rustc_runs("tuple_except_typed", &rust, driver);
}

/// PMAT-764 (HUNT-V16 #4): a non-negative LITERAL list index OOB (`data[10]`)
/// panicked with Rust's native message, not `xpile: IndexError:`, so inside a
/// try it was silently swallowed by an unrelated typed except. The literal-index
/// path now bounds-checks with the tagged panic. Cross-checked vs python3.
#[test]
fn const_index_indexerror() {
    let rust = xpile_transpile_to_rust("const_index_indexerror.py");
    assert!(
        // literal index now tagged (no bare `data[10i64 as usize].clone()`).
        rust.contains("panic!(\"xpile: IndexError: list index out of range\")")
            && !rust.contains("data[10i64 as usize].clone()"),
        "a literal-index OOB must panic with the xpile: IndexError tag:\n{rust}"
    );
    let driver = r#"
fn main() {
    // IndexError not caught by `except KeyError` → propagates (panics).
    let w = std::panic::catch_unwind(|| wrong_except());
    assert!(w.is_err(), "IndexError must propagate past except KeyError");
    assert_eq!(right_except(), -2);   // except IndexError catches it
    assert_eq!(in_bounds(), 20);      // in-bounds literal index unchanged
}
"#;
    assert_rustc_runs("const_index_indexerror", &rust, driver);
}
