//! PMAT-908 (Sprint Day 9 — north-star Phase 6) — the EXECUTING agent-repair
//! witness. Drives the deterministic [`RepairLoop`] against the *real* toolchain
//! probe and the *real* Day-3 oracle: capture the CPython hybrid reference (the
//! C extension bound via `ctypes`), start from a deliberately ABI-broken shim
//! that `rustc` rejects with `E0308`, and prove the loop applies the two ABI-cast
//! repair rules and converges to a `cc`+`rustc`-built artifact whose stdout is
//! byte-identical to CPython.
//!
//! Gated on `cc` + `rustc` + `python3`; graceful-skips on a constrained runner.

use xpile_agent::{HybridCcRustcProbe, Probe, RepairLoop, RepairOutcome};
use xpile_oracle::{capture_cpython_hybrid_ref, CtypesBinding, PythonOracle};

/// The C side of the `hybrid_sum` fixture (real `x*x`, not identity).
const C_SOURCE: &str = "int square_sum(int x){return x*x;}\n";

/// A shim that DROPS both ABI casts — the real failure class the loop repairs.
/// `rustc` rejects it with two `E0308`s (the `i64` arg vs `c_int`, the `c_int`
/// return vs `i64`).
const BROKEN_SHIM: &str = "unsafe extern \"C\" {\n    fn square_sum(x: ::std::os::raw::c_int) -> ::std::os::raw::c_int;\n}\npub fn square_sum_shim(x: i64) -> i64 {\n    let __r = unsafe { square_sum(x) };\n    __r\n}\nfn main() {\n    println!(\"{}\", square_sum_shim(7));\n}\n";

#[test]
fn repair_loop_converges_to_a_cpython_matching_artifact() {
    if !HybridCcRustcProbe::toolchain_available() || !PythonOracle::available() {
        eprintln!("cc/rustc/python3 unavailable — skipping executing repair-loop witness");
        return;
    }

    // Phase 3 — the Day-3 oracle: CPython runs `app.py`'s real `main()` against
    // the cc-compiled C extension bound via ctypes. Reference = square_sum(7) = 49.
    let py = "from ._core import square_sum\ndef main() -> None:\n    print(square_sum(7))";
    let bindings = vec![CtypesBinding {
        symbol: "square_sum".to_string(),
        argtypes: vec!["c_int"],
        restype: Some("c_int"),
    }];
    let reference = capture_cpython_hybrid_ref(
        py,
        &[("_core.c".to_string(), C_SOURCE.to_string())],
        &bindings,
    )
    .expect("capture CPython hybrid reference");
    assert_eq!(reference, "49", "the C extension genuinely computes x*x");

    // Sanity: the broken shim really does NOT build/match before repair.
    let probe = HybridCcRustcProbe {
        c_source: C_SOURCE.to_string(),
        reference: reference.clone(),
    };
    assert!(
        probe.evaluate(BROKEN_SHIM).is_err(),
        "the ABI-broken shim must fail before repair"
    );

    // Phase 6 — the deterministic repair loop converges to a matching artifact.
    let outcome =
        RepairLoop::ffi_int_boundary(Default::default(), "square_sum").run(&probe, BROKEN_SHIM);

    match outcome {
        RepairOutcome::Repaired { iterations, source } => {
            assert_eq!(iterations, 2, "one arg-cast + one return-cast repair");
            // The repaired artifact really builds, runs, and matches CPython.
            assert!(
                probe.evaluate(&source).is_ok(),
                "repaired artifact matches the CPython reference"
            );
            assert!(source.contains("square_sum(x as ::std::os::raw::c_int)"));
            assert!(source.contains("__r as i64"));
        }
        other => panic!("expected Repaired, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PMAT-933 — the FFI-FLOAT-TYPE-FLOW (whole-float repr) executing repair witness.
//
// PMAT-931 found that a `double`-returning FFI boundary, printed as Rust's plain
// `println!("{}", x)`, emits `10` for a whole `f64`, while CPython-via-ctypes
// (`restype = c_double`) prints Python's `10.0`. Crucially `c_double` IS `f64`,
// so NO ABI cast is missing and the artifact BUILDS AND RUNS — the symptom is a
// `Symptom::Divergence`, not a build error. This witness drives the deterministic
// `FloatReprRepair` rule against the REAL toolchain + the REAL Day-3 oracle: the
// plain-print artifact builds, runs, and diverges (`10` vs `10.0`); the loop
// rewrites the print into the CPython-faithful `.0`-suffix repr block and
// re-runs to a byte-identical `10.0`. This broadens the agent-repair witness
// from the build-error (E0308) class to the executing divergence class — the
// class PMAT-931 actually fixed in production.
// ─────────────────────────────────────────────────────────────────────────────

/// The C side of the `hybrid_scale2` fixture: `a * b` over `double`.
const C_SOURCE_F64: &str = "double scale2(double a, double b){return a*b;}\n";

/// A float shim that BUILDS and RUNS but prints `10` (the whole-float repr
/// divergence). The ABI casts are already correct (`c_double` == `f64`); the bug
/// is the plain `println!("{}", …)`, exactly the class PMAT-931 fixed.
const FLOAT_PLAIN_PRINT: &str = "unsafe extern \"C\" {\n    fn scale2(a: ::std::os::raw::c_double, b: ::std::os::raw::c_double) -> ::std::os::raw::c_double;\n}\npub fn scale2_shim(a: f64, b: f64) -> f64 {\n    let __r = unsafe { scale2(a as ::std::os::raw::c_double, b as ::std::os::raw::c_double) };\n    __r as f64\n}\nfn main() {\n    println!(\"{}\", scale2_shim(2.0, 5.0));\n}\n";

#[test]
fn repair_loop_converges_the_whole_float_repr_divergence_to_cpython() {
    if !HybridCcRustcProbe::toolchain_available() || !PythonOracle::available() {
        eprintln!("cc/rustc/python3 unavailable — skipping float repair-loop witness");
        return;
    }

    // Phase 3 — the Day-3 oracle: CPython binds `scale2` via ctypes with
    // `restype = c_double`, so `scale2(2.0, 5.0) = 10.0` prints Python's `10.0`.
    let py = "from ._core import scale2\ndef main() -> None:\n    print(scale2(2.0, 5.0))";
    let bindings = vec![CtypesBinding {
        symbol: "scale2".to_string(),
        argtypes: vec!["c_double", "c_double"],
        restype: Some("c_double"),
    }];
    let reference = capture_cpython_hybrid_ref(
        py,
        &[("_core.c".to_string(), C_SOURCE_F64.to_string())],
        &bindings,
    )
    .expect("capture CPython hybrid float reference");
    assert_eq!(
        reference, "10.0",
        "ctypes c_double prints Python's whole-number float repr (PMAT-931)"
    );

    // Sanity: the plain-print float shim BUILDS and RUNS but DIVERGES (`10` vs
    // `10.0`) — a `Divergence`, not a build error (c_double IS f64).
    let probe = HybridCcRustcProbe {
        c_source: C_SOURCE_F64.to_string(),
        reference: reference.clone(),
    };
    match probe.evaluate(FLOAT_PLAIN_PRINT) {
        Err(xpile_agent::Symptom::Divergence {
            expected, actual, ..
        }) => {
            assert_eq!(expected, "10.0");
            assert_eq!(actual, "10", "the plain print emits the bare integer");
        }
        other => panic!("expected a whole-float repr Divergence, got {other:?}"),
    }

    // Phase 6 — the deterministic float-repr repair loop converges to a match.
    let outcome = RepairLoop::ffi_float_repr(Default::default(), "scale2_shim(2.0, 5.0)")
        .run(&probe, FLOAT_PLAIN_PRINT);

    match outcome {
        RepairOutcome::Repaired { iterations, source } => {
            assert_eq!(iterations, 1, "one float-repr rewrite");
            assert!(
                probe.evaluate(&source).is_ok(),
                "repaired artifact prints the CPython-faithful `10.0`"
            );
            assert!(source.contains("println!(\"{}.0\", __rv)"));
        }
        other => panic!("expected Repaired, got {other:?}"),
    }
}
