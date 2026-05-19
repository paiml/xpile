//! Runtime stratum fixtures (PMAT-267).
//!
//! Closes the "Run=1 demo fixture" caveat (audit-design.md §4) for one
//! Layer-1 contract at a time. Each fixture in this file is a real
//! Runtime-stratum oracle vote: it compiles the transpiled Rust output
//! through `rustc` and *executes* it against a property-style sweep,
//! asserting behavioral equivalence between Python source semantics
//! and the emitted Rust.
//!
//! Mechanism: a tiny deterministic LCG produces input pairs inside the
//! emitted binary itself, so one rustc invocation amortizes the cost
//! of thousands of runtime checks. The LCG seed is fixed so failures
//! are reproducible; CI sees the same trace every run.
//!
//! ## What this is the first of
//!
//! Per the xpile-spec.md §29 status and the audit-design.md §4 caveat,
//! every existing contract reached §14.4 N-of-M QUORUM at *Bronze
//! tier* (Lean refinement theorem + Kani BMC harness — Sem + Sym
//! strata) but no contract had a real Runtime stratum vote. This file
//! ships that vote for `py-int-arith-v1`, making it the FIRST contract
//! with full §14.4 Sem + Sym + Run coverage. Future PRs extend the
//! same pattern to other Layer-1 / Layer-2 contracts.
//!
//! ## Why a single in-binary sweep, not 1000 separate test cases
//!
//! Two reasons:
//! 1. **Speed.** rustc takes ~1s per invocation; 1000 inputs as
//!    separate `#[test]`s would be 1000s, impractical at workspace
//!    test time. One in-binary loop = one rustc + ~ms of native
//!    execution.
//! 2. **Provenance.** The fixture as-emitted is exactly the binary
//!    users would ship; testing it as a unit is closer to the actual
//!    deployment configuration.

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run_xpile(args: &[&str]) -> std::process::Output {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_xpile"));
    Command::new(bin)
        .args(args)
        .output()
        .expect("spawn xpile binary")
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

fn rust_target_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("xpile-runtime-strata").join(name);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Compile + execute the merged source. Returns `Ok` if the binary
/// exits 0, `Err(stderr+stdout)` otherwise.
fn build_and_run(name: &str, merged: &str) -> Result<(), String> {
    if Command::new("rustc").arg("--version").output().is_err() {
        // Skipping on CI runners without rustc is fine — the test
        // body is a no-op rather than a panic. CI gates that DO have
        // rustc will still catch regressions.
        return Ok(());
    }
    let dir = rust_target_dir(name);
    let file = dir.join(format!("{name}.rs"));
    std::fs::write(&file, merged).expect("write merged rust");
    let bin = dir.join(name);
    let compile = Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(&bin)
        .arg(&file)
        .output()
        .expect("spawn rustc");
    if !compile.status.success() {
        return Err(format!(
            "rustc failed:\n=== source ===\n{merged}\n=== stderr ===\n{}",
            String::from_utf8_lossy(&compile.stderr)
        ));
    }
    let run = Command::new(&bin).output().expect("spawn binary");
    if !run.status.success() {
        return Err(format!(
            "binary {name} exited non-zero (assertion tripped):\n=== source ===\n{merged}\n=== stdout ===\n{}\n=== stderr ===\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr),
        ));
    }
    Ok(())
}

/// PMAT-267 — FIRST contract Runtime stratum fixture.
///
/// Asserts the emitted Rust `add(a, b)` matches Python integer
/// arithmetic semantics under `i64::checked_add` across a 4096-pair
/// LCG-generated sweep. Each pair is classified by whether `checked_add`
/// returns `Some` or `None`:
///
/// * **Some path (no overflow):** transpiled `add` must equal
///   `a.wrapping_add(b)` (which equals `a + b` in the non-overflow
///   case). The transpiled code uses `checked_add(...).expect(...)`
///   per PMAT-002, so success means it didn't panic.
/// * **None path (overflow):** the transpiled `add` MUST panic. This
///   is the runtime equivalent of the C-PY-INT-ARITH precondition that
///   overflow halts rather than wrapping silently.
///
/// Both paths are exercised by spawning the binary twice (once per
/// path) and comparing observed behavior.
///
/// Closes the audit-design.md §4 "Run=1 demo fixture" caveat for
/// C-PY-INT-ARITH. Sets the precedent for Runtime stratum fixtures on
/// `xlate-py-list-to-vec-v1`, `xlate-rust-fn-to-lean-thm-v1`, and other
/// Layer-1 contracts.
#[test]
fn py_int_arith_runtime_stratum_add_matches_python_semantics() {
    let transpiled = xpile_transpile_to_rust("add.py");

    // Happy path: 4096 LCG-generated (a, b) pairs where checked_add
    // succeeds. The driver computes `add(a, b)` and asserts it equals
    // `a.wrapping_add(b)`. We *cannot* use `a + b` directly because
    // that would panic on debug overflow — we only feed it pairs that
    // can't overflow. The reference is `wrapping_add` to avoid that
    // concern entirely.
    let driver = r#"
fn main() {
    // Linear congruential generator — same Numerical Recipes
    // constants Rust's old `rand` test code used. Period 2^64; for
    // 4096 samples we won't approach it. Seed fixed for reproducibility.
    //
    // We shift each LCG output right by 2 (sign-extending) before
    // casting to i64 so the pair (a, b) has |a|+|b| ≤ i64::MAX/2.
    // That makes overflow impossible — every pair is on the happy
    // path and the sweep tests the full 4096 cases. The overflow arm
    // is exercised by the companion test.
    let mut state: u64 = 0xdead_beef_cafe_f00d;
    for i in 0..4096u64 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let a: i64 = (state as i64) >> 2;
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let b: i64 = (state as i64) >> 2;
        let expected = a.checked_add(b)
            .expect("sweep generator should not produce overflowing pairs");
        let got = add(a, b);
        assert_eq!(got, expected, "iter {i}: add({a}, {b}) gave {got}, expected {expected}");
    }
    println!("ok happy path: 4096/4096");
}
"#;

    let merged = format!("{transpiled}\n\n{driver}\n");
    build_and_run("py_int_arith_runtime_happy", &merged)
        .expect("runtime stratum: add(a, b) must match checked_add semantics across the sweep");
}

/// PMAT-268 — Runtime-stratum sweep for branching + unary negation
/// (abs_val).
///
/// `abs_val.py` lowers to a Rust function that exercises `if/else`
/// control flow + unary `-`. The sweep runs 4096 LCG-generated inputs
/// (right-shifted by 1 so negation can't overflow) through the
/// transpiled function and verifies the result matches the
/// hand-written reference. The `i64::MIN.wrapping_abs() == i64::MIN`
/// edge case is intentionally excluded by the shift — the contract
/// for that case is unspecified at v0.1.0.
#[test]
fn py_int_arith_runtime_stratum_abs_val_matches_sign_branch() {
    let transpiled = xpile_transpile_to_rust("abs_val.py");

    let driver = r#"
fn main() {
    let mut state: u64 = 0xdead_beef_cafe_f00d;
    for i in 0..4096u64 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let x: i64 = (state as i64) >> 1;
        let expected: i64 = if x < 0 { -x } else { x };
        let got = abs_val(x);
        assert_eq!(got, expected, "iter {i}: abs_val({x}) gave {got}, expected {expected}");
    }
    println!("ok abs_val sweep: 4096/4096");
}
"#;

    let merged = format!("{transpiled}\n\n{driver}\n");
    build_and_run("py_int_arith_runtime_abs_val", &merged)
        .expect("runtime stratum: abs_val(x) must match Python sign-branching semantics");
}

/// PMAT-268 — Runtime-stratum sweep for binary recursion (fib).
///
/// `fib.py` lowers to a recursive Rust function with TWO recursive
/// calls per invocation. The sweep computes the first 24 Fibonacci
/// numbers (small enough that exponential recursion is tractable but
/// well past the boundary cases) and asserts each matches an
/// iteratively-computed reference. Verifies recursion + branch +
/// addition end-to-end.
#[test]
fn py_int_arith_runtime_stratum_fib_matches_iterative_reference() {
    let transpiled = xpile_transpile_to_rust("fib.py");

    let driver = r#"
fn fib_iter(n: i64) -> i64 {
    if n <= 1 { return n; }
    let (mut a, mut b) = (0i64, 1i64);
    for _ in 1..n { let t = b; b = a + b; a = t; }
    b
}

fn main() {
    for n in 0..24i64 {
        let expected = fib_iter(n);
        let got = fib(n);
        assert_eq!(got, expected, "fib({n}) gave {got}, expected {expected}");
    }
    println!("ok fib recursion: 24/24");
}
"#;

    let merged = format!("{transpiled}\n\n{driver}\n");
    build_and_run("py_int_arith_runtime_fib", &merged)
        .expect("runtime stratum: fib(n) must match iterative reference for n in 0..24");
}

/// PMAT-268 — Runtime-stratum sweep for modulo + tail recursion (gcd).
///
/// `gcd.py` exercises `%` (modulo) plus structural recursion. The
/// sweep generates 1024 LCG pairs of positive i64s clamped to
/// `[1, i64::MAX/4]` (so recursion depth stays bounded and modulo
/// is well-defined) and compares against an iterative Euclidean GCD.
/// GCD on negatives is out-of-scope — Python and Rust `%` semantics
/// differ on sign.
#[test]
fn py_int_arith_runtime_stratum_gcd_matches_euclidean_reference() {
    let transpiled = xpile_transpile_to_rust("gcd.py");

    let driver = r#"
fn gcd_iter(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn main() {
    let mut state: u64 = 0xdead_beef_cafe_f00d;
    for i in 0..1024u64 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let a: i64 = ((state >> 2) as i64).abs().max(1).min(i64::MAX / 4);
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let b: i64 = ((state >> 2) as i64).abs().max(1).min(i64::MAX / 4);
        let expected = gcd_iter(a, b);
        let got = gcd(a, b);
        assert_eq!(got, expected, "iter {i}: gcd({a}, {b}) gave {got}, expected {expected}");
    }
    println!("ok gcd sweep: 1024/1024");
}
"#;

    let merged = format!("{transpiled}\n\n{driver}\n");
    build_and_run("py_int_arith_runtime_gcd", &merged)
        .expect("runtime stratum: gcd(a, b) must match iterative Euclidean reference");
}

/// Companion test — verifies the OVERFLOW arm of the C-PY-INT-ARITH
/// contract. Per PMAT-002, the transpiled code uses
/// `checked_add(...).expect(...)` so adding `i64::MAX + 1` must panic.
/// The driver `main` invokes the function unconditionally on a known
/// overflowing pair, asserts the binary exits non-zero, and the
/// `build_and_run` helper inverts the expected exit status.
#[test]
fn py_int_arith_runtime_stratum_overflow_panics() {
    let transpiled = xpile_transpile_to_rust("add.py");

    // Driver invokes add(i64::MAX, 1) which MUST overflow. Expected
    // behavior: process aborts with a panic. The build_and_run helper
    // treats non-zero exit as failure, so we invert by wrapping the
    // call in `std::panic::catch_unwind` and exit 0 IFF the inner call
    // panicked (which is the contract'd behavior).
    let driver = r#"
fn main() {
    let result = std::panic::catch_unwind(|| {
        add(i64::MAX, 1)
    });
    match result {
        Err(_) => {
            // Expected: checked_add overflowed and `.expect(...)` panicked.
            println!("ok overflow panicked");
            std::process::exit(0);
        }
        Ok(v) => {
            eprintln!("contract violated: add(i64::MAX, 1) returned {v}, expected panic");
            std::process::exit(1);
        }
    }
}
"#;

    let merged = format!("{transpiled}\n\n{driver}\n");
    build_and_run("py_int_arith_runtime_overflow", &merged)
        .expect("runtime stratum: add(i64::MAX, 1) must panic per checked_add semantics");
}
