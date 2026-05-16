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
    assert!(stdout.contains("pub fn factorial(n: i64) -> i64"));
    assert!(stdout.contains("if (n <= 1i64) { 1i64 } else"));
    // Recursive call back into factorial — proves Expr::Call works for self-reference.
    assert!(stdout.contains("factorial("));
}

/// Semantic round-trip: emit factorial → compile → run → assert results.
/// Proves not just that the output type-checks, but that it computes
/// the right values (0!=1, 1!=1, 2!=2, 3!=6, 5!=120, 6!=720).
#[test]
fn factorial_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("factorial.py");
    let driver = r#"
fn main() {
    assert_eq!(factorial(0), 1);
    assert_eq!(factorial(1), 1);
    assert_eq!(factorial(2), 2);
    assert_eq!(factorial(3), 6);
    assert_eq!(factorial(5), 120);
    assert_eq!(factorial(6), 720);
    assert_eq!(factorial(10), 3628800);
}
"#;
    assert_rustc_runs("factorial", &rust, driver);
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

/// PMAT-008: validates negative-step `range(...)`. `factorial_iter(n)`
/// computes n! by counting down: `for i in range(n, 0, -1): acc *= i`.
/// The lowering must flip the cond from `<` to `>` and emit
/// `checked_add(-1i64)` for the tail.
#[test]
fn for_range_negative_step_emitted_rust_computes_correct_values() {
    let rust = xpile_transpile_to_rust("countdown.py");
    assert!(
        rust.contains("while (i > 0i64)"),
        "expected `i > 0` cond for negative step, got:\n{rust}"
    );
    assert!(
        rust.contains("i = (i).checked_add(-1i64)"),
        "expected negative-step tail, got:\n{rust}"
    );
    let driver = r#"
fn main() {
    assert_eq!(factorial_iter(0), 1);  // empty product
    assert_eq!(factorial_iter(1), 1);  // 1
    assert_eq!(factorial_iter(5), 120);
    assert_eq!(factorial_iter(10), 3628800);
}
"#;
    assert_rustc_runs("countdown", &rust, driver);
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
