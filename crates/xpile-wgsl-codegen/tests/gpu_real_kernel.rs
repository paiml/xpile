//! PMAT-983 — GPU END-TO-END real-program proof for the WGSL lane.
//!
//! The WGSL analog of PMAT-981's WASM end-to-end proof. The existing
//! `gpu_witness.rs` (PMAT-950/975) executes the SAXPY kernel — pure
//! per-element float arithmetic (`out[i] = 2*in[i] + 1`) with no control
//! flow. That proves arithmetic + buffer read+write COMPOSE, but it does
//! NOT exercise the comparison/`if` control flow that PMAT-970 also added.
//!
//! This test closes that gap: it builds the meta-HIR for a GENUINE
//! per-element kernel — a **clamp+scale**:
//!
//! ```python
//! def clamp_scale(inp: list[float], outp: list[float], i: int) -> None:
//!     v = inp[i] * 3.0          # arithmetic over a buffer READ
//!     if v > 10.0:              # comparison + statement-if
//!         outp[i] = 10.0        # buffer WRITE — clamp high
//!     else:
//!         if v < 0.0:           # nested comparison
//!             outp[i] = 0.0     # buffer WRITE — clamp low
//!         else:
//!             outp[i] = v       # buffer WRITE — pass through
//! ```
//!
//! It composes EVERY construct the PMAT-970/979 lowering added on a single
//! kernel: f32 arithmetic, a `let` binding, two `list[f32]` storage buffers
//! (one `read` input, one `read_write` output), a comparison, a nested
//! `if`/`else`, and a per-element buffer store. It then:
//!
//!   1. lowers the kernel through the REAL [`emit_wgsl_module`] (no
//!      hardcoded shader),
//!   2. wraps the emitted `fn` in a `@compute` entry point that calls it
//!      once per element,
//!   3. compiles + validates via naga,
//!   4. runs it on the real Vulkan adapter via wgpu, and
//!   5. asserts the executed per-element output VALUE-MATCHES the CPython
//!      result of the same kernel over a deterministic fixture.
//!
//! Gated on [`vulkan_adapter_available`]: a GPU box runs the real Vulkan
//! execution; free CI records the skip and the CPU-only naga validation
//! still runs (the emit path is always under test).

use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Stmt, Type,
};
use xpile_wgsl_codegen::{emit_wgsl_module, naga_validate_wgsl, vulkan_adapter_available};

/// Deterministic fixture covering: a value that scales ABOVE the high
/// clamp (8.0*3=24 → 10), a value in-range (2.5*3=7.5 → 7.5), a value at
/// the low clamp via a negative (−1.0*3=−3 → 0), zero (→0), a small
/// positive in-range (0.5*3=1.5), and a value landing exactly on the high
/// clamp boundary (10.0/3 ≈ 3.333…*3 = 10.0 → 10.0). Mixed signs and
/// boundary cases so a wrong comparison or a dropped branch is caught.
const FIXTURE: &[f32] = &[8.0, 2.5, -1.0, 0.0, 0.5, 100.0, -7.0, 3.3333333];

const SCALE: f32 = 3.0;
const CLAMP_HI: f32 = 10.0;
const CLAMP_LO: f32 = 0.0;

/// The CPython-equivalent reference: `out[i] = clamp(in[i]*3, 0, 10)`,
/// expressed with the SAME explicit branch structure the kernel uses so
/// the reference is an INDEPENDENT ground truth, not a re-derivation.
/// (`#[allow(clippy::manual_clamp)]`: delegating to `f32::clamp` would
/// collapse the very branch order the GPU kernel is being checked against —
/// the explicit `if/else if/else` IS the point.)
#[allow(clippy::manual_clamp)]
fn cpython_clamp_scale(x: f32) -> f32 {
    let v = x * SCALE;
    if v > CLAMP_HI {
        CLAMP_HI
    } else if v < CLAMP_LO {
        CLAMP_LO
    } else {
        v
    }
}

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

fn lit_f(v: f64) -> Expr {
    Expr::LitFloat(v)
}

fn index(buf: &str, idx: &str) -> Expr {
    Expr::Index {
        collection: Box::new(ident(buf)),
        index: Box::new(ident(idx)),
    }
}

/// Build the meta-HIR module for the per-element clamp+scale kernel.
///
/// `fn clamp_scale(inp: list[f32], outp: list[f32], i: i32) -> ()`:
///   - `inp` lowers to a `var<storage, read>` buffer (read only),
///   - `outp` lowers to a `var<storage, read_write>` buffer (written),
///   - `i` stays a scalar `i32` fn param (the per-element index),
///   - the body composes arithmetic + comparison + nested if/else + store.
fn clamp_scale_module() -> Module {
    // v = inp[i] * 3.0
    let let_v = Stmt::Let {
        name: "v".into(),
        ty: Type::F32,
        value: Expr::FloatBinOp {
            op: FloatOp::Mul,
            lhs: Box::new(index("inp", "i")),
            rhs: Box::new(lit_f(SCALE as f64)),
        },
        mutable: false,
    };

    let store = |val: Expr| Stmt::IndexAssign {
        list_name: "outp".into(),
        indices: vec![ident("i")],
        value: val,
    };

    // else branch: if v < 0.0 { outp[i] = 0.0 } else { outp[i] = v }
    let inner_if = Stmt::If {
        cond: Expr::BinOp {
            op: BinOp::Lt,
            lhs: Box::new(ident("v")),
            rhs: Box::new(lit_f(CLAMP_LO as f64)),
        },
        then_body: vec![store(lit_f(CLAMP_LO as f64))],
        else_body: vec![store(ident("v"))],
    };

    // if v > 10.0 { outp[i] = 10.0 } else { <inner_if> }
    let outer_if = Stmt::If {
        cond: Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(ident("v")),
            rhs: Box::new(lit_f(CLAMP_HI as f64)),
        },
        then_body: vec![store(lit_f(CLAMP_HI as f64))],
        else_body: vec![inner_if],
    };

    let f = Function {
        name: "clamp_scale".into(),
        params: vec![
            Param {
                name: "inp".into(),
                ty: Type::List(Box::new(Type::F32)),
                mutable: false,
            },
            Param {
                name: "outp".into(),
                ty: Type::List(Box::new(Type::F32)),
                mutable: true,
            },
            Param {
                name: "i".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![let_v, outer_if],
            trailing_return: Expr::Unit,
        },
    };

    Module {
        name: "clamp_scale_kernel".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

/// Wrap the REAL emitted `fn clamp_scale(i: i32)` (the two list params
/// became module-scope buffers) in a `@compute` entry that invokes it once
/// per element. The emitted fn body is the load-bearing computation; the
/// entry point only computes the index and dispatches.
fn wrap_as_compute(emitted_module: &str) -> String {
    format!(
        "{emitted_module}\n\
         @compute @workgroup_size(64)\n\
         fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{\n\
         \x20   let i = i32(gid.x);\n\
         \x20   if (u32(i) < arrayLength(&clamp_scale_inp)) {{\n\
         \x20       clamp_scale(i);\n\
         \x20   }}\n\
         }}\n"
    )
}

/// The real emitted WGSL compute shader for the clamp+scale kernel.
fn real_emitted_clamp_scale_wgsl() -> String {
    let emitted = emit_wgsl_module(&clamp_scale_module())
        .expect("clamp_scale kernel lowers through emit_wgsl_module");
    wrap_as_compute(&emitted)
}

#[test]
fn clamp_scale_kernel_emits_and_naga_validates() {
    // CPU-only half: the real emission must compose correctly and pass the
    // naga front-end (parse + type-check). This runs everywhere, GPU or not.
    let emitted = emit_wgsl_module(&clamp_scale_module()).expect("kernel lowers");

    // Two distinct buffers: the input is read-only, the output is written.
    assert!(
        emitted.contains("var<storage, read> clamp_scale_inp: array<f32>;"),
        "input buffer must bind read-only:\n{emitted}"
    );
    assert!(
        emitted.contains("var<storage, read_write> clamp_scale_outp: array<f32>;"),
        "output buffer must bind read_write (it is stored into):\n{emitted}"
    );
    // `i` stays a scalar fn param; the list params do not.
    assert!(emitted.contains("fn clamp_scale(i: i32)"), "{emitted}");
    // The composed body: arithmetic over a buffer read, a comparison, and a
    // store back into the output buffer.
    assert!(
        emitted.contains("(clamp_scale_inp[u32(i)] * f32(3.0))"),
        "scale-over-read must lower:\n{emitted}"
    );
    assert!(emitted.contains("(v > f32(10.0))"), "{emitted}");
    assert!(emitted.contains("(v < f32(0.0))"), "{emitted}");
    assert!(
        emitted.contains("clamp_scale_outp[u32(i)] = "),
        "per-element store must lower:\n{emitted}"
    );

    // The WHOLE compute shader (real fn + @compute wrapper) naga-validates.
    let full = real_emitted_clamp_scale_wgsl();
    naga_validate_wgsl(&full).unwrap_or_else(|e| {
        panic!("real clamp+scale compute shader must naga-validate: {e}\n{full}")
    });
}

#[test]
fn clamp_scale_kernel_runs_on_vulkan_and_value_matches_cpython() {
    if !vulkan_adapter_available() {
        eprintln!(
            "PMAT-983: skipping executed GPU clamp+scale proof — no Vulkan adapter present. \
             A GPU box runs the real kernel and value-matches CPython; free CI relies on the \
             CPU-only naga validation in clamp_scale_kernel_emits_and_naga_validates."
        );
        return;
    }

    eprintln!("PMAT-983: running the REAL clamp+scale per-element kernel on the Vulkan adapter");

    let shader = real_emitted_clamp_scale_wgsl();
    eprintln!(
        "PMAT-983: xpile's REAL emitted WGSL executed on the Vulkan GPU \
         (meta-HIR → emit_wgsl_module → @compute → run):\n{shader}"
    );

    let gpu = run_clamp_scale_on_gpu(&shader).expect("kernel runs on the Vulkan adapter");
    let cpython: Vec<f32> = FIXTURE.iter().map(|x| cpython_clamp_scale(*x)).collect();

    eprintln!("PMAT-983: fixture  = {FIXTURE:?}");
    eprintln!("PMAT-983: GPU out  = {gpu:?}");
    eprintln!("PMAT-983: CPython  = {cpython:?}");

    assert_eq!(gpu.len(), cpython.len(), "element count mismatch");
    let max_abs_diff = gpu
        .iter()
        .zip(cpython.iter())
        .map(|(g, c)| ((*g as f64) - (*c as f64)).abs())
        .fold(0.0_f64, f64::max);
    assert!(
        max_abs_diff <= 1.0e-5,
        "executed GPU clamp+scale diverged from CPython: max_abs_diff={max_abs_diff}\n\
         GPU={gpu:?}\nCPython={cpython:?}"
    );

    eprintln!(
        "PMAT-983: EXECUTED Vulkan clamp+scale kernel PASSED — \
         per-element GPU output value-matches CPython (max_abs_diff={max_abs_diff}). \
         arithmetic + comparison + nested if/else + buffer read+write COMPOSE on real hardware."
    );
}

/// Run the clamp+scale compute shader over [`FIXTURE`] on a Vulkan adapter,
/// returning the read-back output buffer. Self-contained wgpu marshalling
/// (mirrors the witness engine in `wgpu_diffexec.rs`); two storage buffers:
/// binding 0 = input (read), binding 1 = output (write).
fn run_clamp_scale_on_gpu(shader_src: &str) -> Result<Vec<f32>, String> {
    use wgpu::util::DeviceExt;

    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::VULKAN | wgpu::Backends::METAL | wgpu::Backends::DX12;
    let instance = wgpu::Instance::new(desc);

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .map_err(|e| format!("request_adapter failed: {e}"))?;
    let info = adapter.get_info();
    eprintln!(
        "PMAT-983: adapter = {} ({:?}, {:?})",
        info.name, info.device_type, info.backend
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("xpile-wgsl-clamp-scale"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| format!("request_device failed: {e}"))?;

    let n = FIXTURE.len();
    let bytes = std::mem::size_of_val(FIXTURE) as u64;

    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("clamp-scale-in"),
        contents: bytemuck::cast_slice(FIXTURE),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("clamp-scale-out"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("clamp-scale-readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("clamp-scale-module"),
        source: wgpu::ShaderSource::Wgsl(shader_src.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("clamp-scale-pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    if let Some(err) = pollster::block_on(error_scope.pop()) {
        return Err(format!("WGSL validation error: {err}"));
    }

    let bind_group_layout = pipeline.get_bind_group_layout(0);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("clamp-scale-bg"),
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
            label: Some("clamp-scale-pass"),
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
        .map_err(|e| format!("poll failed: {e}"))?;
    rx.recv()
        .map_err(|e| format!("map channel closed: {e}"))?
        .map_err(|e| format!("map_async failed: {e}"))?;
    let data = slice.get_mapped_range();
    let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    readback.unmap();
    Ok(result)
}
