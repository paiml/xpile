//! PMAT-961 — the TRUE anti-correlation §29 PTX witness.
//!
//! The categorical-independence upgrade of the PMAT-949 GPU witness. Where
//! PMAT-949 diffed two CUDA-C kernels compiled by the SAME `nvcc` (mul+add vs
//! `fmaf`), this diffs two **categorically-independent codegen toolchains** for
//! the same `out[i] = 2*in[i] + 1` kernel on the GPU:
//!
//!   - general: xpile's OWN hand-emitted PTX (from `emit_kernel`), loaded +
//!     JIT-assembled by the CUDA Driver API (`cuModuleLoadData`).
//!   - specialist: the nvcc-compiled CUDA-C `xpile_kernel` (the PMAT-949 path).
//!
//! These share NO codegen frontend; they agree only if BOTH lowerings are
//! correct — the anti-correlation property. Sibling of the wasm-runtime /
//! wgpu / SPIR-V witnesses, on real NVIDIA silicon.
//!
//! Graceful-skip (mirrors cc/python3/nvcc/WABT/wgpu): no `nvcc` + `nvidia-smi`
//! → the engine is never installed, the backend records the benign
//! `NotRun { no-engine }`, the test asserts that well-behaved fallback and
//! exits OK (free CI stays green). On a CUDA box (RTX 4090 / sm_89) the engine
//! runs BOTH toolchains on the GPU and asserts the executed outputs agree → a
//! real `DiffExecResult::Match`.

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

/// The local GPU's compute capability via `nvidia-smi` (`sm_<maj><min>`),
/// falling back to the contract floor `sm_80`.
fn local_compute_capability() -> String {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let raw = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = raw.lines().next() {
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
fn ptx_anti_correlation_executes_on_gpu_and_matches() {
    if !cuda_toolchain_available() {
        eprintln!(
            "PMAT-961: skipping anti-correlation PTX witness — nvcc/nvidia-smi \
             not present. A CUDA box runs xpile's hand-emitted PTX (Driver API) \
             vs the nvcc-compiled CUDA-C and produces a real \
             DiffExecResult::Match; free CI records NotRun {{ no-engine }} and \
             stays green."
        );

        // Even on a non-GPU host the backend must stay well-behaved: both real
        // emitters fire and the quorum records NotRun (NOT a crash, NOT a fake
        // Match). Keeps the path under test in CI.
        let backend = PtxBackend::new_ptx_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &ptx_config("sm_80"))
            .expect("witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real emitters should fire");
                assert!(
                    emitters.iter().any(|e| e == "xpile-ptx-hand-emitted"),
                    "the hand-emitted PTX (general) emitter must be reported, got {emitters:?}"
                );
                assert!(
                    emitters.iter().any(|e| e == "cuda-saxpy-general"),
                    "the nvcc CUDA-C (specialist) emitter must be reported, got {emitters:?}"
                );
            }
            other => panic!("expected Multi NotRun (no GPU), got {other:?}"),
        }
        return;
    }

    let sm = local_compute_capability();
    eprintln!("PMAT-961: running anti-correlation PTX witness on {sm}");

    let backend = PtxBackend::new_ptx_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &ptx_config(&sm))
        .expect("witness backend lowers + runs both toolchains on GPU");

    // The primary emission carries xpile's OWN hand-emitted PTX (general slot).
    assert!(
        artifact.primary.contains(".visible .entry xpile_kernel"),
        "primary should be xpile's hand-emitted PTX, got:\n{}",
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
            assert_eq!(emitters.len(), 2, "both codegen paths ran");
            assert!(
                emitters.iter().any(|e| e == "xpile-ptx-hand-emitted"),
                "xpile hand-emitted PTX path must be reported, got {emitters:?}"
            );
            assert!(
                emitters.iter().any(|e| e == "cuda-saxpy-general"),
                "nvcc CUDA-C path must be reported, got {emitters:?}"
            );
            // `out = 2*in + 1` is exactly representable for the fixture; the
            // hand-emitted PTX `(x+x)+1` and the nvcc `2*x+1` agree
            // bit-for-bit on the RTX 4090.
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed outputs diverged across toolchains: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-961: ANTI-CORRELATION PTX witness PASSED on {sm} — xpile's \
                 hand-emitted PTX (Driver API) vs nvcc-compiled CUDA-C agree \
                 (max_abs_diff={max_abs_diff}). Two CATEGORICALLY-INDEPENDENT \
                 codegen toolchains, upgrading PMAT-949's two-CUDA-C-same-nvcc \
                 check to a genuinely independent pair."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!(
            "toolchains DIVERGED (contract falsified): xpile PTX vs nvcc CUDA-C \
             max_abs_diff={max_abs_diff}"
        ),
        other => panic!("expected an executed Multi Match on a GPU box, got {other:?}"),
    }
}

/// PMAT-962 — the anti-correlation witness over a NEW construct: **control
/// flow**. The relu kernel `out[i] = (in[i] > 0) ? in[i] : 0` is emitted as
/// xpile's OWN hand-emitted PTX (a real `setp.gt.f64` + `@!%p bra` branch +
/// shared result register) and, independently, as nvcc-compiled CUDA-C (a C
/// `?:` ternary). The two categorically-independent toolchains must agree on
/// the GPU — proving the PMAT-962 `if`/`else` lowering is correct on real
/// silicon, not just ptxas-well-formed.
///
/// Same graceful-skip posture: no nvcc/nvidia-smi → benign `NotRun`, CI green.
#[test]
fn ptx_if_anti_correlation_executes_on_gpu_and_matches() {
    if !cuda_toolchain_available() {
        eprintln!(
            "PMAT-962: skipping if-bearing anti-correlation PTX witness — \
             nvcc/nvidia-smi not present. A CUDA box runs xpile's hand-emitted \
             branch PTX (Driver API) vs nvcc-compiled CUDA-C `?:` and produces a \
             real Match; free CI records NotRun and stays green."
        );
        let backend = PtxBackend::new_ptx_if_diffexec_witness();
        let artifact: Artifact = backend
            .lower(&kernel_module(), &ptx_config("sm_80"))
            .expect("if-witness backend lowers");
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec: Some(DiffExecResult::NotRun { .. }),
            } => {
                assert_eq!(emitters.len(), 2, "both real emitters should fire");
                assert!(
                    emitters.iter().any(|e| e == "xpile-ptx-hand-emitted-if"),
                    "the hand-emitted branch PTX (general) emitter must be reported, got {emitters:?}"
                );
                assert!(
                    emitters.iter().any(|e| e == "cuda-relu-general"),
                    "the nvcc CUDA-C relu (specialist) emitter must be reported, got {emitters:?}"
                );
            }
            other => panic!("expected Multi NotRun (no GPU), got {other:?}"),
        }
        return;
    }

    let sm = local_compute_capability();
    eprintln!("PMAT-962: running if-bearing anti-correlation PTX witness on {sm}");

    let backend = PtxBackend::new_ptx_if_diffexec_witness();
    let artifact: Artifact = backend
        .lower(&kernel_module(), &ptx_config(&sm))
        .expect("if-witness backend lowers + runs both toolchains on GPU");

    // The primary emission carries xpile's OWN hand-emitted branch PTX.
    assert!(
        artifact.primary.contains(".visible .entry xpile_kernel"),
        "primary should be xpile's hand-emitted PTX, got:\n{}",
        artifact.primary
    );
    assert!(
        artifact.primary.contains("setp.gt.f64"),
        "primary should carry the if-condition `setp.gt.f64`, got:\n{}",
        artifact.primary
    );

    match artifact.quorum_status {
        QuorumStatus::Multi {
            emitters,
            diff_exec: Some(DiffExecResult::Match { max_abs_diff }),
        } => {
            assert_eq!(emitters.len(), 2, "both codegen paths ran");
            assert!(
                emitters.iter().any(|e| e == "xpile-ptx-hand-emitted-if"),
                "xpile hand-emitted branch PTX path must be reported, got {emitters:?}"
            );
            assert!(
                emitters.iter().any(|e| e == "cuda-relu-general"),
                "nvcc CUDA-C relu path must be reported, got {emitters:?}"
            );
            // relu over the fixture is exactly representable; the hand-emitted
            // branch PTX and the nvcc `?:` agree bit-for-bit on the RTX 4090.
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed branchy outputs diverged across toolchains: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-962: IF-BEARING ANTI-CORRELATION PTX witness PASSED on {sm} — \
                 xpile's hand-emitted branch PTX (setp.gt.f64 + @!%p bra, Driver API) \
                 vs nvcc-compiled CUDA-C `?:` agree (max_abs_diff={max_abs_diff}). The \
                 PMAT-962 control-flow lowering is correct on real silicon."
            );
        }
        QuorumStatus::Multi {
            diff_exec: Some(DiffExecResult::Divergent { max_abs_diff, .. }),
            ..
        } => panic!(
            "branchy toolchains DIVERGED (control-flow lowering falsified): xpile PTX \
             vs nvcc CUDA-C max_abs_diff={max_abs_diff}"
        ),
        other => panic!("expected an executed Multi Match on a GPU box, got {other:?}"),
    }
}
