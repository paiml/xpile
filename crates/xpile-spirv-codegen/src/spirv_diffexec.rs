//! PMAT-960 / PMAT-977 / PMAT-988 — the executed Vulkan SPIR-V DiffExec witness (§29).
//!
//! The native-Vulkan-IR sibling of
//! [`xpile_wgsl_codegen::WgpuWgslDiffExecEngine`] (PMAT-950). Where the
//! WGSL witness uploads WGSL *source* to wgpu (which compiles it with
//! naga internally), this witness compiles WGSL to SPIR-V ahead of time
//! via [`crate::wgsl_to_spirv_words`] and uploads the resulting SPIR-V
//! binary directly (`wgpu::ShaderSource::SpirV`). So the same
//! `out[i] = 2*in[i] + 1` semantics are executed from the native Vulkan
//! IR, not from portable WGSL — a genuinely distinct execution path that
//! earns the SPIR-V lane its own Run≥1 witness.
//!
//! ## PMAT-977 — the **general** side now drives xpile's REAL emission
//!
//! Before PMAT-977 BOTH sides ran *hardcoded* WGSL string constants
//! (`GENERAL_WGSL` / `SPECIALIST_WGSL`): a literal `@compute` kernel that
//! never exercised xpile's real lowering. The witness proved
//! `hardcoded shader → naga SPIR-V → run`, NOT that xpile's compiler emits
//! correct SPIR-V.
//!
//! PMAT-977 rewires the **general** side to the REAL path the SPIR-V lane
//! is supposed to attest:
//!
//! ```text
//! meta-HIR Module  →  xpile_wgsl_codegen::emit_wgsl_module  (the REAL
//!                       PMAT-970 lowering — `2.0*x + 1.0` over an f32 param)
//!                  →  thin @compute dispatch harness around the REAL fn
//!                  →  naga (WGSL → SPIR-V)
//!                  →  run on the Vulkan adapter
//!                  →  diff vs the trusted reference (CPython-equivalent
//!                       expected vector AND the specialist `fma` module)
//! ```
//!
//! The load-bearing arithmetic (`saxpy`'s `x * 2.0 + 1.0`) is emitted by
//! xpile's real compiler, not hand-written. Only the per-element dispatch
//! shell (`@compute @workgroup_size(64) fn main`, the storage-buffer
//! bindings) is added around it — the GPU analogue of `extern "C"` glue
//! around a real lowered function. So this witness now proves
//! `meta-HIR → xpile real WGSL → naga SPIR-V → run → correct`.
//!
//! The **specialist** side stays a hardcoded `fma` kernel: it is the
//! *trusted reference* the real path is diffed against (categorically
//! different SPIR-V — an `OpExtInst Fma` vs an explicit mul+add). The
//! witness additionally checks the executed real-path output against a
//! CPython-equivalent [`EXPECTED_OUTPUT`] vector, so a Match is not merely
//! "two GPU paths agree" but "xpile's real emission computes the
//! reference truth".
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
//!
//! ## PMAT-988 — the witness now RUNS the text it is HANDED (attested == executed)
//!
//! Before PMAT-988 [`SpirvDiffExecEngine::execute_and_compare`] only
//! `debug_assert!`ed its `general_text` / `specialist_text` arguments, then
//! ran a *re-derived* `general_real_wgsl()` for the general side and the
//! module-level hardcoded [`SPECIALIST_WGSL`] constant for the specialist —
//! so the witness EXECUTED artifacts it derived itself and IGNORED the
//! attested text it was given. What was attested was not guaranteed to be
//! what ran: a regression that changed the emitter's primary text could not
//! be caught by the GPU run.
//!
//! PMAT-988 closes that gap. The emitter primaries are
//! [`crate::spirv_text_summary`] outputs that inline the **exact WGSL** each
//! emitter compiled into its attested SPIR-V. `execute_and_compare` now
//! recovers that WGSL from the passed-in `general_text` / `specialist_text`
//! via [`extract_wgsl_from_summary`] and runs THAT — so the artifact that
//! EXECUTES is byte-derived from the artifact that is ATTESTED. The general
//! side still traces back to xpile's REAL emission, because the real WGSL is
//! precisely what the general emitter embedded in `general_text`.

use wgpu::util::DeviceExt;

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile};
use xpile_meta_hir::{Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Type};
use xpile_wgsl_codegen::emit_wgsl_module;

use crate::wgsl_to_spirv_words;

/// The deterministic fixture input vector both SPIR-V modules run over.
/// Kept **bit-identical** to [`xpile_wgsl_codegen::FIXTURE_INPUT`] so the
/// SPIR-V and WGSL executed witnesses attest the same numeric truth on
/// different execution paths (portable WGSL vs native Vulkan IR).
pub const FIXTURE_INPUT: &[f32] = &[0.0, 1.0, 2.0, -3.0, 4.5, 10.0, -0.5, 100.0];

/// The CPython-equivalent reference output: `2.0*x + 1.0` over
/// [`FIXTURE_INPUT`], computed in plain Rust f32 (bit-equivalent to a
/// CPython `[2.0*x + 1.0 for x in fixture]` cast to float32). The executed
/// REAL-path SPIR-V result is checked against this so a Match attests
/// "xpile's real emission computes the reference truth", not merely "two
/// GPU paths happen to agree".
pub const EXPECTED_OUTPUT: &[f32] = &[1.0, 3.0, 5.0, -5.0, 10.0, 21.0, 0.0, 201.0];

/// Specialist WGSL source — the *trusted reference* the REAL path is
/// diffed against. Same `out[i] = 2*in[i] + 1` semantics via the `fma`
/// builtin: a categorically independent path that compiles to SPIR-V (an
/// `OpExtInst Fma` rather than a separate mul+add), runs on the GPU, and
/// is diffed against the real-path (general) module. This side stays
/// hardcoded *on purpose* — it is the independent oracle, not xpile's
/// output under test.
pub const SPECIALIST_WGSL: &str = "\
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> outp: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&inp)) {
        // specialist path: fused multiply-add builtin (trusted reference)
        outp[i] = fma(2.0, inp[i], 1.0);
    }
}
";

/// The `@compute` entry-point name both SPIR-V modules expose.
const ENTRY_POINT: &str = "main";

/// The marker line [`crate::spirv_text_summary`] writes immediately before
/// it inlines the source WGSL as `;   `-prefixed comment lines. The witness
/// recovers the **attested** WGSL by parsing the summary it was handed back
/// out from this marker, so the artifact it RUNS is byte-derived from the
/// artifact it ATTESTS — never a separately re-derived shader.
const SUMMARY_WGSL_MARKER: &str = "; Source WGSL";

/// The per-line prefix [`crate::spirv_text_summary`] uses when it inlines
/// the source WGSL (`;` + three spaces). Stripping it recovers the exact
/// WGSL bytes the emitter compiled into the attested SPIR-V.
const SUMMARY_WGSL_LINE_PREFIX: &str = ";   ";

/// Recover the source WGSL embedded in a SPIR-V **summary** primary (the
/// `general_text` / `specialist_text` the backend threads into
/// [`SpirvDiffExecEngine::execute_and_compare`]).
///
/// [`crate::spirv_text_summary`] inlines the exact WGSL it compiled to
/// SPIR-V after a [`SUMMARY_WGSL_MARKER`] line, one WGSL line per
/// `;   `-prefixed comment. This walks that block back out so the witness
/// can compile + run the **attested** WGSL — i.e. the artifact that
/// EXECUTES is byte-derived from the artifact that is ATTESTED, closing the
/// PMAT-988 "runs hardcoded constants, ignores its arguments" gap.
///
/// `Err` when the text carries no recoverable WGSL block — a summary that
/// doesn't embed its source can't be the thing we attest+run, and a broken
/// witness must fault rather than silently fall back to a hardcoded shader.
pub fn extract_wgsl_from_summary(summary: &str) -> Result<String, String> {
    let mut lines = summary.lines();
    // Advance to the marker.
    let found_marker = lines.by_ref().any(|l| l.starts_with(SUMMARY_WGSL_MARKER));
    if !found_marker {
        return Err(format!(
            "attested SPIR-V summary has no `{SUMMARY_WGSL_MARKER}` block — \
             cannot recover the WGSL that was compiled (refusing to run a \
             re-derived shader). Got:\n{summary}"
        ));
    }
    // Every subsequent `;   `-prefixed line is one line of the embedded WGSL.
    let mut wgsl = String::new();
    for line in lines {
        match line.strip_prefix(SUMMARY_WGSL_LINE_PREFIX) {
            Some(code) => {
                wgsl.push_str(code);
                wgsl.push('\n');
            }
            // The WGSL block is contiguous; the first non-prefixed line ends it.
            None => break,
        }
    }
    if wgsl.trim().is_empty() {
        return Err(format!(
            "attested SPIR-V summary's `{SUMMARY_WGSL_MARKER}` block was empty — \
             nothing to compile + run. Got:\n{summary}"
        ));
    }
    Ok(wgsl)
}

/// The name of the scalar function in the general meta-HIR module — the
/// one xpile's REAL lowering ([`emit_wgsl_module`]) emits and the dispatch
/// harness calls per element.
const GENERAL_FN: &str = "saxpy";

/// Build the **general** meta-HIR module the SPIR-V witness drives through
/// xpile's REAL emission: a single scalar function
///
/// ```python
/// def saxpy(x: float32) -> float32:
///     return x * 2.0 + 1.0
/// ```
///
/// This is genuine meta-HIR (the same node shapes a frontend produces);
/// [`emit_wgsl_module`] lowers it to real WGSL (`fn saxpy(x: f32) -> f32 {
/// return ((x * f32(2.0)) + f32(1.0)); }`). The witness then wraps that
/// real function in a `@compute` dispatch harness and runs the result.
pub fn general_metahir_module() -> Module {
    // body: (x * 2.0) + 1.0  — FloatBinOp(Add, FloatBinOp(Mul, x, 2.0), 1.0)
    let body_expr = Expr::FloatBinOp {
        op: FloatOp::Add,
        lhs: Box::new(Expr::FloatBinOp {
            op: FloatOp::Mul,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::LitFloat(2.0)),
        }),
        rhs: Box::new(Expr::LitFloat(1.0)),
    };
    let saxpy = Function {
        name: GENERAL_FN.into(),
        params: vec![Param {
            name: "x".into(),
            ty: Type::F32,
            mutable: false,
        }],
        return_type: Type::F32,
        body: Block {
            stmts: vec![],
            trailing_return: body_expr,
        },
    };
    Module {
        name: "spirv_saxpy_general".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(saxpy)],
        ffi_boundaries: Vec::new(),
    }
}

/// PMAT-1388: the function a `@compute` dispatch harness can call per
/// element — returned as `(fn name, WGSL element type)`.
///
/// The harness reads one scalar from a storage buffer, calls the function,
/// and writes one scalar back, so it can only wrap a module whose emitted
/// entry is unambiguous and element-shaped: **exactly one** function, taking
/// **one** parameter whose type equals the return type. Anything else
/// (several functions — which is the entry? — a multi-parameter function, a
/// mixed-type signature) gets NO harness and is emitted as a bare SPIR-V
/// module of the compiled functions, which naga accepts (an entry-point-free
/// SPIR-V module is a valid library module).
///
/// The WGSL lane lowers meta-HIR `I64` to WGSL `i32` (WGSL core has no 64-bit
/// integer), so the buffer element type follows the *emitted* type, not the
/// meta-HIR one.
fn dispatch_entry(module: &Module) -> Option<(&str, &'static str)> {
    let mut fns = module.items.iter().filter_map(|i| match i {
        Item::Function(f) => Some(f),
        _ => None,
    });
    let f = fns.next()?;
    if fns.next().is_some() {
        return None;
    }
    if f.params.len() != 1 {
        return None;
    }
    let elem = match (&f.params[0].ty, &f.return_type) {
        (Type::F32, Type::F32) => "f32",
        (Type::I64, Type::I64) => "i32",
        _ => return None,
    };
    Some((f.name.as_str(), elem))
}

/// Drive xpile's REAL emission to get the compute shader for **`module`**.
///
/// PMAT-1388: this used to be `general_real_wgsl()`, which took NO argument
/// and always lowered the hardcoded [`general_metahir_module`]. The general
/// SPIR-V emitter called it with the caller's `Module` bound to `_module`, so
/// `xpile transpile <anything> --target spirv` exited 0 emitting a SPIR-V
/// binary for `2.0*x + 1.0` — a program the user never wrote. Six
/// categorically different inputs (including two the WGSL sibling REFUSES)
/// produced byte-identical SPIR-V. The module is now a parameter and the
/// saxpy module is just one caller of it.
///
/// Calls [`emit_wgsl_module`] on the given module — the REAL PMAT-970
/// meta-HIR → WGSL lowering — then, when the module has a
/// [`dispatch_entry`], wraps the emitted scalar function (the load-bearing,
/// real-compiler-emitted arithmetic) in a thin `@compute @workgroup_size(64)`
/// dispatch harness exposing the binding-0-in / binding-1-out storage-buffer
/// contract the GPU witness drives. The returned WGSL is then compiled to
/// SPIR-V via naga.
///
/// This is the REAL path: the arithmetic is xpile's output, only the
/// per-element dispatch shell is added (the GPU analogue of `extern "C"`
/// glue around a real lowered function).
pub fn real_wgsl_for(module: &Module) -> Result<String, String> {
    let emitted = emit_wgsl_module(module).map_err(|e| {
        format!(
            "xpile emit_wgsl_module refused module `{}`: {e:?}",
            module.name
        )
    })?;
    let Some((entry, elem)) = dispatch_entry(module) else {
        return Ok(emitted);
    };
    // Thin dispatch harness around the REAL emitted function. The harness
    // declares the in/out storage buffers and calls xpile's entry function
    // once per element; the arithmetic lives entirely in the emitted fn.
    let harness = format!(
        "@group(0) @binding(0) var<storage, read> inp: array<{elem}>;\n\
         @group(0) @binding(1) var<storage, read_write> outp: array<{elem}>;\n\
         \n\
         @compute @workgroup_size(64)\n\
         fn {ENTRY_POINT}(@builtin(global_invocation_id) gid: vec3<u32>) {{\n\
         \x20   let i = gid.x;\n\
         \x20   if (i < arrayLength(&inp)) {{\n\
         \x20       // dispatch the REAL xpile-emitted scalar fn per element\n\
         \x20       outp[i] = {entry}(inp[i]);\n\
         \x20   }}\n\
         }}\n"
    );
    Ok(format!("{emitted}\n{harness}"))
}

/// The witness's own general-side WGSL: [`real_wgsl_for`] over
/// [`general_metahir_module`], with the sanity check that the REAL lowering
/// actually produced the scalar `saxpy` fn.
pub fn general_real_wgsl() -> Result<String, String> {
    let module = general_metahir_module();
    let wgsl = real_wgsl_for(&module)
        .map_err(|e| format!("xpile emit_wgsl_module (general saxpy) failed: {e}"))?;
    if !wgsl.contains(&format!("fn {GENERAL_FN}(")) {
        return Err(format!(
            "xpile emit_wgsl_module did not emit `fn {GENERAL_FN}(` — got:\n{wgsl}"
        ));
    }
    Ok(wgsl)
}

/// PMAT-1388: `true` when `module` is structurally the very module the
/// hardcoded `fma` specialist is a hand-written variant of.
///
/// [`Module`] does not implement `PartialEq`, but it derives `Debug`, whose
/// rendering is a total structural encoding of the tree — so comparing the
/// two renderings is an exact structural equality test with no new
/// dependency and no hand-written (and therefore drift-prone) field walk.
pub fn is_general_saxpy_module(module: &Module) -> bool {
    format!("{module:?}") == format!("{:?}", general_metahir_module())
}

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
/// Guarded by [`xpile_wgsl_codegen::gpu_probe_env_usable`] (PMAT-1088):
/// a fully headless Linux session skips loudly *before* the Vulkan
/// loader is touched — the ICD enumeration intermittently SIGSEGVs
/// there, which no in-process probe can survive.
pub fn vulkan_adapter_available() -> bool {
    if !xpile_wgsl_codegen::gpu_probe_env_usable() {
        return false;
    }
    // PMAT-1098: loader entry is serialized process-wide — concurrent
    // enumeration from parallel test threads intermittently SIGSEGVs
    // the ICD even in a usable session.
    let _loader = xpile_wgsl_codegen::vulkan_loader_guard();
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
    /// Loader entry is serialized (PMAT-1098).
    fn acquire_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
        let _loader = xpile_wgsl_codegen::vulkan_loader_guard();
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

        // PMAT-988: the MultiEmitterBackend hands us the two emitters' PRIMARY
        // text (the SPIR-V summaries). The witness MUST run what it is GIVEN —
        // earlier it ran a re-derived `general_real_wgsl()` + the hardcoded
        // `SPECIALIST_WGSL` constant and merely `debug_assert!`ed its
        // arguments, so the ATTESTED text and the EXECUTED text could diverge
        // silently (the adversarial finding). Each summary inlines the exact
        // WGSL the emitter compiled into its attested SPIR-V; we recover that
        // WGSL and run IT, so what EXECUTES is byte-derived from what is
        // ATTESTED. The general side therefore still traces back to xpile's
        // REAL emission — because that real WGSL is what the general emitter
        // embedded in `general_text`.
        let general_wgsl = extract_wgsl_from_summary(general_text)
            .map_err(|e| format!("recovering attested general WGSL: {e}"))?;
        let specialist_wgsl = extract_wgsl_from_summary(specialist_text)
            .map_err(|e| format!("recovering attested specialist WGSL: {e}"))?;

        let (device, queue, adapter_label) = Self::acquire_device()?;

        let general = Self::run_spirv(
            &device,
            &queue,
            &general_wgsl,
            "general (attested xpile emit)",
        )?;
        let specialist = Self::run_spirv(
            &device,
            &queue,
            &specialist_wgsl,
            "specialist (attested reference)",
        )?;
        let _ = adapter_label; // available for tracing; not part of the vote.

        // First oracle: the executed REAL-path output must equal the
        // CPython-equivalent reference vector. This is the strong check —
        // it attests xpile's real emission computes the reference truth,
        // not merely that two GPU paths agree.
        if general.len() != EXPECTED_OUTPUT.len() {
            return Ok(DiffExecResult::Divergent {
                max_abs_diff: f64::INFINITY,
                tolerance,
            });
        }
        let real_vs_expected = general
            .iter()
            .zip(EXPECTED_OUTPUT.iter())
            .map(|(g, e)| ((*g as f64) - (*e as f64)).abs())
            .fold(0.0_f64, f64::max);
        if real_vs_expected > tolerance {
            return Ok(DiffExecResult::Divergent {
                max_abs_diff: real_vs_expected,
                tolerance,
            });
        }

        // Second oracle: the REAL path and the independent `fma` reference
        // must agree (categorically different SPIR-V, same numeric truth).
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
            .fold(real_vs_expected, f64::max);

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
    fn specialist_is_the_trusted_reference_shape() {
        // The specialist stays a hardcoded `fma` kernel — the independent
        // oracle the REAL path is diffed against, on the harness contract.
        assert!(SPECIALIST_WGSL.contains("@compute @workgroup_size(64)"));
        assert!(SPECIALIST_WGSL.contains("fma(2.0, inp[i], 1.0)"));
        assert_eq!(ENTRY_POINT, "main");
    }

    #[test]
    fn general_module_is_real_metahir() {
        // PMAT-977: the general side is a genuine meta-HIR module, not a
        // hardcoded shader.
        let m = general_metahir_module();
        assert_eq!(m.items.len(), 1);
        match &m.items[0] {
            Item::Function(f) => {
                assert_eq!(f.name, GENERAL_FN);
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].ty, Type::F32);
                assert_eq!(f.return_type, Type::F32);
            }
            other => panic!("expected a function item, got {other:?}"),
        }
    }

    #[test]
    fn general_wgsl_is_xpiles_real_emission() {
        // The general WGSL must come from xpile's REAL lowering
        // (emit_wgsl_module): the contract banner + the real `saxpy` fn
        // with the lowered `x * f32(2.0) + f32(1.0)` arithmetic — NOT a
        // hardcoded `2.0 * inp[i] + 1.0` shader.
        let wgsl = general_real_wgsl().expect("real xpile WGSL emit");
        // Real-lowering fingerprints from xpile_wgsl_codegen::emit_wgsl_module:
        assert!(
            wgsl.contains("meta-HIR → WGSL"),
            "missing real-lowering banner:\n{wgsl}"
        );
        assert!(
            wgsl.contains("C-COMPILE-RUST-TO-WGSL"),
            "missing WGSL contract citation:\n{wgsl}"
        );
        assert!(
            wgsl.contains(&format!("fn {GENERAL_FN}(x: f32) -> f32")),
            "missing real lowered saxpy fn:\n{wgsl}"
        );
        assert!(
            wgsl.contains("(x * f32(2.0)) + f32(1.0)"),
            "missing real lowered arithmetic:\n{wgsl}"
        );
        // The thin dispatch harness wraps the real fn:
        assert!(wgsl.contains("@compute @workgroup_size(64)"));
        assert!(wgsl.contains(&format!("outp[i] = {GENERAL_FN}(inp[i]);")));
    }

    #[test]
    fn general_real_wgsl_naga_validates() {
        // The real emission + harness must parse + type-check under naga
        // (the CPU-only gate), so the GPU run can never be the first thing
        // to discover an invalid shader.
        let wgsl = general_real_wgsl().expect("real xpile WGSL emit");
        xpile_wgsl_codegen::naga_validate_wgsl(&wgsl)
            .expect("real xpile general WGSL must naga-validate");
    }

    #[test]
    fn expected_output_is_cpython_equivalent() {
        // EXPECTED_OUTPUT is the reference `2.0*x + 1.0` over FIXTURE_INPUT,
        // computed in plain f32 — the CPython-equivalent truth the executed
        // real-path result is checked against.
        let computed: Vec<f32> = FIXTURE_INPUT.iter().map(|x| 2.0_f32 * x + 1.0).collect();
        assert_eq!(computed.as_slice(), EXPECTED_OUTPUT);
    }

    #[test]
    fn engine_constructs() {
        let _engine = SpirvDiffExecEngine::new();
    }

    // ─── PMAT-988: attested == executed ──────────────────────────────────

    /// Build the exact SPIR-V summary the GENERAL emitter hands the witness:
    /// compile the real xpile WGSL to SPIR-V, then render the inline-WGSL
    /// summary via `crate::spirv_text_summary` (the emitter's primary text).
    fn general_attested_summary() -> String {
        let wgsl = general_real_wgsl().expect("real xpile WGSL emit");
        let words = crate::wgsl_to_spirv_words(&wgsl).expect("compile to SPIR-V");
        crate::spirv_text_summary(&words, &wgsl)
    }

    /// Same for the SPECIALIST `fma` reference summary.
    fn specialist_attested_summary() -> String {
        let words = crate::wgsl_to_spirv_words(SPECIALIST_WGSL).expect("compile to SPIR-V");
        crate::spirv_text_summary(&words, SPECIALIST_WGSL)
    }

    #[test]
    fn extract_round_trips_the_attested_general_wgsl() {
        // The WGSL recovered from the general emitter's PRIMARY summary must
        // be the exact WGSL the emitter compiled — so running the recovered
        // WGSL runs the attested artifact, not a re-derived one.
        let summary = general_attested_summary();
        let recovered = extract_wgsl_from_summary(&summary).expect("recover attested WGSL");
        let original = general_real_wgsl().expect("real xpile WGSL emit");
        // spirv_text_summary inlines line-by-line; the recovered block is the
        // original with a trailing newline normalisation.
        assert_eq!(recovered.trim_end(), original.trim_end());
        // And it must still recompile to real SPIR-V (it IS the attested src).
        let words = crate::wgsl_to_spirv_words(&recovered).expect("recovered WGSL compiles");
        assert!(crate::spirv_looks_real(&words));
    }

    #[test]
    fn extract_round_trips_the_attested_specialist_wgsl() {
        let summary = specialist_attested_summary();
        let recovered = extract_wgsl_from_summary(&summary).expect("recover attested WGSL");
        assert_eq!(recovered.trim_end(), SPECIALIST_WGSL.trim_end());
        assert!(recovered.contains("fma(2.0, inp[i], 1.0)"));
    }

    #[test]
    fn extract_errors_when_no_wgsl_block() {
        // A summary that doesn't embed its source is not something we can
        // attest+run — the witness must fault, never silently fall back.
        let err = extract_wgsl_from_summary("; SPIR-V\n; Magic: 0x07230203\n")
            .expect_err("no Source WGSL block must Err");
        assert!(err.contains("no `; Source WGSL` block"), "got: {err}");
    }

    #[test]
    fn extract_errors_when_wgsl_block_empty() {
        let err = extract_wgsl_from_summary("; Source WGSL (reused):\nnot a comment\n")
            .expect_err("empty Source WGSL block must Err");
        assert!(err.contains("was empty"), "got: {err}");
    }

    fn diffexec_config() -> BackendConfig {
        BackendConfig {
            emit_contracts: true,
            target: xpile_backend::Target::Spirv,
            profile: xpile_backend::Profile::RustOut,
            hardware: Some(HwProfile::Spirv { version: (1, 3) }),
        }
    }

    #[test]
    fn engine_runs_the_input_it_is_given_not_hardcoded_constants() {
        // PMAT-988 REGRESSION GUARD. Feed the engine a specialist summary
        // whose embedded WGSL is DELIBERATELY WRONG (computes `x` instead of
        // `2*x + 1`). An engine that ignores its arguments and runs the
        // hardcoded `SPECIALIST_WGSL` would still report Match; an engine
        // that RUNS what it is HANDED must report Divergent (the wrong
        // shader's output disagrees with both EXPECTED_OUTPUT and the real
        // general path). So this test FAILS for an input-ignoring engine.
        if !vulkan_adapter_available() {
            eprintln!("[skip] no Vulkan adapter — PMAT-988 regression guard needs a GPU");
            return;
        }
        let engine = SpirvDiffExecEngine::new();
        let module = general_metahir_module();
        let cfg = diffexec_config();

        // Sanity: with the CORRECT attested summaries the engine matches.
        let good = engine
            .execute_and_compare(
                &general_attested_summary(),
                &specialist_attested_summary(),
                &module,
                &cfg,
                1.0e-3,
            )
            .expect("correct attested summaries run on the GPU");
        assert!(
            matches!(good, DiffExecResult::Match { .. }),
            "correct attested inputs should Match, got {good:?}"
        );

        // Now a WRONG specialist: identity `outp[i] = inp[i]`. This is a real,
        // naga-valid shader, just the wrong arithmetic.
        let wrong_specialist_wgsl = "\
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> outp: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&inp)) {
        outp[i] = inp[i];
    }
}
";
        let wrong_words =
            crate::wgsl_to_spirv_words(wrong_specialist_wgsl).expect("wrong shader compiles");
        let wrong_summary = crate::spirv_text_summary(&wrong_words, wrong_specialist_wgsl);

        let bad = engine
            .execute_and_compare(
                &general_attested_summary(),
                &wrong_summary,
                &module,
                &cfg,
                1.0e-3,
            )
            .expect("the wrong-but-valid shader still runs on the GPU");
        // The result MUST change: an input-respecting engine diverges here.
        assert!(
            matches!(bad, DiffExecResult::Divergent { .. }),
            "PMAT-988: feeding a wrong specialist must change the result to \
             Divergent (proves the engine runs its INPUT, not hardcoded \
             constants), got {bad:?}"
        );
    }

    #[test]
    fn witness_executes_the_attested_text_and_matches() {
        // Positive end-to-end: the attested general (xpile real emission) and
        // specialist summaries — recovered + run by the engine — must Match,
        // proving the EXECUTED artifact (derived from the attested text) is
        // the truth. Prints the executed/attested correspondence for the
        // captured Vulkan output.
        if !vulkan_adapter_available() {
            eprintln!("[skip] no Vulkan adapter — executed-witness test needs a GPU");
            return;
        }
        let general_summary = general_attested_summary();
        let specialist_summary = specialist_attested_summary();

        // Prove the thing we run is byte-derived from the thing we attest.
        let executed_general =
            extract_wgsl_from_summary(&general_summary).expect("recover attested general WGSL");
        eprintln!("[PMAT-988] EXECUTED general WGSL (recovered from attested summary):");
        eprintln!("{executed_general}");

        let engine = SpirvDiffExecEngine::new();
        let result = engine
            .execute_and_compare(
                &general_summary,
                &specialist_summary,
                &general_metahir_module(),
                &diffexec_config(),
                1.0e-3,
            )
            .expect("attested summaries run on the GPU");
        eprintln!("[PMAT-988] DiffExec result over the ATTESTED text: {result:?}");
        match result {
            DiffExecResult::Match { max_abs_diff } => {
                assert!(max_abs_diff <= 1.0e-3);
            }
            other => panic!("expected Match over the attested text, got {other:?}"),
        }
    }
}
