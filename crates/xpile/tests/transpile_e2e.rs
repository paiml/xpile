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
    assert!(
        !rust.contains("checked_") && rust.contains("/ 2f64"),
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
    // Post PMAT-002: Python `%` lowers to checked_rem_euclid (Euclidean
    // semantics matching Python, plus overflow check).
    assert!(stdout.contains("(n).checked_rem_euclid(2i64)"));
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
        rust.contains("table[&(key)].clone()"),
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
        rust.contains("d[&(k)].clone()"),
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
        rust.contains("format!(\"{:.2}\", x)")
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
            && rust.contains(".trim().to_string()")
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

/// PMAT-496 (sprint): bounded slicing `xs[a:b]`. List → `Vec` via
/// `.to_vec()`, str → `String` via `.to_string()` (byte-indexed).
#[test]
fn slicing_emitted_rust_list_and_str() {
    let rust = xpile_transpile_to_rust("slicing.py");
    assert!(
        rust.contains(" as usize..(")
            && rust.contains(".to_vec()")
            && rust.contains(".to_string()"),
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
        rust.contains("sort_by_key(|__k| { let w = __k.clone(); w.len() as i64 }"),
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

/// PMAT-502e (Tranche 2): 1-arg `min(xs)`/`max(xs)` over an int list →
/// `xs.iter().copied().min().unwrap()` (or `.max()`).
#[test]
fn list_minmax_builtin() {
    let rust = xpile_transpile_to_rust("list_minmax_builtin.py");
    assert!(
        rust.contains(".iter().copied().min().unwrap()")
            && rust.contains(".iter().copied().max().unwrap()"),
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
        rust.contains(".iter().cloned().max_by_key(|__k| { let w = __k.clone(); w.len() as i64 })")
            && rust.contains(".iter().cloned().min_by_key("),
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

/// PMAT-502k (Tranche 2): sequence repetition `seq * n` / `n * seq` →
/// `(seq).repeat(((n).max(0)) as usize)` — str → String, list → Vec.
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
        rust.contains(".trim_start().to_string()")
            && rust.contains(".trim_end().to_string()")
            && rust.contains(".map(|__i| __i as i64).unwrap_or(-1)")
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

/// PMAT-502ah (Tranche 2): `s.capitalize()` → first char upper, rest lower
/// (empty → ""), matching Python.
#[test]
fn str_capitalize() {
    let rust = xpile_transpile_to_rust("str_capitalize.py");
    assert!(
        rust.contains("__f.to_uppercase().collect::<String>()"),
        "expected capitalize block, got:\n{rust}"
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

/// PMAT-502m (Tranche 2): numeric conversions `int(x)` / `float(x)` →
/// `((x) as i64)` (truncate toward zero) / `((x) as f64)`.
#[test]
fn num_cast_int_float() {
    let rust = xpile_transpile_to_rust("num_cast.py");
    assert!(
        rust.contains(") as f64)") && rust.contains(") as i64)"),
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
        rust.contains(".round_ties_even() as i64)"),
        "expected round_ties_even emission, got:\n{rust}"
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
    assert!(
        rust.contains("checked_div_euclid") && rust.contains("checked_rem_euclid"),
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
    assert!(
        rust.contains("((lo <= x) && (x <= hi))"),
        "expected chained-comparison conjunction, got:\n{rust}"
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

/// PMAT-502s (Tranche 2): negative list index `xs[-k]` → `xs[len(xs) - k]`
/// (Python from-the-end indexing).
#[test]
fn neg_index() {
    let rust = xpile_transpile_to_rust("neg_index.py");
    assert!(
        rust.contains("xs.len() as i64).checked_sub("),
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
    assert!(
        rust.contains("xs[..(n) as usize].to_vec()")
            && rust.contains("xs[(n) as usize..].to_vec()")
            && rust.contains("xs[..].to_vec()"),
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
    assert!(
        rust.contains(".fold(f64::INFINITY, f64::min)")
            && rust.contains(".fold(f64::NEG_INFINITY, f64::max)"),
        "expected float min/max fold emission, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(lowest(vec![3.5, 1.5, 2.5]), 1.5);
    assert_eq!(highest(vec![3.5, 1.5, 2.5]), 3.5);
}
"#;
    assert_rustc_runs("list_minmax_float", &rust, driver);
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
        rust.contains("xs.sort_by(|a, b| a.partial_cmp(b).unwrap());"),
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

/// PMAT-502ar (Tranche 2): positional list insertion `xs.insert(i, x)` →
/// `xs.insert((i) as usize, x);`.
#[test]
fn list_insert() {
    let rust = xpile_transpile_to_rust("list_insert.py");
    assert!(
        rust.contains("xs.insert((1i64) as usize, x);"),
        "insert(var):\n{rust}"
    );
    assert!(
        rust.contains("xs.insert((0i64) as usize, 99i64);"),
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

/// PMAT-502as (Tranche 2): list pop (expression form) `xs.pop()` →
/// `(xs).pop().unwrap()` and `xs.pop(i)` → `(xs).remove((i) as usize)`.
/// Covers a param receiver, a local receiver (mutability pre-pass), and
/// pop inside arithmetic.
#[test]
fn list_pop() {
    let rust = xpile_transpile_to_rust("list_pop.py");
    assert!(rust.contains("(xs).pop().unwrap()"), "pop last:\n{rust}");
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

/// PMAT-502at (Tranche 2): item deletion `del coll[key]` — list →
/// `coll.remove((k) as usize);`, dict → `coll.remove(&(k));`.
#[test]
fn del_item() {
    let rust = xpile_transpile_to_rust("del_item.py");
    assert!(
        rust.contains("xs.remove((i) as usize);"),
        "list del (var):\n{rust}"
    );
    assert!(
        rust.contains("xs.remove((0i64) as usize);"),
        "list del (literal):\n{rust}"
    );
    assert!(rust.contains("d.remove(&(k));"), "dict del:\n{rust}");
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

/// PMAT-502au (Tranche 2): dict pop (expression form) `d.pop(k)` →
/// `(d).remove(&(k)).unwrap()` and `d.pop(k, default)` →
/// `(d).remove(&(k)).unwrap_or(default)`. Covers param + local receivers.
#[test]
fn dict_pop() {
    let rust = xpile_transpile_to_rust("dict_pop.py");
    assert!(
        rust.contains("(d).remove(&(k)).unwrap()"),
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

/// PMAT-502aw (Tranche 2): str padding `s.rjust(w)` →
/// `format!("{:>1$}", s, (w) as usize)` and `s.ljust(w)` →
/// `format!("{:<1$}", s, (w) as usize)`. Rust format width is a minimum,
/// so a longer string is returned unchanged (matching Python).
#[test]
fn str_just() {
    let rust = xpile_transpile_to_rust("str_just.py");
    assert!(
        rust.contains("format!(\"{:>1$}\", s, (w) as usize)"),
        "rjust:\n{rust}"
    );
    assert!(
        rust.contains("format!(\"{:<1$}\", s, (w) as usize)"),
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
    // Range comp desugars to a counter while-loop, not a ForEach.
    assert!(
        rust.contains("let mut x: i64 = 0i64;") && rust.contains("while (x < n) {"),
        "range counter loop:\n{rust}"
    );
    assert!(
        rust.contains("let mut x: i64 = 1i64;"),
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
    assert!(
        rust.contains("xs[..].iter().step_by(2).cloned().collect::<Vec<_>>()"),
        "xs[::2]:\n{rust}"
    );
    assert!(
        rust.contains(
            "xs[(1i64) as usize..(8i64) as usize].iter().step_by(3).cloned().collect::<Vec<_>>()"
        ),
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
    // Range comp desugars to a counter while-loop, not a ForEach.
    assert!(
        rust.contains("let mut x: i64 = 0i64;") && rust.contains("while (x < n) {"),
        "range counter loop:\n{rust}"
    );
    assert!(
        rust.contains("let mut x: i64 = 2i64;"),
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
        rust.contains("(s.len() as i64 != 0i64)"),
        "str cast:\n{rust}"
    );
    assert!(
        rust.contains("(xs.len() as i64 != 0i64)"),
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
    assert!(
        rust.contains("(s).trim().parse::<i64>().expect("),
        "int(s):\n{rust}"
    );
    assert!(
        rust.contains("(s).trim().parse::<f64>().expect("),
        "float(s):\n{rust}"
    );
    assert!(
        rust.contains("((x) as i64)"),
        "numeric still as-cast:\n{rust}"
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

/// PMAT-502bi (Tranche 2): `s.index(sub)` → byte index of the first
/// match, panicking (ValueError) when absent (like `.find` but no `-1`).
#[test]
fn str_index() {
    let rust = xpile_transpile_to_rust("str_index.py");
    assert!(
        rust.contains(".find(&(String::from(\"b\"))[..]).map(|__i| __i as i64).expect(")
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
/// → `0.0 - x` (a `FloatBinOp`, not the i64-only `checked_neg`).
#[test]
fn neg_float_var() {
    let rust = xpile_transpile_to_rust("neg_float_var.py");
    assert!(rust.contains("(0f64 - x)"), "float-var negation:\n{rust}");
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

/// PMAT-502bq (Tranche 2): augmented assignment over a float `x += y`
/// (and `-= *= /=`) → plain infix `FloatBinOp`, not the i64-only
/// `checked_*` path. `/=` (true division) is supported in aug position.
#[test]
fn aug_assign_float() {
    let rust = xpile_transpile_to_rust("aug_assign_float.py");
    assert!(rust.contains("x = (x + y)"), "float += :\n{rust}");
    assert!(rust.contains("x = (x / 4f64)"), "float /= :\n{rust}");
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

/// PMAT-502br (Tranche 2): float floor-division `a // b` → `(a / b).floor()`
/// and modulo `a % b` → `a - b * (a / b).floor()` (Python floor semantics,
/// result of `%` follows the divisor's sign — verified for mixed signs).
#[test]
fn float_floordiv_mod() {
    let rust = xpile_transpile_to_rust("float_floordiv_mod.py");
    assert!(rust.contains("(a / b).floor()"), "float // :\n{rust}");
    assert!(
        rust.contains("(a - b * (a / b).floor())"),
        "float % :\n{rust}"
    );
    let driver = r#"
fn main() {
    // python3: 7//2=3, -7//2=-4, 7%3=1, -7%3=2, 7%-3=-2
    assert!((fd(7.0, 2.0) - 3.0).abs() < 1e-9);
    assert!((fd(-7.0, 2.0) - (-4.0)).abs() < 1e-9);
    assert!((fmod(7.0, 3.0) - 1.0).abs() < 1e-9);
    assert!((fmod(-7.0, 3.0) - 2.0).abs() < 1e-9);
    assert!((fmod(7.0, -3.0) - (-2.0)).abs() < 1e-9);
    assert!((wrap(13.0, 5.0) - 3.0).abs() < 1e-9);
}
"#;
    assert_rustc_runs("float_floordiv_mod", &rust, driver);
}

/// PMAT-502bs (Tranche 2): Python 3 true division `/` always yields a float
/// — `int / int` casts both operands to f64 (`7 / 2 == 3.5`); a mixed
/// `float / int` casts only the int side (no `f64 / i64` mismatch).
#[test]
fn true_division() {
    let rust = xpile_transpile_to_rust("true_division.py");
    assert!(
        rust.contains("(((a) as f64) / ((b) as f64))"),
        "int/int true division:\n{rust}"
    );
    assert!(
        rust.contains("(x / ((2i64) as f64))"),
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
    assert!(rust.contains("(x).powf("), "float power:\n{rust}");
    assert!(
        rust.contains("(((n) as f64)).powf(0.5f64)"),
        "int-base float power:\n{rust}"
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
        rust.contains("(base).powf(((3i64) as f64))"),
        "float **= int rhs:\n{rust}"
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
/// offsets the index var (`__i as i64 + start`).
#[test]
fn enumerate_start() {
    let rust = xpile_transpile_to_rust("enumerate_start.py");
    assert!(
        rust.contains("__i as i64 + 1i64") && rust.contains("__i as i64 + 10i64"),
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
    assert!(
        rust.contains(r#"format!("${:.2}", x)"#),
        "float spec:\n{rust}"
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

/// PMAT-502ci (Tranche 2): `for i in reversed(range(...))` — descending range
/// iteration (desugars to a step -1 range).
#[test]
fn reversed_range() {
    let rust = xpile_transpile_to_rust("reversed_range.py");
    // start = n - 1, descending (`i > …`).
    assert!(
        rust.contains("(n).checked_sub(1i64)") && rust.contains("while (i > "),
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
    assert!(
        rust.contains(".chars().next().expect(") && rust.contains("char::from_u32("),
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
#[test]
fn split_whitespace() {
    let rust = xpile_transpile_to_rust("split_whitespace.py");
    assert!(
        rust.contains(".split_whitespace().map(|__c| __c.to_string()).collect::<Vec<String>>()"),
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

/// PMAT-498b (Tranche 2): `sum(xs)` over a numeric list →
/// `xs.iter().sum::<i64>()` / `::<f64>()` (turbofish from the element type).
#[test]
fn sum_builtin_int_and_float() {
    let rust = xpile_transpile_to_rust("sum_builtin.py");
    assert!(
        rust.contains(".iter().sum::<i64>()") && rust.contains(".iter().sum::<f64>()"),
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

/// PMAT-498 (Tranche 2): scalar numeric builtins `abs`/`min`/`max` →
/// `(x).abs()` / `(a).min(b)` / `(a).max(b)`. `clamp` via `min(max(...))`.
#[test]
fn num_builtins_abs_min_max() {
    let rust = xpile_transpile_to_rust("num_builtins.py");
    assert!(
        rust.contains(".abs()") && rust.contains(".min(") && rust.contains(".max("),
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

/// PMAT-008 + PMAT-036: validates negative-step `range(...)` under
/// BigInt mode. `factorial_iter(n)` counts down via `for i in
/// range(n, 0, -1): acc *= i`. The lowering must flip the cond from
/// `<` to `>` AND emit the loop with BigInt-typed `i` (PMAT-036 fix
/// — the for-target's binding type follows the enclosing function's
/// return type now). Tail uses BigInt arithmetic.
#[test]
fn for_range_negative_step_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("countdown.py");
    assert!(
        rust.contains("let mut i: xpile_bigint::BigInt = n"),
        "expected BigInt for-target init (PMAT-036), got:\n{rust}"
    );
    assert!(
        rust.contains("while (i.clone() > xpile_bigint::BigInt::from(0i64))"),
        "expected `i > 0` cond comparing BigInt operands, got:\n{rust}"
    );
    assert!(
        rust.contains("i = (i.clone() + xpile_bigint::BigInt::from(-1i64))"),
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

/// PMAT-007: validates `for i in range(...)` desugaring. The three
/// `range` shapes (stop, start+stop, start+stop+step) all lower to
/// a `let mut i` + `while i < <stop>` + `i = i + <step>` tail.
#[test]
fn for_range_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("for_sum.py");
    assert!(
        rust.contains("let mut i: i64 = 0i64"),
        "expected init at 0:\n{rust}"
    );
    assert!(
        rust.contains("let mut i: i64 = a"),
        "expected init at a:\n{rust}"
    );
    assert!(
        rust.contains("i = (i).checked_add(2i64)"),
        "expected step=2 tail:\n{rust}"
    );
    assert!(
        rust.contains("while (i < n)"),
        "expected while-cond:\n{rust}"
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
    // Free vars: only loop_state, since cond `i > 0` references only i.
    assert!(
        stdout.contains("partial def factorial_iter_loop_0 (acc : Int) (i : Int) : Int :="),
        "expected helper signature (no free vars), got:\n{stdout}"
    );
    assert!(
        stdout.contains("if (i > (0: Int)) then"),
        "expected negative-step cond, got:\n{stdout}"
    );
    assert!(
        stdout.contains("let i := (i + (-1: Int))"),
        "expected negative-step body update, got:\n{stdout}"
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
    assert!(
        rust.contains("checked_shr"),
        "expected checked_shr, got:\n{rust}"
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
