//! PMAT-961 — the TRUE anti-correlation §29 PTX witness.
//!
//! Where PMAT-949's [`crate::NvccCudaDiffExecEngine`] diffs two CUDA-C kernels
//! that are BOTH compiled by the same `nvcc` (mul+add vs `fmaf`) — a strong
//! but *single-toolchain* check — this engine diffs two **categorically
//! independent codegen toolchains** for the SAME kernel on the GPU:
//!
//!   - **general**: xpile's OWN hand-emitted PTX text (from
//!     [`crate::emit::emit_kernel`]) loaded and JIT-assembled at runtime by the
//!     CUDA **Driver API** (`cuModuleLoadData` — the driver's embedded `ptxas`),
//!     then launched via `cuLaunchKernel`.
//!   - **specialist**: the nvcc-compiled CUDA-C `xpile_kernel` (the PMAT-949
//!     path) — `nvcc` → its own front-end → `ptxas` → SASS, launched via the
//!     CUDA **Runtime API** (`<<<>>>`).
//!
//! These two paths share NO codegen frontend: xpile emits PTX by hand, nvcc
//! emits PTX from C++. They agree only if BOTH lowerings are correct — exactly
//! the §29 anti-correlation property, upgrading the PTX lane from "two CUDA-C
//! kernels, same nvcc" to "two independent codegen paths". This is the PTX
//! analog of the wasm-runtime / wgpu / SPIR-V witnesses, run on real NVIDIA
//! silicon.
//!
//! ## Implementation — `nvcc`-compiled host harnesses, zero new Cargo deps
//!
//! Both halves are exercised through small C++ host harnesses compiled by
//! `nvcc` and run on the GPU (the same no-new-dependency posture as the WABT /
//! nvcc witnesses — `cargo deny check advisories` is unaffected):
//!
//!   - the **general** harness `#include <cuda.h>`, embeds xpile's PTX as a
//!     string, `cuModuleLoadData`s it, and launches `xpile_kernel` (linked
//!     against `-lcuda`, the driver library). The PTX is the artifact xpile
//!     emitted — nvcc only assembles the *host* glue, never the kernel.
//!   - the **specialist** harness is the existing PMAT-949 CUDA-C kernel +
//!     fixture harness ([`crate::NvccCudaDiffExecEngine::harness`]),
//!     nvcc-compiling the kernel itself.
//!
//! Both print the result vector over the bit-identical [`FIXTURE_INPUT`]; the
//! engine parses and numerically compares them. Gated on
//! [`crate::cuda_toolchain_available`]: absent on free CI → the engine is never
//! installed, the backend records the benign `NotRun { no-engine }`, CI stays
//! green. Present locally (RTX 4090 / sm_89) → the executed anti-correlation
//! witness runs.
//!
//! Error posture mirrors the [`DiffExecEngine`] contract: a *toolchain present
//! but broken* run (nvcc failure, driver load error, unparseable output)
//! returns `Err(_)` → a hard `BackendError`; a broken run must never
//! masquerade as "not run".

use std::path::PathBuf;
use std::process::Command;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile};
use xpile_meta_hir::Module;

use crate::cuda_diffexec::{NvccCudaDiffExecEngine, FIXTURE_INPUT};

/// A real anti-correlation CUDA `DiffExecEngine`: runs xpile's hand-emitted PTX
/// (via the CUDA Driver API) AND the nvcc-compiled CUDA-C kernel, then diffs
/// the executed outputs. This is the categorical-independence upgrade of the
/// §29 PTX lane (PMAT-961).
pub struct PtxDiffExecEngine {
    work_dir: PathBuf,
}

impl Default for PtxDiffExecEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PtxDiffExecEngine {
    pub fn new() -> Self {
        let work_dir =
            std::env::temp_dir().join(format!("xpile-ptx-diffexec-{}", std::process::id()));
        Self { work_dir }
    }

    /// `compute_capability` (e.g. `sm_89`) → the `nvcc -arch` flag. Derived,
    /// never hard-coded — the same honesty discipline as [`crate::ptxas_arch`].
    fn nvcc_arch(compute_capability: &str) -> String {
        format!("-arch={compute_capability}")
    }

    /// Wrap xpile's hand-emitted `ptx_src` in a CUDA **Driver API** host
    /// harness that `cuModuleLoadData`s the PTX, launches `xpile_kernel` over
    /// [`FIXTURE_INPUT`], and prints the result vector. This runs xpile's OWN
    /// PTX (the driver's embedded ptxas JITs it) — NOT nvcc-compiled CUDA-C.
    fn driver_harness(ptx_src: &str) -> String {
        let n = FIXTURE_INPUT.len();
        let inits: String = FIXTURE_INPUT
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        // The PTX is embedded with a raw string literal; xpile-emitted PTX
        // never contains the `)PTX"` delimiter sequence.
        format!(
            r#"#include <cstdio>
#include <cuda.h>

static const char* XPILE_PTX = R"PTX(
{ptx_src}
)PTX";

#define CK(x) do {{ CUresult e = (x); if (e != CUDA_SUCCESS) {{ const char* s = nullptr; cuGetErrorString(e, &s); printf("ERR %s: %s\n", #x, s ? s : "?"); return 3; }} }} while(0)

int main() {{
    const int n = {n};
    double h_in[n] = {{ {inits} }};
    double h_out[n];
    CK(cuInit(0));
    CUdevice dev; CK(cuDeviceGet(&dev, 0));
    CUcontext ctx; CK(cuCtxCreate(&ctx, 0, dev));
    CUmodule mod; CK(cuModuleLoadData(&mod, XPILE_PTX));
    CUfunction fn; CK(cuModuleGetFunction(&fn, mod, "xpile_kernel"));
    CUdeviceptr d_in, d_out;
    CK(cuMemAlloc(&d_in, n * sizeof(double)));
    CK(cuMemAlloc(&d_out, n * sizeof(double)));
    CK(cuMemcpyHtoD(d_in, h_in, n * sizeof(double)));
    int nn = n;
    void* args[] = {{ &d_in, &d_out, &nn }};
    CK(cuLaunchKernel(fn, 1,1,1, 256,1,1, 0, 0, args, 0));
    CK(cuCtxSynchronize());
    CK(cuMemcpyDtoH(h_out, d_out, n * sizeof(double)));
    for (int i = 0; i < n; i++) {{
        printf("%.17g", h_out[i]);
        if (i + 1 < n) printf(" ");
    }}
    printf("\n");
    cuMemFree(d_in);
    cuMemFree(d_out);
    return 0;
}}
"#
        )
    }

    /// nvcc-compile `src` (with the extra args, e.g. `-lcuda`) into `bin_name`,
    /// run it on the GPU, parse the printed float vector.
    fn compile_run_parse(
        &self,
        src: &str,
        arch_flag: &str,
        bin_name: &str,
        extra: &[&str],
    ) -> Result<Vec<f64>, String> {
        std::fs::create_dir_all(&self.work_dir).map_err(|e| format!("create work dir: {e}"))?;
        let cu_path = self.work_dir.join(format!("{bin_name}.cu"));
        let bin_path = self.work_dir.join(bin_name);
        std::fs::write(&cu_path, src).map_err(|e| format!("write {bin_name}.cu: {e}"))?;

        let mut cmd = Command::new("nvcc");
        cmd.arg(arch_flag).arg("-O2").arg("-o").arg(&bin_path);
        for a in extra {
            cmd.arg(a);
        }
        cmd.arg(&cu_path);
        let compile = cmd.output().map_err(|e| format!("spawn nvcc: {e}"))?;
        if !compile.status.success() {
            return Err(format!(
                "nvcc failed ({arch_flag}) for {bin_name}:\n{}",
                String::from_utf8_lossy(&compile.stderr)
            ));
        }

        let run = Command::new(&bin_path)
            .output()
            .map_err(|e| format!("spawn {bin_name} binary: {e}"))?;
        let stdout = String::from_utf8_lossy(&run.stdout);
        if !run.status.success() {
            return Err(format!(
                "{bin_name} GPU run exited non-zero: stdout={stdout:?} stderr={:?}",
                String::from_utf8_lossy(&run.stderr)
            ));
        }
        let line = stdout.trim();
        if let Some(rest) = line.strip_prefix("ERR") {
            return Err(format!("{bin_name} reported device error:{rest}"));
        }
        line.split_whitespace()
            .map(|tok| {
                tok.parse::<f64>()
                    .map_err(|e| format!("parse float `{tok}` from {bin_name}: {e}"))
            })
            .collect()
    }
}

impl DiffExecEngine for PtxDiffExecEngine {
    /// `general_text` is xpile's hand-emitted PTX; `specialist_text` is the
    /// nvcc-compilable CUDA-C kernel (the PMAT-949 specialist). The two are run
    /// through categorically-independent toolchains and numerically compared.
    fn execute_and_compare(
        &self,
        general_text: &str,
        specialist_text: &str,
        _module: &Module,
        config: &BackendConfig,
        tolerance: f64,
    ) -> Result<DiffExecResult, String> {
        let compute_capability =
            match &config.hardware {
                Some(HwProfile::Ptx { compute_capability }) => compute_capability.as_str(),
                _ => return Err(
                    "PTX anti-correlation engine requires HwProfile::Ptx { compute_capability }"
                        .to_string(),
                ),
            };
        let arch = Self::nvcc_arch(compute_capability);

        // general = xpile's OWN hand-emitted PTX, via the CUDA Driver API
        // (links the driver lib `-lcuda`; the PTX is JIT-assembled by the
        // driver, NOT nvcc-compiled from C++).
        let xpile_out = self.compile_run_parse(
            &Self::driver_harness(general_text),
            &arch,
            "xpile_ptx_driver",
            &["-lcuda"],
        )?;

        // specialist = the nvcc-compiled CUDA-C kernel (the PMAT-949 path),
        // through nvcc's own C++ front-end + ptxas.
        let nvcc_out = self.compile_run_parse(
            &NvccCudaDiffExecEngine::harness(specialist_text),
            &arch,
            "nvcc_cuda_c",
            &[],
        )?;

        if xpile_out.len() != nvcc_out.len() {
            return Ok(DiffExecResult::Divergent {
                max_abs_diff: f64::INFINITY,
                tolerance,
            });
        }
        let max_abs_diff = xpile_out
            .iter()
            .zip(nvcc_out.iter())
            .map(|(g, s)| (g - s).abs())
            .fold(0.0_f64, f64::max);

        if max_abs_diff <= tolerance {
            Ok(DiffExecResult::Match { max_abs_diff })
        } else {
            Ok(DiffExecResult::Divergent {
                max_abs_diff,
                tolerance,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvcc_arch_is_derived_not_hardcoded() {
        assert_eq!(PtxDiffExecEngine::nvcc_arch("sm_89"), "-arch=sm_89");
        assert_eq!(PtxDiffExecEngine::nvcc_arch("sm_120"), "-arch=sm_120");
    }

    #[test]
    fn driver_harness_embeds_ptx_and_fixture() {
        let h = PtxDiffExecEngine::driver_harness(".version 8.0\n.target sm_89\n");
        // The PTX is embedded verbatim.
        assert!(h.contains(".version 8.0"));
        // It loads via the Driver API (NOT nvcc-compiled CUDA-C `<<<>>>`).
        assert!(h.contains("cuModuleLoadData"));
        assert!(h.contains("cuLaunchKernel"));
        assert!(h.contains("xpile_kernel"));
        // The fixture arity is reflected.
        assert!(h.contains(&format!("const int n = {};", FIXTURE_INPUT.len())));
    }
}
