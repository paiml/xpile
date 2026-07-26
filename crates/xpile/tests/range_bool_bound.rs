//! PMAT-1364: a `bool` in a `range(...)` bound position COERCES to `int`
//! instead of emitting Rust that rustc rejects.
//!
//! Python's `bool` is a subclass of `int` — `True == 1`, `False == 0` — so
//! `range(True)` is `range(1)` and `range(False)` is empty. The desugar passed
//! the lowered bound to the emitter RAW, so every bool-typed bound produced
//! uncompilable Rust while `xpile transpile` exited 0:
//!
//! ```text
//! let __forstop1: i64 = b;                      // E0308: expected i64, found bool
//! let __forstop1: i64 = true;                   // E0308  (`range(True)`)
//! let __forstop1: i64 = (x > 2i64);             // E0308  (`range(x > 2)`)
//! let mut __forc0: i64 = b;                     // E0308  (`range(b, 3)`)
//! let mut __forc0: i64 = (b).checked_sub(1i64); // E0599: no `checked_sub` on bool
//! ```
//!
//! Measured on the fixture that is now `tests/oracle_fixtures/range_bool_bound.py`:
//! the pre-fix emitter exited 0 and rustc then reported **8 errors** (6 × E0308,
//! 2 × E0599). Accept-then-fail-rustc is the disposition this sprint exists to
//! remove — it is strictly worse than refusing, because the user pays a whole
//! backend round-trip to rediscover what the frontend already knew.
//!
//! The fix routes both bounds through `to_i64_operand`, the SAME helper every
//! other int-position consumer already uses. It re-infers, so it is a no-op on a
//! non-bool bound — `non_bool_bounds_are_emitted_unchanged` below is what makes
//! that claim testable rather than asserted. Without it a blanket unconditional
//! cast would pass every other test in this file.
//!
//! BigInt-mode functions REFUSE instead. There the counter type is
//! `xpile_bigint::BigInt` and `to_i64_operand` only reaches i64, so the cast
//! would swap one E0308 for another; no bool → BigInt promotion node exists.
//! (`-> BigInt` promotes `int` params to `BigInt` but deliberately leaves `bool`
//! params alone, which is exactly why bool is the only bound shape that can
//! reach the desugar mistyped.)
//!
//! The byte-for-byte agreement with CPython over all seven bound positions lives
//! in `tests/oracle_fixtures/range_bool_bound.py`, which the differential oracle
//! compiles with rustc and diffs against `python3`.

use depyler_frontend::PythonFrontend;
use std::path::Path;
use std::process::Command;
use xpile_frontend::{Frontend, FrontendError};

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn lower(src: &str) -> Result<xpile_meta_hir::Module, FrontendError> {
    PythonFrontend.parse_and_lower(Path::new("t.py"), src)
}

/// Transpile `src` to Rust through the SHIPPED binary (the same
/// frontend + backend dispatch the CLI performs), returning its stdout.
/// Panics with the emitter's own stderr on a non-zero exit.
fn emit_rust(label: &str, src: &str) -> String {
    let dir = std::env::temp_dir()
        .join("xpile-range-bool")
        .join(label.replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let py = dir.join("t.py");
    std::fs::write(&py, src).expect("write fixture");
    let out = Command::new(xpile_bin())
        .args(["transpile", py.to_str().unwrap(), "--target", "rust"])
        .output()
        .expect("spawn xpile");
    assert!(
        out.status.success(),
        "{label}: transpile must succeed, got {}:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8(out.stdout).expect("utf8")
}

/// Every bound position that can hold a bool, with the EXACT binding line the
/// fixed emitter must produce. Pinning the whole line (not just `as i64`) is
/// what makes these non-vacuous: a cast landing on the wrong operand, or on the
/// counter instead of the stop temp, would still contain the substring.
const COERCED: &[(&str, &str, &str)] = &[
    (
        "stop: bool param",
        "def f(b: bool) -> int:\n    t: int = 0\n    for i in range(b):\n        t = t + 1\n    return t\n",
        "let __forstop1: i64 = ((b) as i64);",
    ),
    (
        "stop: bool literal",
        "def f() -> int:\n    t: int = 0\n    for i in range(True):\n        t = t + 1\n    return t\n",
        "let __forstop1: i64 = ((true) as i64);",
    ),
    (
        "stop: comparison expression",
        "def f(x: int) -> int:\n    t: int = 0\n    for i in range(x > 2):\n        t = t + 1\n    return t\n",
        "let __forstop1: i64 = (((x > 2i64)) as i64);",
    ),
    (
        "start: bool param",
        "def f(b: bool) -> int:\n    t: int = 0\n    for i in range(b, 3):\n        t = t + i\n    return t\n",
        "let mut __forc0: i64 = ((b) as i64);",
    ),
    (
        // The `reversed` flip rewrites each bound to `<bound> - 1`, and a
        // `BinOp::Sub` infers as I64 whatever its operands are. So a coercion
        // applied AFTER the flip would silently skip these two shapes while
        // every non-reversed test above still passed. That ordering hazard is
        // the reason both reversed positions are pinned separately.
        "reversed: bool stop becomes the start",
        "def f(b: bool) -> int:\n    t: int = 0\n    for i in reversed(range(b)):\n        t = t + 1\n    return t\n",
        "let mut __forc0: i64 = (((b) as i64)).checked_sub(1i64)",
    ),
    (
        "reversed: bool start becomes the stop",
        "def f(b: bool) -> int:\n    t: int = 0\n    for i in reversed(range(b, 4)):\n        t = t + i\n    return t\n",
        "let __forstop1: i64 = (((b) as i64)).checked_sub(1i64)",
    ),
];

#[test]
fn bool_range_bounds_coerce_to_i64() {
    for (label, src, expected_line) in COERCED {
        let rust = emit_rust(label, src);
        assert!(
            rust.contains(expected_line),
            "{label}: expected the bound to be cast to i64 —\n  want: {expected_line}\n\
             got:\n{rust}"
        );
    }
}

/// The conservatism boundary, and the anti-vacuity half of this file: the
/// coercion must be CONDITIONAL on the bound actually being a bool. An
/// unconditional `as i64` wrap would green every assertion above while churning
/// the emitted form of every `range(...)` in the corpus — so pin that ordinary
/// int bounds come out byte-identical to the pre-slice emitter, with no cast
/// inserted anywhere in the loop header.
#[test]
fn non_bool_bounds_are_emitted_unchanged() {
    let unchanged: &[(&str, &str, &[&str])] = &[
        (
            "int param stop",
            "def f(n: int) -> int:\n    t: int = 0\n    for i in range(n):\n        t = t + 1\n    return t\n",
            &["let __forstop1: i64 = n;"],
        ),
        (
            "int literal stop stays inline (no temp at all)",
            "def f() -> int:\n    t: int = 0\n    for i in range(5):\n        t = t + 1\n    return t\n",
            &["while (__forc0 < 5i64)"],
        ),
        (
            "int param start and stop",
            "def f(a: int, b: int) -> int:\n    t: int = 0\n    for i in range(a, b):\n        t = t + i\n    return t\n",
            &["let mut __forc0: i64 = a;", "let __forstop1: i64 = b;"],
        ),
        (
            "len() stop",
            "def f(xs: list[int]) -> int:\n    t: int = 0\n    for i in range(len(xs)):\n        t = t + xs[i]\n    return t\n",
            &["let __forstop1: i64 = (xs.len() as i64);"],
        ),
    ];
    for (label, src, wants) in unchanged {
        let rust = emit_rust(label, src);
        for want in *wants {
            assert!(
                rust.contains(want),
                "{label}: a non-bool bound must be emitted unchanged —\n  want: {want}\n\
                 got:\n{rust}"
            );
        }
        // No bool→i64 NumCast may appear in the loop header of a program that
        // has no bool anywhere. This is the assertion a blanket cast fails.
        //
        // The fingerprint is the NumCast wrapper specifically — `= ((<value>)
        // as i64)`, double-parenthesised — NOT any `as i64`. The `len()` bound
        // legitimately emits `= (xs.len() as i64)` (a pre-existing usize→i64
        // cast, single-parenthesised), and a blanket wrap of it would read
        // `= (((xs.len() as i64)) as i64)`, which this still catches.
        for line in rust.lines() {
            let l = line.trim();
            if l.starts_with("let __forstop") || l.starts_with("let mut __forc") {
                assert!(
                    !(l.contains("= ((") && l.contains(") as i64)")),
                    "{label}: a non-bool bound must NOT be wrapped in a bool→i64 \
                     NumCast; got: {l}"
                );
            }
        }
    }
}

/// RED: a bool bound in a BigInt-mode function refuses, stage-pinned, naming
/// the position. Coercing there would swap E0308-on-bool for E0308-on-i64
/// (`let __forstopN: xpile_bigint::BigInt = ((b) as i64)`), so a refusal is the
/// only honest disposition until a bool → BigInt promotion exists.
#[test]
fn bool_range_bound_in_bigint_mode_refuses() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "BigInt-mode stop bound",
            "def f(b: bool, n: BigInt) -> BigInt:\n    t: BigInt = n\n    for i in range(b):\n        t = t + n\n    return t\n",
            "stop",
        ),
        (
            "BigInt-mode start bound",
            "def f(b: bool, n: BigInt) -> BigInt:\n    t: BigInt = n\n    for i in range(b, 4):\n        t = t + n\n    return t\n",
            "start",
        ),
    ];
    for (label, src, position) in cases {
        let err = match lower(src) {
            Err(e) => e,
            Ok(_) => panic!(
                "{label}: lowered without complaint — the BigInt counter type makes \
                 this shape uncompilable, so accepting it is an accept-then-fail"
            ),
        };
        // Pin the STAGE, not just `Err(_)`: a `Parse` error would mean the
        // fixture is malformed and the test would green for the wrong reason.
        assert!(
            matches!(err, FrontendError::Lower(_)),
            "{label}: expected a LOWERING refusal, got {err:?}"
        );
        let msg = err.to_string();
        for needle in ["bool", "BigInt-mode", "range(...)", position] {
            assert!(
                msg.contains(needle),
                "{label}: the refusal must name `{needle}`; got:\n{msg}"
            );
        }
    }
}

/// The BigInt refusal is about the BOOL bound specifically. The same functions
/// with an `int`-annotated bound must still lower — otherwise the RED test
/// above would pass merely because BigInt-mode range loops are broken in
/// general.
#[test]
fn bigint_mode_range_loops_with_int_bounds_still_lower() {
    let ok = [
        "def f(k: int, n: BigInt) -> BigInt:\n    t: BigInt = n\n    for i in range(k):\n        t = t + n\n    return t\n",
        "def f(k: int, n: BigInt) -> BigInt:\n    t: BigInt = n\n    for i in range(k, 4):\n        t = t + n\n    return t\n",
    ];
    for src in ok {
        assert!(
            lower(src).is_ok(),
            "a BigInt-mode range loop over an int bound must still lower:\n{src}"
        );
    }
}

/// The STEP is parsed as an integer LITERAL before any of this runs, so it has
/// no bool shape to coerce. Pin that `range(0, 6, True)` still takes the
/// pre-existing non-literal-step refusal rather than silently stepping by 1 —
/// a widening of the step path would be a NEW silent wrong answer, which is
/// exactly what this slice is removing elsewhere.
#[test]
fn bool_step_still_refuses_as_a_non_literal_step() {
    let src = "def f() -> int:\n    t: int = 0\n    for i in range(0, 6, True):\n        t = t + i\n    return t\n";
    let err = lower(src).expect_err("a bool step must not be accepted");
    assert!(
        matches!(err, FrontendError::Lower(_)),
        "expected a LOWERING refusal, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("non-literal-int or zero step"),
        "the bool step must take the pre-existing step refusal, not a new path; got:\n{msg}"
    );
}
