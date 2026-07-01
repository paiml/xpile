//! PMAT-1006 — the PRODUCTION 3-way §29 PTX quorum witness.
//!
//! PMAT-997 proved the rustc-nvptx (LLVM NVPTX back-end) emitter agrees with
//! xpile's hand-emit at WITNESS level. This lowers the §29 quorum through the
//! PRODUCTION `PtxBackend::new_ptx_3way_diffexec_witness()` path: the DiffExec
//! runs THREE categorically-independent codegen toolchains — xpile's hand-emitted
//! PTX text, the nvcc-compiled CUDA-C (NVVM/LLVM-7), and nightly rustc's NVPTX
//! back-end (modern LLVM) — for the same `out[i] = 2*in[i] + 1` kernel, and the
//! `QuorumStatus::Multi` HONESTLY names every toolchain that voted.
//!
//! Graceful degradation (the honesty invariant):
//!   - CUDA box + nightly rustc + nvptx64 target → runs all THREE, reports 3
//!     emitters + a real `DiffExecResult::Match`.
//!   - CUDA box WITHOUT rustc-nvptx → falls back to the 2-way quorum (2 emitters).
//!   - no GPU (free CI) → `NotRun` with 2 emitters — NEVER a false third voter.

use xpile_backend::{
    Artifact, Backend, BackendConfig, DiffExecResult, HwProfile, Profile, QuorumStatus, Target,
};
use xpile_meta_hir::{Module, SourceLang};
use xpile_ptx_codegen::{cuda_toolchain_available, rustc_nvptx_available, PtxBackend};

fn kernel_module() -> Module {
    Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

fn local_compute_capability() -> String {
    if let Ok(o) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output()
    {
        if o.status.success() {
            if let Some(line) = String::from_utf8_lossy(&o.stdout).lines().next() {
                if let Some((maj, min)) = line.trim().split_once('.') {
                    return format!("sm_{}{}", maj.trim(), min.trim());
                }
            }
        }
    }
    "sm_80".to_string()
}

fn ptx_config(sm: &str) -> BackendConfig {
    BackendConfig {
        target: Target::Ptx,
        profile: Profile::RustOut,
        hardware: Some(HwProfile::Ptx {
            compute_capability: sm.to_string(),
        }),
    }
}

#[test]
fn ptx_three_way_quorum_reports_and_agrees() {
    let backend = PtxBackend::new_ptx_3way_diffexec_witness();

    if !cuda_toolchain_available() {
        // No GPU: the quorum records NotRun with the TWO base emitters — never a
        // false rustc-nvptx voter (the extra emitter is appended only on execute).
        let artifact: Artifact = backend
            .lower(&kernel_module(), &ptx_config("sm_80"))
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(
                    emitters.len(),
                    2,
                    "no GPU → only the 2 fired emitters, got {emitters:?}"
                );
                assert!(
                    !emitters.iter().any(|e| e == "rustc-nvptx"),
                    "rustc-nvptx must NOT be reported without an executed run: {emitters:?}"
                );
            }
            other => panic!("expected Multi NotRun (no GPU), got {other:?}"),
        }
        eprintln!(
            "PMAT-1006: no GPU — 3-way quorum backend records NotRun with 2 emitters \
             (no false third voter). A CUDA + nightly-rustc box runs all three."
        );
        return;
    }

    let sm = local_compute_capability();
    let three_way = rustc_nvptx_available();
    eprintln!(
        "PMAT-1006: running the PRODUCTION §29 PTX quorum on {sm} ({}-way)",
        if three_way { 3 } else { 2 }
    );

    let artifact: Artifact = backend
        .lower(&kernel_module(), &ptx_config(&sm))
        .expect("3-way witness backend lowers + runs the toolchains on the GPU");

    assert!(
        artifact.primary.contains(".visible .entry xpile_kernel"),
        "primary is xpile's hand-emitted PTX"
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed toolchains diverged: max_abs_diff={max_abs_diff}"
            );
            assert!(emitters.iter().any(|e| e == "xpile-ptx-hand-emitted"));
            assert!(emitters.iter().any(|e| e == "cuda-saxpy-general"));
            if three_way {
                assert_eq!(
                    emitters.len(),
                    3,
                    "3-way must report exactly 3 voters, got {emitters:?}"
                );
                assert!(
                    emitters.iter().any(|e| e == "rustc-nvptx"),
                    "the rustc-nvptx voter must be reported when it ran: {emitters:?}"
                );
                eprintln!(
                    "PMAT-1006: PRODUCTION 3-WAY §29 quorum PASSED on {sm} — xpile \
                     hand-emit PTX, nvcc CUDA-C, AND rustc-nvptx (LLVM NVPTX) all \
                     agree (max_abs_diff={max_abs_diff}); QuorumStatus::Multi names \
                     all 3 toolchains. A miscompile would have to corrupt all three \
                     codegen paths identically."
                );
            } else {
                assert_eq!(emitters.len(), 2, "2-way fallback reports 2 voters");
                eprintln!(
                    "PMAT-1006: rustc-nvptx absent — 2-way quorum ran (xpile + nvcc \
                     agree, max_abs_diff={max_abs_diff}); no false third voter."
                );
            }
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!("PTX toolchains DIVERGED (quorum falsified): max_abs_diff={max_abs_diff}"),
        other => panic!("expected an executed Multi Match on a GPU box, got {other:?}"),
    }
}
