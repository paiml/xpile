//! PMAT-949 — executed GPU DiffExec witness for `C-COMPILE-RUST-TO-PTX-MMA`.
//!
//! This is the load-bearing Run≥1 witness that closes the
//! audit-design.md §4 / §62 "Run=1 / `DiffExecResult::NotRun`" caveat.
//!
//! Graceful-skip posture (mirrors the cc/python3 differential gates):
//! when `nvcc` + `nvidia-smi` are absent (free CI runners have no GPU),
//! the engine is never installed, the backend records the benign
//! `NotRun { no-engine }`, and the test prints a skip notice and exits
//! OK. On a CUDA box (RTX 4090 / sm_89, GB10 / sm_121) the engine
//! `nvcc`-compiles BOTH emitters' kernels, RUNS them on the GPU, and the
//! test asserts the executed outputs agree → a real
//! `DiffExecResult::Match`.

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, HwProfile, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::{Module, SourceLang};
use xpile_ptx_codegen::{cuda_toolchain_available, PtxBackend};

fn kernel_module() -> Module {
    Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

/// Detect the local GPU's compute capability via `nvidia-smi`, formatted
/// as `sm_<major><minor>` (e.g. `sm_89`). Falls back to `sm_80` (the
/// contract floor) if the query is unavailable — the caller only reaches
/// this after `cuda_toolchain_available()` is true.
fn local_compute_capability() -> String {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let raw = String::from_utf8_lossy(&o.stdout);
            // First GPU's "8.9" → "sm_89".
            if let Some(line) = raw.lines().next() {
                let trimmed = line.trim();
                if let Some((maj, min)) = trimmed.split_once('.') {
                    return format!("sm_{}{}", maj.trim(), min.trim());
                }
            }
            "sm_80".to_string()
        }
        _ => "sm_80".to_string(),
    }
}

fn ptx_config(sm: &str) -> BackendConfig {
    BackendConfig {
        emit_contracts: true,
        target: Target::Ptx,
        profile: Profile::RustOut,
        hardware: Some(HwProfile::Ptx {
            compute_capability: sm.to_string(),
        }),
    }
}

#[test]
fn cuda_diffexec_executes_on_gpu_and_matches() {
    if !cuda_toolchain_available() {
        eprintln!(
            "PMAT-949: skipping executed GPU witness — nvcc/nvidia-smi not present. \
             A CUDA box runs this and produces a real DiffExecResult::Match; \
             free CI records the benign NotRun {{ no-engine }} and stays green."
        );

        // Even on a non-GPU host the backend must stay well-behaved:
        // both real emitters fire and the quorum records NotRun (NOT a
        // crash, NOT a fake Match). This keeps the path under test in CI.
        let backend = PtxBackend::new_cuda_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &ptx_config("sm_80"))
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real emitters should fire");
            }
            other => panic!("expected Multi NotRun (no GPU), got {other:?}"),
        }
        return;
    }

    let sm = local_compute_capability();
    eprintln!("PMAT-949: running executed GPU witness on {sm}");

    let backend = PtxBackend::new_cuda_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &ptx_config(&sm))
        .expect("witness backend lowers + runs on GPU");

    // The primary emission carries a real CUDA-C kernel + the contract.
    assert!(
        artifact.primary.contains("__global__ void xpile_kernel"),
        "primary should be a real CUDA-C kernel, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact
            .citations
            .iter()
            .any(|c| c.as_str() == "C-COMPILE-RUST-TO-PTX-MMA"),
        "emission must cite C-COMPILE-RUST-TO-PTX-MMA"
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert_eq!(emitters.len(), 2, "general + specialist both ran");
            assert!(
                emitters.iter().any(|e| e == "cuda-saxpy-general"),
                "general emitter must be reported, got {emitters:?}"
            );
            assert!(
                emitters.iter().any(|e| e == "cuda-saxpy-specialist-fma"),
                "specialist emitter must be reported, got {emitters:?}"
            );
            // `out = 2*in + 1` is exactly representable for the fixture
            // inputs; mul+add and fmaf agree bit-for-bit here.
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed GPU outputs diverged: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-949: EXECUTED GPU witness PASSED on {sm} — \
                 general vs specialist agree (max_abs_diff={max_abs_diff}). \
                 This is the real Run≥1 DiffExecResult::Match closing the \
                 §4/§62 NotRun caveat for C-COMPILE-RUST-TO-PTX-MMA."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("GPU emitters DIVERGED (contract falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match on a GPU box, got {other:?}"),
    }
}
