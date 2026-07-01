//! PMAT-997 — the THIRD categorically-independent §29 PTX emitter witness:
//! nightly `rustc`'s `nvptx64-nvidia-cuda` target (LLVM NVPTX back-end).
//!
//! The existing §29 PTX pair (PMAT-961) already runs two independent toolchains
//! for `out[i] = 2*in[i] + 1` and agrees on the GPU: xpile's OWN hand-emitted
//! PTX text vs the `nvcc`-compiled CUDA-C (C++ front-end → NVVM/LLVM-7). This
//! adds a THIRD path that fails DIFFERENTLY than both: a real Rust kernel
//! compiled by nightly `rustc` through **modern LLVM's NVPTX back-end**.
//!
//! The witness EXECUTES the 3rd emitter's PTX on real NVIDIA silicon (via the
//! CUDA Driver API `cuModuleLoadData` — the driver JIT-assembles the PTX) using
//! the SAME [`PtxDiffExecEngine::driver_harness`] the general half uses, and
//! asserts its output AGREES with xpile's hand-emitted PTX AND with the
//! reference `2*in + 1`. Two categorically-independent codegen paths (hand-
//! written PTX text vs the LLVM NVPTX back-end) agreeing on the GPU is a real
//! anti-correlation witness — a miscompile would have to corrupt both
//! identically.
//!
//! ## Toolchain posture (no new build lane)
//!
//! `rustc` is invoked as an EXTERNAL subprocess (like `nvcc`/`ptxas`/`wat2wasm`)
//! so xpile keeps building on stable — no nightly build, no `rustc-dev`, no new
//! Cargo dep. Gated on [`rustc_nvptx_available`] (nightly + the nvptx64 target)
//! AND [`cuda_toolchain_available`] (nvcc + nvidia-smi); absent → the CONSTRUCT
//! assertions still run and the EXECUTED half cleanly skips (free CI stays
//! green, never a false Match).

use std::process::Command;

use xpile_ptx_codegen::{
    cuda_toolchain_available, emit_kernel, emit_rustc_nvptx_ptx, rustc_nvptx_available,
    saxpy_kernel_fn, PtxDiffExecEngine, FIXTURE_INPUT,
};

/// The reference `out[i] = 2*in[i] + 1` over the fixture (the CPython/host
/// truth every emitter must reproduce).
fn reference() -> Vec<f64> {
    FIXTURE_INPUT
        .iter()
        .map(|x| 2.0 * (*x as f64) + 1.0)
        .collect()
}

/// The local GPU's compute capability (`sm_<maj><min>`), fallback `sm_80`.
fn local_sm() -> String {
    if let Ok(o) = Command::new("nvidia-smi")
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

/// nvcc-compile a `driver_harness` C source (host glue only — the kernel PTX is
/// embedded + JIT-assembled by the driver), run it on the GPU, parse the printed
/// f64 vector.
fn run_ptx_on_gpu(tag: &str, ptx: &str, arch: &str) -> Result<Vec<f64>, String> {
    let cu = PtxDiffExecEngine::driver_harness(ptx);
    let dir = std::env::temp_dir().join(format!(
        "xpile-rustc-nvptx-wit-{}-{}",
        tag,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("work dir: {e}"))?;
    let cu_path = dir.join(format!("{tag}.cu"));
    let bin = dir.join(tag);
    std::fs::write(&cu_path, &cu).map_err(|e| format!("write {tag}.cu: {e}"))?;
    let compile = Command::new("nvcc")
        .arg(format!("-arch={arch}"))
        .arg("-O2")
        .arg("-o")
        .arg(&bin)
        .arg("-lcuda")
        .arg(&cu_path)
        .output()
        .map_err(|e| format!("spawn nvcc: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "nvcc failed for {tag}:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        ));
    }
    let run = Command::new(&bin)
        .output()
        .map_err(|e| format!("spawn {tag}: {e}"))?;
    let stdout = String::from_utf8_lossy(&run.stdout);
    if !run.status.success() || stdout.trim_start().starts_with("ERR") {
        return Err(format!("{tag} GPU run failed: {stdout:?}"));
    }
    stdout
        .split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| format!("parse `{t}`: {e}")))
        .collect()
}

#[test]
fn rustc_nvptx_is_a_real_independent_emitter() {
    // CONSTRUCT (gated on the toolchain): the 3rd emitter produces genuine LLVM
    // NVPTX back-end PTX with the §29 entry point — categorically distinct from
    // xpile's hand-emitted text and nvcc's CUDA-C.
    if !rustc_nvptx_available() {
        eprintln!(
            "PMAT-997: skipping rustc-nvptx emitter witness — nightly rustc + \
             nvptx64-nvidia-cuda target absent. On a box with it, the 3rd \
             independent §29 emitter (LLVM NVPTX back-end) produces a \
             `.visible .entry xpile_kernel` PTX module."
        );
        return;
    }
    let rustc_ptx = match emit_rustc_nvptx_ptx() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("PMAT-997: rustc-nvptx emit unavailable ({e}) — skipping");
            return;
        }
    };
    assert!(
        rustc_ptx.contains("Generated by LLVM NVPTX Back-End"),
        "the 3rd emitter must be the LLVM NVPTX back-end:\n{rustc_ptx}"
    );
    assert!(rustc_ptx.contains(".visible .entry xpile_kernel"));

    // OFFLINE: ptxas assembles the rustc-nvptx PTX (the PTX analog of
    // wat2wasm-assembles-WAT), gated on ptxas.
    let sm = local_sm();
    let dir = std::env::temp_dir().join(format!("xpile-rustc-nvptx-asm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    let ptx_path = dir.join("k.ptx");
    std::fs::write(&ptx_path, &rustc_ptx).ok();
    if let Ok(asm) = Command::new("ptxas")
        .arg(format!("-arch={sm}"))
        .arg(&ptx_path)
        .arg("-o")
        .arg(dir.join("k.cubin"))
        .output()
    {
        assert!(
            asm.status.success(),
            "ptxas must assemble the rustc-nvptx PTX:\n{}",
            String::from_utf8_lossy(&asm.stderr)
        );
        eprintln!("PMAT-997: ptxas assembled the rustc-nvptx PTX for {sm}");
    }
}

#[test]
fn rustc_nvptx_executes_on_gpu_and_agrees_with_xpile_hand_emit() {
    if !rustc_nvptx_available() || !cuda_toolchain_available() {
        eprintln!(
            "PMAT-997: skipping EXECUTED 3-way anti-correlation witness — needs \
             nightly rustc + nvptx64 target AND nvcc + nvidia-smi. A CUDA box \
             runs the 3rd emitter's PTX (LLVM NVPTX back-end) vs xpile's \
             hand-emitted PTX on the GPU and asserts both agree AND == 2*in+1. \
             Free CI skips execution and stays green."
        );
        return;
    }

    let sm = local_sm();
    eprintln!(
        "PMAT-997: running EXECUTED 3rd-emitter (rustc-nvptx) anti-correlation witness on {sm}"
    );

    let rustc_ptx = emit_rustc_nvptx_ptx().expect("rustc-nvptx emits PTX");
    let xpile_ptx = emit_kernel(&saxpy_kernel_fn(), &sm).expect("xpile hand-emit PTX");
    assert!(
        xpile_ptx.contains(".visible .entry xpile_kernel"),
        "xpile hand-emit reference PTX shape"
    );

    let ref_out = reference();
    let rustc_out = run_ptx_on_gpu("rustc_nvptx", &rustc_ptx, &sm)
        .unwrap_or_else(|e| panic!("rustc-nvptx GPU run failed: {e}"));
    let xpile_out = run_ptx_on_gpu("xpile_hand", &xpile_ptx, &sm)
        .unwrap_or_else(|e| panic!("xpile hand-emit GPU run failed: {e}"));

    assert_eq!(rustc_out.len(), ref_out.len(), "rustc-nvptx output arity");
    assert_eq!(xpile_out.len(), ref_out.len(), "xpile output arity");

    let mut max_diff = 0.0_f64;
    for i in 0..ref_out.len() {
        // 3rd emitter agrees with the reference...
        assert!(
            (rustc_out[i] - ref_out[i]).abs() <= 1.0e-3,
            "rustc-nvptx[{i}]={} but reference 2*in+1={}",
            rustc_out[i],
            ref_out[i]
        );
        // ...AND with xpile's independent hand-emitted PTX (anti-correlation).
        let d = (rustc_out[i] - xpile_out[i]).abs();
        if d > max_diff {
            max_diff = d;
        }
    }
    assert!(
        max_diff <= 1.0e-3,
        "3rd emitter (rustc-nvptx) DIVERGED from xpile hand-emit on the GPU: max_abs_diff={max_diff}"
    );

    eprintln!(
        "PMAT-997: 3rd-EMITTER ANTI-CORRELATION witness PASSED on {sm} — nightly \
         rustc's LLVM NVPTX back-end PTX executed on the GPU (Driver API) and \
         agreed with xpile's INDEPENDENT hand-emitted PTX (max_abs_diff={max_diff}) \
         AND the reference 2*in+1. Three categorically-independent codegen \
         toolchains (hand-emit text / nvcc CUDA-C / rustc LLVM-NVPTX) now witness \
         the §29 kernel — strengthening the §14.10 anti-correlation guard."
    );
}
