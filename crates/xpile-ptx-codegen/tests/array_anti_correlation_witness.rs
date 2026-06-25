//! PMAT-980 — the anti-correlation §29 PTX witness over the NEW construct: an
//! **array element-wise kernel** (`list[scalar]` parameter read by `xs[i]`).
//!
//! Extends the PMAT-961 / PMAT-962 anti-correlation pattern to an ARRAY kernel.
//! The per-element function is `out[i] = xs[i] + 1.0` — but where the existing
//! witnesses drive a kernel whose input element is an IMPLICIT scalar param,
//! this one drives xpile's NEW `emit_array_kernel` path, where the `xs[i]` read
//! is an EXPLICIT indexed `ld.global` from a `list[scalar]` parameter
//! (PMAT-980, the PTX analog of PMAT-966's WASM array-load witness):
//!
//!   - general: xpile's OWN hand-emitted ARRAY PTX (from `emit_kernel` over a
//!     `def k(xs: list[f64]) -> f64: return xs[i] + 1.0`), loaded + JIT-assembled
//!     by the CUDA Driver API (`cuModuleLoadData`) and launched over the fixture.
//!   - specialist: an nvcc-compiled CUDA-C `out[i] = in[i] + 1.0f`.
//!
//! These two share NO codegen frontend (xpile hand-emits PTX; nvcc emits PTX
//! from C++); they agree on the GPU only if BOTH the array-indexing lowering and
//! the nvcc compile are correct — the anti-correlation property, on an array
//! kernel. The two halves use the EXISTING `PtxDiffExecEngine` harnesses (single
//! input array + output + count — exactly the array kernel's calling shape).
//!
//! Graceful-skip (mirrors cc/python3/nvcc/WABT): no nvcc + nvidia-smi → the
//! engine reports `NotRun { no-engine }` and the test asserts that benign
//! fallback (free CI stays green). On a CUDA box (RTX 4090 / sm_89) it runs BOTH
//! toolchains on the GPU and asserts the executed outputs agree → a real
//! `DiffExecResult::Match`.

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile, Profile, Target};
use xpile_meta_hir::{Block, Expr, FloatOp, Function, Module, Param, SourceLang, Type};
use xpile_ptx_codegen::{cuda_toolchain_available, emit_kernel, PtxDiffExecEngine};

/// The nvcc-compilable CUDA-C `xpile_kernel` for `out[i] = in[i] + 1.0`. Matches
/// the `(const float* in, float* out, int n)` signature the
/// `NvccCudaDiffExecEngine` harness expects (the specialist half).
const ADD_ONE_CUDA_C: &str = "\
__global__ void xpile_kernel(const float* in, float* out, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        out[i] = in[i] + 1.0f;
    }
}
";

/// `def xpile_kernel(xs: list[f64]) -> f64: return xs[i] + 1.0` — the array
/// element-wise kernel (the NEW PMAT-980 `list[scalar]` + `xs[i]` shape).
fn array_add_one_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![Param {
            name: "xs".into(),
            ty: Type::List(Box::new(Type::F64)),
            mutable: false,
        }],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(Expr::Index {
                    collection: Box::new(Expr::Ident("xs".into())),
                    index: Box::new(Expr::Ident("i".into())),
                }),
                rhs: Box::new(Expr::LitFloat(1.0)),
            },
        },
    }
}

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

fn kernel_module() -> Module {
    Module {
        name: "array_add_one".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

#[test]
fn ptx_array_anti_correlation_executes_on_gpu_and_matches() {
    let sm = local_compute_capability();

    // xpile's OWN hand-emitted ARRAY PTX (the general half). Emit it up front so
    // the test asserts the array-indexing shape regardless of GPU presence.
    let array_ptx = emit_kernel(&array_add_one_fn(), &sm).expect("array kernel emits");
    assert!(
        array_ptx.contains("ARRAY element-wise subset"),
        "the witness must drive the array path, got:\n{array_ptx}"
    );
    assert!(
        array_ptx.contains("ld.global.f64") && array_ptx.contains("st.global.f64"),
        "the array PTX must do an indexed global load + store, got:\n{array_ptx}"
    );

    if !cuda_toolchain_available() {
        eprintln!(
            "PMAT-980: skipping array anti-correlation PTX witness — nvcc/nvidia-smi \
             not present. A CUDA box runs xpile's hand-emitted array PTX (Driver API, \
             `xs[i]` indexed ld.global) vs nvcc-compiled CUDA-C `in[i]+1.0f` and \
             produces a real DiffExecResult::Match; free CI records the emit-shape \
             assertions and stays green."
        );
        return;
    }

    eprintln!("PMAT-980: running array anti-correlation PTX witness on {sm}");
    let engine = PtxDiffExecEngine::new();
    let result = engine
        .execute_and_compare(
            &array_ptx,
            ADD_ONE_CUDA_C,
            &kernel_module(),
            &ptx_config(&sm),
            1.0e-3,
        )
        .expect("array witness runs both toolchains on the GPU");

    match result {
        DiffExecResult::Match { max_abs_diff } => {
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed array outputs diverged across toolchains: max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-980: ARRAY ANTI-CORRELATION PTX witness PASSED on {sm} — xpile's \
                 hand-emitted array PTX (`xs[i]` indexed ld.global, Driver API) vs \
                 nvcc-compiled CUDA-C `in[i]+1.0f` agree (max_abs_diff={max_abs_diff}). \
                 The PMAT-980 `list[scalar]` + `xs[i]` array lowering is correct on real \
                 NVIDIA silicon, not just ptxas-well-formed."
            );
        }
        DiffExecResult::Divergent { max_abs_diff, .. } => panic!(
            "array toolchains DIVERGED (array-indexing lowering falsified): xpile array PTX \
             vs nvcc CUDA-C max_abs_diff={max_abs_diff}"
        ),
        other => panic!("expected an executed Match on a GPU box, got {other:?}"),
    }
}
