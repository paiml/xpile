//! PMAT-950 — executed cross-vendor GPU DiffExec witness for the WGSL
//! §29 lane (`C-COMPILE-RUST-TO-WGSL`).
//!
//! Sibling of `crates/xpile-ptx-codegen/tests/gpu_witness.rs` (PMAT-949,
//! the NVIDIA-only CUDA witness). This one runs the same
//! `out[i] = 2*in[i] + 1` semantics through **wgpu**, so the witness is
//! cross-vendor: Vulkan (RTX 4090 / AMD Navi10), Metal (Apple), or DX12
//! (Windows).
//!
//! Graceful-skip posture (mirrors the cc/python3 / nvcc differential
//! gates): when no wgpu adapter is present (free CI runners have no GPU),
//! the engine is never installed, the backend records the benign
//! `NotRun { no-engine }`, and the test asserts that well-behaved
//! fallback and exits OK. On a GPU box the engine RUNS BOTH emitters'
//! WGSL on the adapter and asserts the executed outputs agree → a real
//! `DiffExecResult::Match`.

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, HwProfile, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::{Module, SourceLang};
use xpile_wgsl_codegen::{wgpu_adapter_available, WgslBackend};

fn kernel_module() -> Module {
    Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

fn wgsl_config() -> BackendConfig {
    BackendConfig {
        target: Target::Wgsl,
        profile: Profile::RustOut,
        hardware: Some(HwProfile::Wgsl {
            features: Vec::new(),
        }),
    }
}

#[test]
fn wgpu_diffexec_executes_on_gpu_and_matches() {
    if !wgpu_adapter_available() {
        eprintln!(
            "PMAT-950: skipping executed GPU witness — no wgpu adapter present. \
             A GPU box runs this and produces a real DiffExecResult::Match; \
             free CI records the benign NotRun {{ no-engine }} and stays green."
        );

        // Even with no adapter the backend must stay well-behaved: both
        // real emitters fire and the quorum records NotRun (NOT a crash,
        // NOT a fake Match). This keeps the path under test in CI.
        let backend = WgslBackend::new_wgpu_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &wgsl_config())
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real emitters should fire");
            }
            other => panic!("expected Multi NotRun (no adapter), got {other:?}"),
        }
        return;
    }

    eprintln!("PMAT-950: running executed cross-vendor GPU witness via wgpu");

    let backend = WgslBackend::new_wgpu_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &wgsl_config())
        .expect("witness backend lowers + runs on GPU");

    // The primary emission carries a real WGSL compute shader + the contract.
    assert!(
        artifact.primary.contains("@compute") && artifact.primary.contains("fn main"),
        "primary should be a real WGSL compute shader, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact
            .citations
            .iter()
            .any(|c| c.as_str() == "C-COMPILE-RUST-TO-WGSL"),
        "emission must cite C-COMPILE-RUST-TO-WGSL"
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert_eq!(emitters.len(), 2, "general + specialist both ran");
            assert!(
                emitters.iter().any(|e| e == "wgsl-saxpy-general"),
                "general emitter must be reported, got {emitters:?}"
            );
            assert!(
                emitters.iter().any(|e| e == "wgsl-saxpy-specialist-fma"),
                "specialist emitter must be reported, got {emitters:?}"
            );
            // `out = 2*in + 1` is exactly representable for the fixture
            // inputs; the explicit mul+add and the `fma` builtin agree
            // bit-for-bit here.
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed GPU outputs diverged: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-950: EXECUTED cross-vendor GPU witness PASSED — \
                 general vs specialist agree (max_abs_diff={max_abs_diff}). \
                 This is the real Run≥1 DiffExecResult::Match closing the \
                 WGSL §29 on-hardware Vulkan DiffExec caveat (PMAT-490)."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("GPU emitters DIVERGED (contract falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match on a GPU box, got {other:?}"),
    }
}
