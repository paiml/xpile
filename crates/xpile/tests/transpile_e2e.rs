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
