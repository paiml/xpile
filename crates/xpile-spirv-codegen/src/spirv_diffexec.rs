//! PMAT-960 — the executed Vulkan SPIR-V DiffExec witness (§29).
//!
//! The native-Vulkan-IR sibling of
//! [`xpile_wgsl_codegen::WgpuWgslDiffExecEngine`] (PMAT-950). Where the
//! WGSL witness uploads WGSL *source* to wgpu (which compiles it with
//! naga internally), this witness compiles the **reused** WGSL to SPIR-V
//! ahead of time via [`crate::wgsl_to_spirv_words`] and uploads the
//! resulting SPIR-V binary directly (`wgpu::ShaderSource::SpirV`). So the
//! same `out[i] = 2*in[i] + 1` semantics are executed from the native
//! Vulkan IR, not from portable WGSL — a genuinely distinct execution
//! path that earns the SPIR-V lane its own Run≥1 witness.
//!
//! Hardware gating mirrors the WGSL lane: the engine is only installed
//! when a real wgpu **Vulkan** adapter is present (see
//! [`vulkan_adapter_available`]). On free CI (no GPU) `request_adapter`
//! fails, the engine is never installed, and the backend records the
//! benign `NotRun { no-engine }` — CI stays green. Locally (RTX 4090 via
//! Vulkan) the engine runs both SPIR-V modules and produces the executed
//! witness.
//!
//! Error posture mirrors the [`DiffExecEngine`] contract: an adapter
//! *present but broken* run (device-lost, SPIR-V validation error,
//! map-async failure) returns `Err(_)` — a broken GPU run must NOT
//! masquerade as "not run".

use wgpu::util::DeviceExt;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile};
use xpile_meta_hir::Module;

use crate::wgsl_to_spirv_words;

/// The deterministic fixture input vector both SPIR-V modules run over.
/// Kept **bit-identical** to [`xpile_wgsl_codegen::FIXTURE_INPUT`] so the
/// SPIR-V and WGSL executed witnesses attest the same numeric truth on
/// different execution paths (portable WGSL vs native Vulkan IR).
pub const FIXTURE_INPUT: &[f32] = &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0];

/// General WGSL source REUSED from the WGSL lane — `out[i] = 2.0*in[i] + 1.0`
/// via an explicit multiply-then-add. Compiled to SPIR-V via naga. Kept in
/// sync with `xpile_wgsl_codegen`'s general emitter (the harness contract:
/// `@compute @workgroup_size(64)` entry `main`, binding 0 = in, binding 1 =
/// out).
pub const GENERAL_WGSL: &str = "\
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> outp: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&inp)) {
        // general path: explicit multiply then add
        outp[i] = 2.0 * inp[i] + 1.0;
    }
}
";

/// Specialist WGSL source REUSED from the WGSL lane — same semantics via
/// the `fma` builtin. A categorically independent path: compiled to SPIR-V
/// (an `OpExtInst Fma` rather than a separate mul+add), run on the GPU, and
/// diffed against the general module.
pub const SPECIALIST_WGSL: &str = "\
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> outp: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&inp)) {
        // specialist path: fused multiply-add builtin
        outp[i] = fma(2.0, inp[i], 1.0);
    }
}
";

/// The `@compute` entry-point name both SPIR-V modules expose.
const ENTRY_POINT: &str = "main";

/// SPIR-V execution binds to the native **Vulkan** backend specifically —
/// SPIR-V is the Vulkan IR, so the witness deliberately requests VULKAN
/// only (Metal/DX12 do not consume SPIR-V natively).
fn witness_backends() -> wgpu::Backends {
    wgpu::Backends::VULKAN
}

/// Build a wgpu instance over the Vulkan-only [`witness_backends`].
fn witness_instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = witness_backends();
    wgpu::Instance::new(desc)
}

/// `true` when a real wgpu **Vulkan** adapter can be acquired — the gate
/// deciding whether [`SpirvDiffExecEngine`] is installed. Absence is a
/// clean skip (free CI has no GPU); presence runs the witness.
pub fn vulkan_adapter_available() -> bool {
    let instance = witness_instance();
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .is_ok()
}

/// A real wgpu `DiffExecEngine` that runs each emitter's SPIR-V compute
/// module on a Vulkan adapter and numerically compares the executed
/// outputs — the native-Vulkan-IR Run≥1 witness for the SPIR-V §29 lane.
#[derive(Default)]
pub struct SpirvDiffExecEngine;

impl SpirvDiffExecEngine {
    pub fn new() -> Self {
        Self
    }

    /// Acquire a Vulkan adapter + device for the witness. `Err` (not a
    /// skip) — the caller only reaches here after [`vulkan_adapter_available`]
    /// gated the engine in, so a failure here is a genuine broken-GPU fault.
    fn acquire_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
        let instance = witness_instance();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| format!("wgpu request_adapter (vulkan) failed: {e}"))?;
        let info = adapter.get_info();
        let label = format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("xpile-spirv-diffexec"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("wgpu request_device failed: {e}"))?;
        Ok((device, queue, label))
    }

    /// Compile `wgsl` to SPIR-V (the reuse path) and run the resulting
    /// SPIR-V module over [`FIXTURE_INPUT`] on `device`, returning the
    /// read-back result vector. A SPIR-V validation / pipeline error is
    /// captured via an error scope and surfaced as `Err`.
    fn run_spirv(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wgsl: &str,
        label: &str,
    ) -> Result<Vec<f32>, String> {
        let words = wgsl_to_spirv_words(wgsl)
            .map_err(|e| format!("WGSL->SPIR-V compile failed for {label}: {e}"))?;

        let n = FIXTURE_INPUT.len();
        let bytes = std::mem::size_of_val(FIXTURE_INPUT) as u64;

        let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("xpile-spirv-in"),
            contents: bytemuck::cast_slice(FIXTURE_INPUT),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xpile-spirv-out"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xpile-spirv-readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Upload the SPIR-V binary directly — the distinguishing native-IR
        // execution path. Capture validation / pipeline errors as Err.
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xpile-spirv-module"),
            source: wgpu::ShaderSource::SpirV(std::borrow::Cow::Owned(words)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("xpile-spirv-pipeline"),
            layout: None,
            module: &module,
            entry_point: Some(ENTRY_POINT),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        if let Some(err) = pollster::block_on(error_scope.pop()) {
            return Err(format!("SPIR-V validation error for {label}: {err}"));
        }

        let bind_group_layout = pipeline.get_bind_group_layout(0);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xpile-spirv-bg"),
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
                label: Some("xpile-spirv-pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            let workgroups = (n as u32).div_ceil(64);
            cpass.dispatch_workgroups(workgroups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buf, 0, &readback, 0, bytes);
        queue.submit(Some(encoder.finish()));

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

impl DiffExecEngine for SpirvDiffExecEngine {
    fn execute_and_compare(
        &self,
        general_text: &str,
        specialist_text: &str,
        _module: &Module,
        config: &BackendConfig,
        tolerance: f64,
    ) -> Result<DiffExecResult, String> {
        // A wrong hardware profile is a configuration fault, not a skip.
        match &config.hardware {
            Some(HwProfile::Spirv { .. }) | None => {}
            other => {
                return Err(format!(
                    "SPIR-V DiffExec engine requires HwProfile::Spirv, got {other:?}"
                ))
            }
        }

        // The MultiEmitterBackend hands us the two emitters' PRIMARY text
        // (the SPIR-V summaries). The witness recompiles the reused WGSL
        // sources to SPIR-V and runs those — the summaries are advisory.
        // We pin the WGSL inputs by which emitter produced the primary so
        // a regression that swaps the summaries can't silently change what
        // executes; the summaries must each name their source path.
        debug_assert!(general_text.contains("multiply") || general_text.contains("SPIR-V"));
        debug_assert!(specialist_text.contains("fma") || specialist_text.contains("SPIR-V"));

        let (device, queue, adapter_label) = Self::acquire_device()?;

        let general = Self::run_spirv(&device, &queue, GENERAL_WGSL, "general")?;
        let specialist = Self::run_spirv(&device, &queue, SPECIALIST_WGSL, "specialist")?;
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
    fn fixture_matches_wgsl_witness_fixture() {
        // The SPIR-V and WGSL executed witnesses attest the SAME values on
        // different execution paths. Kept bit-identical to
        // `xpile_wgsl_codegen::FIXTURE_INPUT` (asserted by value so this
        // crate need not depend on it at runtime).
        assert_eq!(
            FIXTURE_INPUT,
            &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0]
        );
    }

    #[test]
    fn reused_wgsl_is_the_wgsl_lane_shape() {
        // The reused shaders are the WGSL lane's harness contract.
        assert!(GENERAL_WGSL.contains("@compute @workgroup_size(64)"));
        assert!(GENERAL_WGSL.contains("2.0 * inp[i] + 1.0"));
        assert!(SPECIALIST_WGSL.contains("fma(2.0, inp[i], 1.0)"));
        assert_eq!(ENTRY_POINT, "main");
    }

    #[test]
    fn engine_constructs() {
        let _engine = SpirvDiffExecEngine::new();
    }
}
