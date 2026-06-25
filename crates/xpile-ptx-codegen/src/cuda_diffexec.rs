//! PMAT-949 — the FIRST real *executed* GPU DiffExec witness (§29).
//!
//! Closes the long-standing audit-design.md §4 / §62 "Run=1 / single
//! demo-fixture, DiffExecResult::NotRun" caveat for
//! `C-COMPILE-RUST-TO-PTX-MMA`. Until this slice the §29 Multi-Emitter
//! Oracle Quorum recorded [`DiffExecResult::NotRun { no-engine }`] under
//! `QuorumPolicy::DiffExec` — no emitter output was ever *run* on real
//! hardware. This module ships [`NvccCudaDiffExecEngine`], a genuine
//! [`DiffExecEngine`] that:
//!
//!   1. takes the two emitters' CUDA-C kernel sources (general +
//!      specialist),
//!   2. wraps each in a deterministic host harness over a fixed fixture
//!      input vector,
//!   3. `nvcc`-compiles each for the **local** compute capability
//!      (derived from the [`HwProfile::Ptx`] `compute_capability`, never
//!      hard-coded — same discipline as PMAT-481's `ptxas_arch`),
//!   4. **runs both binaries on the GPU**,
//!   5. parses the emitted float vectors and compares them within the
//!      contract's tolerance, returning a real
//!      [`DiffExecResult::Match`] / [`DiffExecResult::Divergent`].
//!
//! Hardware gating: the engine is only installed when `nvcc` +
//! `nvidia-smi` are present (see [`cuda_toolchain_available`]). On free
//! CI (no GPU) the engine is never installed, so the backend records the
//! benign `NotRun { no-engine }` and CI stays green — the
//! cc/python3 graceful-skip posture. Locally (RTX 4090 / sm_89, GB10 /
//! sm_121) the engine runs and produces the executed witness.
//!
//! Error posture mirrors the [`DiffExecEngine`] contract: a *toolchain
//! present but broken* run (nvcc failure, launch error, unparseable
//! output) returns `Err(_)`, which the backend turns into a hard
//! `BackendError` — a broken GPU run must NOT masquerade as "not run".

use std::path::PathBuf;
use std::process::Command;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile};
use xpile_meta_hir::Module;

/// The deterministic fixture input vector both kernels run over. Fixed
/// so the witness is reproducible; the values exercise negatives, zero,
/// a fraction, and a larger magnitude.
pub const FIXTURE_INPUT: &[f32] = &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0];

/// `true` when both `nvcc` and `nvidia-smi` are invocable — the gate
/// that decides whether [`NvccCudaDiffExecEngine`] should be installed.
/// Mirrors the `have_python_and_rustc` / cc-availability graceful-skip
/// pattern: absence is a clean skip (CI), presence runs the witness
/// (local GPU box).
pub fn cuda_toolchain_available() -> bool {
    let nvcc = Command::new("nvcc").arg("--version").output().is_ok();
    let smi = Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    nvcc && smi
}

/// A real CUDA `DiffExecEngine`: nvcc-compiles each emitter's CUDA-C
/// kernel, runs both on the GPU, and numerically compares the outputs.
/// This is the executed Run≥1 witness for `C-COMPILE-RUST-TO-PTX-MMA`.
pub struct NvccCudaDiffExecEngine {
    /// Working directory for emitted `.cu` sources + binaries. Defaults
    /// to a unique subdir of the system temp dir.
    work_dir: PathBuf,
}

impl Default for NvccCudaDiffExecEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl NvccCudaDiffExecEngine {
    pub fn new() -> Self {
        // Unique per-process dir so parallel test runs don't collide.
        let work_dir =
            std::env::temp_dir().join(format!("xpile-cuda-diffexec-{}", std::process::id()));
        Self { work_dir }
    }

    /// `compute_capability` (e.g. `sm_89`) → the `nvcc -arch` flag.
    /// Derived from the requested capability, never hard-coded — the
    /// same honesty discipline as [`crate::ptxas_arch`].
    fn nvcc_arch(compute_capability: &str) -> String {
        format!("-arch={compute_capability}")
    }

    /// Wrap a bare `__global__` kernel source in a deterministic host
    /// harness that runs it over [`FIXTURE_INPUT`] and prints the result
    /// vector as space-separated floats on one line.
    ///
    /// The kernel source MUST define exactly:
    /// `__global__ void xpile_kernel(const float* in, float* out, int n)`.
    ///
    /// `pub(crate)` so the PMAT-961 [`crate::PtxDiffExecEngine`] reuses this
    /// exact CUDA-C fixture harness for the nvcc (specialist) half of the
    /// anti-correlation diff.
    pub(crate) fn harness(kernel_src: &str) -> String {
        let n = FIXTURE_INPUT.len();
        let inits: String = FIXTURE_INPUT
            .iter()
            .map(|v| format!("{v:?}f"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"#include <cstdio>

{kernel_src}

int main() {{
    const int n = {n};
    float h_in[n] = {{ {inits} }};
    float h_out[n];
    float *d_in = nullptr, *d_out = nullptr;
    if (cudaMalloc(&d_in, n * sizeof(float)) != cudaSuccess) {{ printf("ERR malloc in\n"); return 2; }}
    if (cudaMalloc(&d_out, n * sizeof(float)) != cudaSuccess) {{ printf("ERR malloc out\n"); return 2; }}
    if (cudaMemcpy(d_in, h_in, n * sizeof(float), cudaMemcpyHostToDevice) != cudaSuccess) {{ printf("ERR memcpy h2d\n"); return 2; }}
    int threads = 256;
    int blocks = (n + threads - 1) / threads;
    xpile_kernel<<<blocks, threads>>>(d_in, d_out, n);
    cudaError_t err = cudaDeviceSynchronize();
    if (err != cudaSuccess) {{ printf("ERR launch %s\n", cudaGetErrorString(err)); return 3; }}
    if (cudaMemcpy(h_out, d_out, n * sizeof(float), cudaMemcpyDeviceToHost) != cudaSuccess) {{ printf("ERR memcpy d2h\n"); return 2; }}
    for (int i = 0; i < n; i++) {{
        printf("%.7g", h_out[i]);
        if (i + 1 < n) printf(" ");
    }}
    printf("\n");
    cudaFree(d_in);
    cudaFree(d_out);
    return 0;
}}
"#
        )
    }

    /// Compile `kernel_src` for `arch_flag` into `bin_name`, run it on
    /// the GPU, and parse the printed float vector.
    fn compile_run_parse(
        &self,
        kernel_src: &str,
        arch_flag: &str,
        bin_name: &str,
    ) -> Result<Vec<f32>, String> {
        std::fs::create_dir_all(&self.work_dir).map_err(|e| format!("create work dir: {e}"))?;
        let cu_path = self.work_dir.join(format!("{bin_name}.cu"));
        let bin_path = self.work_dir.join(bin_name);
        std::fs::write(&cu_path, Self::harness(kernel_src))
            .map_err(|e| format!("write {bin_name}.cu: {e}"))?;

        let compile = Command::new("nvcc")
            .arg(arch_flag)
            .arg("-O2")
            .arg("-o")
            .arg(&bin_path)
            .arg(&cu_path)
            .output()
            .map_err(|e| format!("spawn nvcc: {e}"))?;
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
                tok.parse::<f32>()
                    .map_err(|e| format!("parse float `{tok}` from {bin_name}: {e}"))
            })
            .collect()
    }
}

impl DiffExecEngine for NvccCudaDiffExecEngine {
    fn execute_and_compare(
        &self,
        general_text: &str,
        specialist_text: &str,
        _module: &Module,
        config: &BackendConfig,
        tolerance: f64,
    ) -> Result<DiffExecResult, String> {
        let compute_capability = match &config.hardware {
            Some(HwProfile::Ptx { compute_capability }) => compute_capability.as_str(),
            _ => {
                return Err(
                    "CUDA DiffExec engine requires HwProfile::Ptx { compute_capability }"
                        .to_string(),
                )
            }
        };
        let arch = Self::nvcc_arch(compute_capability);

        let general = self.compile_run_parse(general_text, &arch, "general")?;
        let specialist = self.compile_run_parse(specialist_text, &arch, "specialist")?;

        if general.len() != specialist.len() {
            return Ok(DiffExecResult::Divergent {
                max_abs_diff: f64::INFINITY,
                tolerance,
            });
        }
        let max_abs_diff = general
            .iter()
            .zip(specialist.iter())
            .map(|(g, s)| ((*g as f64) - (*s as f64)).abs())
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
        assert_eq!(NvccCudaDiffExecEngine::nvcc_arch("sm_89"), "-arch=sm_89");
        assert_eq!(NvccCudaDiffExecEngine::nvcc_arch("sm_121"), "-arch=sm_121");
    }

    #[test]
    fn harness_embeds_fixture_and_kernel() {
        let h = NvccCudaDiffExecEngine::harness("__global__ void xpile_kernel() {}");
        assert!(h.contains("xpile_kernel<<<"));
        assert!(h.contains("cudaDeviceSynchronize"));
        // arity of the fixture is reflected in the harness `n`.
        assert!(h.contains(&format!("const int n = {};", FIXTURE_INPUT.len())));
    }
}
