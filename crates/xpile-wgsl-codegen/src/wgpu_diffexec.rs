//! PMAT-950 — the FIRST real *executed* cross-vendor GPU DiffExec
//! witness for the WGSL lane (§29).
//!
//! Sibling of [`xpile_ptx_codegen::NvccCudaDiffExecEngine`] (PMAT-949,
//! the NVIDIA-only CUDA witness). Where the CUDA engine runs through
//! `nvcc` (one vendor toolchain), this one runs through **wgpu** — so
//! the same `out[i] = 2*in[i] + 1` semantics are diffed on whatever
//! adapter the host exposes: **Vulkan** (the RTX 4090 / AMD Navi10),
//! **Metal** (Apple), or **DX12** (Windows). That makes the WGSL
//! `DiffExec` lane a genuinely *cross-vendor* executed witness, not a
//! single-toolchain one.
//!
//! Until this slice the §29 Multi-Emitter Oracle Quorum recorded
//! [`DiffExecResult::NotRun { reason: no-engine }`] for the WGSL backend
//! under `QuorumPolicy::DiffExec` — no emitted WGSL was ever *run* on a
//! real adapter (the `crates/xpile-wgsl-codegen/src/lib.rs` PMAT-482
//! comment marked this "the on-hardware AMD-Vulkan `DiffExec` slice,
//! PMAT-490"). This module ships [`WgpuWgslDiffExecEngine`], a real
//! [`DiffExecEngine`] that:
//!
//!   1. takes the two emitters' WGSL compute-shader sources (general +
//!      specialist),
//!   2. acquires a wgpu adapter + device for the requested backends,
//!   3. uploads [`FIXTURE_INPUT`] to a storage buffer, dispatches each
//!      shader, and copies the result back,
//!   4. **runs both shaders on the GPU**,
//!   5. compares the read-back float vectors within the contract's
//!      tolerance, returning a real [`DiffExecResult::Match`] /
//!      [`DiffExecResult::Divergent`].
//!
//! The fixture is identical to the CUDA witness's
//! [`xpile_ptx_codegen::FIXTURE_INPUT`] so the two lanes attest the same
//! numeric truth on different stacks.
//!
//! Hardware gating: the engine is only installed when a wgpu adapter is
//! actually present (see [`wgpu_adapter_available`]). On free CI (no GPU)
//! `request_adapter` fails, the engine is never installed, and the
//! backend records the benign `NotRun { no-engine }` — the cc/python3 /
//! `nvcc` graceful-skip posture, so CI stays green. Locally (RTX 4090 via
//! Vulkan, …) the engine runs and produces the executed witness.
//!
//! Error posture mirrors the [`DiffExecEngine`] contract: an adapter
//! *present but broken* run (device-lost, WGSL validation error,
//! map-async failure) returns `Err(_)`, which the backend turns into a
//! hard `BackendError` — a broken GPU run must NOT masquerade as
//! "not run".

use wgpu::util::DeviceExt;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile};
use xpile_meta_hir::Module;

/// The deterministic fixture input vector both shaders run over. Kept
/// **bit-identical** to [`xpile_ptx_codegen::FIXTURE_INPUT`] so the WGSL
/// and CUDA executed witnesses attest the same values; exercises
/// negatives, zero, a fraction, and a larger magnitude.
pub const FIXTURE_INPUT: &[f32] = &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0];

/// The cross-vendor backend set both [`wgpu_adapter_available`] and
/// [`WgpuWgslDiffExecEngine`] request. `VULKAN | METAL | DX12` matches
/// the `wgpu` feature set declared in `Cargo.toml`; it deliberately
/// omits GL/WebGPU so the witness runs on a real native compute adapter.
fn witness_backends() -> wgpu::Backends {
    wgpu::Backends::VULKAN | wgpu::Backends::METAL | wgpu::Backends::DX12
}

/// Build a wgpu instance over the cross-vendor [`witness_backends`].
///
/// `Instance::new` only panics when *no* backend feature is enabled for
/// the target platform; since `Cargo.toml` enables vulkan/metal/dx12 and
/// CI's Linux runner has the `vulkan` feature compiled in, the call is
/// panic-free even with no GPU present (it just yields an instance whose
/// `request_adapter` then fails cleanly).
fn witness_instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = witness_backends();
    wgpu::Instance::new(desc)
}

/// `true` when a real wgpu adapter can be acquired — the gate that
/// decides whether [`WgpuWgslDiffExecEngine`] should be installed.
/// Mirrors [`xpile_ptx_codegen::cuda_toolchain_available`] /
/// the cc-availability graceful-skip pattern: absence is a clean skip
/// (free CI has no GPU), presence runs the witness (local GPU box).
pub fn wgpu_adapter_available() -> bool {
    let instance = witness_instance();
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .is_ok()
}

/// The `@compute` entry-point name both emitters' WGSL must define, and
/// the storage-buffer binding contract the harness drives:
///   `@group(0) @binding(0) var<storage, read>       …: array<f32>` (in)
///   `@group(0) @binding(1) var<storage, read_write>  …: array<f32>` (out)
const ENTRY_POINT: &str = "main";

/// A real wgpu `DiffExecEngine`: runs each emitter's WGSL compute shader
/// on a GPU adapter and numerically compares the executed outputs. This
/// is the cross-vendor executed Run≥1 witness for the WGSL §29 lane.
#[derive(Default)]
pub struct WgpuWgslDiffExecEngine;

impl WgpuWgslDiffExecEngine {
    pub fn new() -> Self {
        Self
    }

    /// Acquire an adapter + device for the witness backends. Returns
    /// `Err` (not a skip) — the caller only reaches here after
    /// [`wgpu_adapter_available`] gated the engine in, so a failure here
    /// is a genuine broken-GPU fault, per the [`DiffExecEngine`] error
    /// posture.
    fn acquire_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
        let instance = witness_instance();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| format!("wgpu request_adapter failed: {e}"))?;
        let info = adapter.get_info();
        let label = format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("xpile-wgsl-diffexec"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("wgpu request_device failed: {e}"))?;
        Ok((device, queue, label))
    }

    /// Run one WGSL compute shader over [`FIXTURE_INPUT`] on `device`,
    /// returning the read-back result vector. A WGSL validation error is
    /// captured via an error scope and surfaced as `Err` rather than the
    /// default uncaptured-error panic.
    fn run_wgsl(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shader_src: &str,
        label: &str,
    ) -> Result<Vec<f32>, String> {
        let n = FIXTURE_INPUT.len();
        let bytes = std::mem::size_of_val(FIXTURE_INPUT) as u64;

        let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("xpile-wgsl-in"),
            contents: bytemuck::cast_slice(FIXTURE_INPUT),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xpile-wgsl-out"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xpile-wgsl-readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Capture WGSL validation / pipeline errors as Err, not a panic.
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xpile-wgsl-module"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("xpile-wgsl-pipeline"),
            layout: None,
            module: &module,
            entry_point: Some(ENTRY_POINT),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        if let Some(err) = pollster::block_on(error_scope.pop()) {
            return Err(format!("WGSL validation error for {label}: {err}"));
        }

        let bind_group_layout = pipeline.get_bind_group_layout(0);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xpile-wgsl-bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buf.as_entire_binding(),
                },
            ],
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("xpile-wgsl-pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            // workgroup_size(64); one invocation per element, rounded up.
            let workgroups = (n as u32).div_ceil(64);
            cpass.dispatch_workgroups(workgroups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buf, 0, &readback, 0, bytes);
        queue.submit(Some(encoder.finish()));

        // Map the readback buffer and block until the GPU is done.
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("wgpu poll failed for {label}: {e}"))?;
        rx.recv()
            .map_err(|e| format!("map channel closed for {label}: {e}"))?
            .map_err(|e| format!("buffer map_async failed for {label}: {e}"))?;
        let data = slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        readback.unmap();
        Ok(result)
    }
}

impl DiffExecEngine for WgpuWgslDiffExecEngine {
    fn execute_and_compare(
        &self,
        general_text: &str,
        specialist_text: &str,
        _module: &Module,
        config: &BackendConfig,
        tolerance: f64,
    ) -> Result<DiffExecResult, String> {
        // The WGSL DiffExec engine requires the WGSL hardware profile —
        // a wrong profile is a configuration fault, not a skip.
        match &config.hardware {
            Some(HwProfile::Wgsl { .. }) | None => {}
            other => {
                return Err(format!(
                    "wgpu DiffExec engine requires HwProfile::Wgsl, got {other:?}"
                ))
            }
        }

        let (device, queue, adapter_label) = Self::acquire_device()?;

        let general = Self::run_wgsl(&device, &queue, general_text, "general")?;
        let specialist = Self::run_wgsl(&device, &queue, specialist_text, "specialist")?;
        let _ = adapter_label; // available for tracing; not part of the vote.

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
    fn fixture_matches_cuda_witness_fixture() {
        // The WGSL and CUDA executed witnesses must attest the SAME
        // values on different stacks. Kept bit-identical to
        // `xpile_ptx_codegen::FIXTURE_INPUT` (asserted by value here so
        // this crate needn't depend on the PTX codegen crate).
        assert_eq!(
            FIXTURE_INPUT,
            &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0]
        );
    }

    #[test]
    fn entry_point_is_the_harness_contract() {
        // The emitters and harness agree on the `main` entry point.
        assert_eq!(ENTRY_POINT, "main");
    }

    #[test]
    fn engine_constructs() {
        // Pure-CPU smoke: building the engine never touches a GPU.
        let _engine = WgpuWgslDiffExecEngine::new();
    }
}
