//! PMAT-960 / PMAT-977 — executed Vulkan SPIR-V DiffExec witness for the
//! SPIR-V §29 lane (`C-COMPILE-RUST-TO-SPIRV`).
//!
//! Sibling of `crates/xpile-wgsl-codegen/tests/gpu_witness.rs` (PMAT-950).
//! Where the WGSL witness uploads WGSL source to wgpu, this one compiles
//! WGSL to SPIR-V via naga and uploads the SPIR-V binary directly
//! (`ShaderSource::SpirV`) — the native Vulkan IR execution path.
//!
//! ## PMAT-977 — the general side proves xpile's REAL emission
//!
//! Before PMAT-977 BOTH sides ran hardcoded WGSL string constants, so the
//! witness proved `hardcoded shader → SPIR-V → run`, never exercising
//! xpile's compiler. PMAT-977 rewires the **general** emitter to the REAL
//! path: a meta-HIR `saxpy` module → `xpile_wgsl_codegen::emit_wgsl_module`
//! (the real PMAT-970 lowering) → a thin `@compute` dispatch harness →
//! naga SPIR-V → run on Vulkan. The executed result is checked against a
//! CPython-equivalent expected vector AND the independent `fma` reference.
//! So a Match now attests `meta-HIR → xpile real WGSL → naga SPIR-V → run
//! → correct`, not `hardcoded shader → run`.
//!
//! Graceful-skip posture (mirrors the WGSL / cc / nvcc gates): when no
//! wgpu Vulkan adapter is present (free CI runners have no GPU), the engine
//! is never installed, the backend records the benign `NotRun { no-engine }`,
//! and the test asserts that well-behaved fallback and exits OK. On a Vulkan
//! box the engine RUNS BOTH emitters' SPIR-V on the adapter and asserts the
//! executed outputs agree → a real `DiffExecResult::Match`.

//! ## PMAT-1388 — the witness now HANDS the backend the module it attests
//!
//! PMAT-977's claim above was true of the *emitter* but untestable by *this
//! witness*: `kernel_module()` returned a module named `saxpy_spirv_kernel`
//! with `items: Vec::new()` — no functions at all — and the general emitter
//! discarded its argument and re-derived the saxpy module internally. So the
//! witness asserted on a shader it had supplied no input for, and could not
//! have detected the emitter ignoring that input (PMAT-1388: it did, for
//! every program). The witness now passes `general_metahir_module()` — the
//! very module whose lowering it prints and executes — so the assertions
//! below are about a compilation of something the test actually provided.

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, HwProfile, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::Module;
use xpile_spirv_codegen::{
    general_metahir_module, general_real_wgsl, vulkan_adapter_available, SpirvBackend,
};

/// The module under test — the REAL meta-HIR `saxpy` module whose lowering
/// this witness prints, compiles to SPIR-V and executes on Vulkan.
fn kernel_module() -> Module {
    general_metahir_module()
}

fn spirv_config() -> BackendConfig {
    BackendConfig {
        emit_contracts: true,
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
        // real emitters fire (the general one through xpile's REAL
        // emit_wgsl_module path, PMAT-977) and the quorum records NotRun
        // (NOT a crash, NOT a fake Match).
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

    eprintln!("PMAT-960/PMAT-977: running executed Vulkan SPIR-V witness via wgpu");

    // Show the REAL xpile emission the general side drives to SPIR-V: this
    // is meta-HIR → emit_wgsl_module → @compute harness, NOT a hardcoded
    // shader. Printed so the captured witness output proves the real path.
    let real_wgsl = general_real_wgsl().expect("xpile real WGSL emit");
    eprintln!(
        "PMAT-977: general side runs xpile's REAL emission \
         (meta-HIR -> emit_wgsl_module -> @compute -> naga SPIR-V):\n{real_wgsl}"
    );

    let backend = SpirvBackend::new_spirv_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &spirv_config())
        .expect("witness backend lowers + runs on Vulkan");

    // The primary emission carries a real SPIR-V module summary + contract,
    // and (PMAT-977) the real lowered `saxpy` arithmetic xpile emitted —
    // the SPIR-V summary inlines the source WGSL it compiled.
    assert!(
        artifact.primary.contains("SPIR-V") && artifact.primary.contains("Magic:"),
        "primary should be a real SPIR-V summary, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact.primary.contains("(x * f32(2.0)) + f32(1.0)"),
        "primary must carry xpile's REAL lowered saxpy arithmetic, got:\n{}",
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
                emitters.iter().any(|e| e == "spirv-general"),
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
                "PMAT-960/PMAT-977: EXECUTED Vulkan SPIR-V witness PASSED — xpile's \
                 REAL emission (meta-HIR -> emit_wgsl_module -> naga SPIR-V) ran on \
                 Vulkan and matched the CPython-equivalent reference AND the \
                 independent fma module (max_abs_diff={max_abs_diff}). This is the \
                 real Run>=1 DiffExecResult::Match proving \
                 `meta-HIR -> xpile real WGSL -> naga SPIR-V -> run -> correct`."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("SPIR-V emitters DIVERGED (contract falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match on a Vulkan box, got {other:?}"),
    }
}
