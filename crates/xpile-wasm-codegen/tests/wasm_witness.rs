//! PMAT-952 (runtime-witness half) — executed WASM-runtime DiffExec
//! witness for the native-WASM §29 lane (`C-COMPILE-RUST-TO-WASM`).
//!
//! Sibling of `crates/xpile-ptx-codegen/tests/gpu_witness.rs` (PMAT-949,
//! the CUDA witness) and `crates/xpile-wgsl-codegen/tests/gpu_witness.rs`
//! (PMAT-950, the cross-vendor wgpu/WGSL witness). This one runs the same
//! `out[i] = 2*in[i] + 1` semantics through **WABT** (`wat2wasm` assembles
//! each module; `wasm-interp --run-all-exports` executes every exported
//! function) — the runtime-stratum upgrade of the EMIT-only PMAT-951
//! slice, with NO new Cargo dependency.
//!
//! Graceful-skip posture (mirrors the cc/python3 / nvcc / wgpu differential
//! gates): when WABT is absent (free CI runners have no `wat2wasm` /
//! `wasm-interp`), the engine is never installed, the backend records the
//! benign `NotRun { no-engine }`, and the test asserts that well-behaved
//! fallback and exits OK. On a box with WABT the engine RUNS BOTH emitters'
//! WAT in the wasm runtime and asserts the executed outputs agree → a real
//! `DiffExecResult::Match`.

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::{Module, SourceLang};
use xpile_wasm_codegen::{wasm_runtime_available, WasmBackend};

fn kernel_module() -> Module {
    Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

fn wasm_config() -> BackendConfig {
    BackendConfig {
        target: Target::Wasm,
        profile: Profile::RustOut,
        hardware: None,
    }
}

#[test]
fn wasm_diffexec_executes_in_runtime_and_matches() {
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-952: skipping executed WASM witness — WABT (wat2wasm / \
             wasm-interp) absent. A box with WABT runs this and produces a \
             real DiffExecResult::Match; free CI records the benign \
             NotRun {{ no-engine }} and stays green."
        );

        // Even with no runtime the backend must stay well-behaved: both
        // real emitters fire and the quorum records NotRun (NOT a crash,
        // NOT a fake Match). This keeps the path under test in CI.
        let backend = WasmBackend::new_wasm_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &wasm_config())
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real emitters should fire");
            }
            other => panic!("expected Multi NotRun (no runtime), got {other:?}"),
        }
        return;
    }

    eprintln!("PMAT-952: running executed WASM-runtime witness via WABT");

    let backend = WasmBackend::new_wasm_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &wasm_config())
        .expect("witness backend lowers + runs in wasm runtime");

    // The primary emission carries a real WAT module + the contract.
    assert!(
        artifact.primary.contains("(module") && artifact.primary.contains("(func (export"),
        "primary should be a real WAT module, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact
            .citations
            .iter()
            .any(|c| c.as_str() == "C-COMPILE-RUST-TO-WASM"),
        "emission must cite C-COMPILE-RUST-TO-WASM"
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert_eq!(emitters.len(), 2, "general + specialist both ran");
            assert!(
                emitters.iter().any(|e| e == "wasm-saxpy-general"),
                "general emitter must be reported, got {emitters:?}"
            );
            assert!(
                emitters
                    .iter()
                    .any(|e| e == "wasm-saxpy-specialist-doubling"),
                "specialist emitter must be reported, got {emitters:?}"
            );
            // `out = 2*x + 1` is exactly representable for the fixture
            // inputs; the explicit `x*2+1` and the reassociated `(x+x)+1`
            // agree bit-for-bit in IEEE-754 f64 here.
            assert!(
                max_abs_diff <= 1.0e-9,
                "executed WASM outputs diverged: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-952: EXECUTED WASM-runtime witness PASSED — general \
                 (x*2+1) vs specialist ((x+x)+1) agree (max_abs_diff={max_abs_diff}). \
                 This is the real Run≥1 DiffExecResult::Match upgrading \
                 C-COMPILE-RUST-TO-WASM to the runtime stratum."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("WASM emitters DIVERGED (contract falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match with WABT present, got {other:?}"),
    }
}
