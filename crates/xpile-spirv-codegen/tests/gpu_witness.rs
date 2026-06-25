//! PMAT-960 — executed Vulkan SPIR-V DiffExec witness for the SPIR-V §29
//! lane (`C-COMPILE-RUST-TO-SPIRV`).
//!
//! Sibling of `crates/xpile-wgsl-codegen/tests/gpu_witness.rs` (PMAT-950).
//! Where the WGSL witness uploads WGSL source to wgpu, this one compiles
//! the REUSED WGSL to SPIR-V via naga and uploads the SPIR-V binary
//! directly (`ShaderSource::SpirV`) — the native Vulkan IR execution path.
//!
//! Graceful-skip posture (mirrors the WGSL / cc / nvcc gates): when no
//! wgpu Vulkan adapter is present (free CI runners have no GPU), the engine
//! is never installed, the backend records the benign `NotRun { no-engine }`,
//! and the test asserts that well-behaved fallback and exits OK. On a Vulkan
//! box the engine RUNS BOTH emitters' SPIR-V on the adapter and asserts the
//! executed outputs agree → a real `DiffExecResult::Match`.

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, HwProfile, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::{Module, SourceLang};
use xpile_spirv_codegen::{vulkan_adapter_available, SpirvBackend};

fn kernel_module() -> Module {
    Module {
        name: "saxpy_spirv_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

fn spirv_config() -> BackendConfig {
    BackendConfig {
        target: Target::Spirv,
        profile: Profile::RustOut,
        hardware: Some(HwProfile::Spirv { version: (1, 3) }),
    }
}

#[test]
fn spirv_diffexec_executes_on_vulkan_and_matches() {
    if !vulkan_adapter_available() {
        eprintln!(
            "PMAT-960: skipping executed Vulkan SPIR-V witness — no wgpu Vulkan \
             adapter present. A Vulkan box runs this and produces a real \
             DiffExecResult::Match; free CI records the benign NotRun {{ no-engine }} \
             and stays green."
        );

        // Even with no adapter the backend must stay well-behaved: both
        // real emitters fire (each compiling reused WGSL to SPIR-V) and
        // the quorum records NotRun (NOT a crash, NOT a fake Match).
        let backend = SpirvBackend::new_spirv_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &spirv_config())
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real SPIR-V emitters should fire");
            }
            other => panic!("expected Multi NotRun (no adapter), got {other:?}"),
        }
        return;
    }

    eprintln!("PMAT-960: running executed Vulkan SPIR-V witness via wgpu");

    let backend = SpirvBackend::new_spirv_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &spirv_config())
        .expect("witness backend lowers + runs on Vulkan");

    // The primary emission carries a real SPIR-V module summary + contract.
    assert!(
        artifact.primary.contains("SPIR-V") && artifact.primary.contains("Magic:"),
        "primary should be a real SPIR-V summary, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact
            .citations
            .iter()
            .any(|c| c.as_str() == "C-COMPILE-RUST-TO-SPIRV"),
        "emission must cite C-COMPILE-RUST-TO-SPIRV"
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert_eq!(emitters.len(), 2, "general + specialist both ran");
            assert!(
                emitters.iter().any(|e| e == "spirv-saxpy-general"),
                "general emitter must be reported, got {emitters:?}"
            );
            assert!(
                emitters.iter().any(|e| e == "spirv-saxpy-specialist-fma"),
                "specialist emitter must be reported, got {emitters:?}"
            );
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed SPIR-V outputs diverged: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-960: EXECUTED Vulkan SPIR-V witness PASSED — general vs \
                 specialist agree (max_abs_diff={max_abs_diff}). This is the real \
                 Run>=1 DiffExecResult::Match for the native Vulkan IR lane, the \
                 SPIR-V sibling of the WGSL wgpu witness (PMAT-950)."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("SPIR-V emitters DIVERGED (contract falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match on a Vulkan box, got {other:?}"),
    }
}
