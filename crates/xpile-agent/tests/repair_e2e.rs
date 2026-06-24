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
