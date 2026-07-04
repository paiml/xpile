//! PMAT-984 — the PTX GPU END-TO-END real-program COMPOSITION proof (the PTX
//! analog of PMAT-981's WASM real-program proof).
//!
//! The individual PTX lane capabilities each have their own witness already:
//! scalar arithmetic (PMAT-961), control flow (PMAT-962), the abs/min/max/sqrt
//! builtins (PMAT-972), and the `list[scalar]` array read `xs[i]`→`ld.global`
//! plus the array output write (PMAT-980). What was NOT yet proven is that they
//! all COMPOSE on a SINGLE real per-element compute kernel — the existing
//! array witness only drives `out[i] = xs[i] + 1.0`, an array read fused with a
//! lone add.
//!
//! This witness drives a GENUINE per-element clamp,
//!
//!     def xpile_kernel(xs: list[f64]) -> f64:
//!         return min(max(xs[i], 0.0), 5.0)        # clamp to [0.0, 5.0]
//!
//! which composes, in ONE kernel, FOUR independent lowerings that until now
//! were only ever exercised separately:
//!
//!   1. the array element READ  `xs[i]`     → indexed `ld.global.f64` (PMAT-980),
//!   2. the `max(_, 0.0)` builtin            → `max.f64` (PMAT-972),
//!   3. the `min(_, 5.0)` builtin nesting    → `min.f64` over the max's result,
//!   4. the array element WRITE `out[i] = …` → indexed `st.global.f64` (PMAT-980).
//!
//! It then assembles that PTX with `ptxas` (the well-formedness oracle) and —
//! when a CUDA box is present — runs it on the GPU through the SAME
//! categorically-independent anti-correlation harness PMAT-980 uses (xpile's
//! hand-emitted PTX via the CUDA Driver API vs an nvcc-compiled CUDA-C
//! `fminf(fmaxf(in[i], 0.0f), 5.0f)`), asserting the executed outputs
//! VALUE-MATCH. The two halves share NO codegen frontend, so they agree on the
//! GPU only if the WHOLE composition — array read + both builtins + array
//! write — lowers correctly, not just each piece in isolation.
//!
//! Graceful-skip (mirrors the PMAT-980 array witness): no nvcc/nvidia-smi → the
//! engine reports the benign `NotRun` fallback and the test records the
//! `ptxas`-assembled emit-shape assertions only (free CI stays green). On the
//! RTX 4090 / sm_89 it runs BOTH toolchains on the GPU and produces a real
//! `DiffExecResult::Match`.

use xpile_backend::{BackendConfig, DiffExecEngine, DiffExecResult, HwProfile, Profile, Target};
use xpile_meta_hir::{Block, Expr, Function, Module, NumBuiltinOp, Param, SourceLang, Type};
use xpile_ptx_codegen::{
    cuda_toolchain_available, emit_kernel, ptxas_assemble, ptxas_available, PtxDiffExecEngine,
};

/// Clamp bounds — chosen so the fixture `[0, 1, 2, -3, 4.5, 10, -0.5, 100]`
/// exercises BOTH clamp arms: -3, -0.5 hit the low bound; 10, 100 hit the high
/// bound; the rest pass through. A pure `xs[i]+1` could never distinguish a
/// broken `min`/`max` from a correct one — this one can.
const CLAMP_LO: f64 = 0.0;
const CLAMP_HI: f64 = 5.0;

/// The nvcc-compilable CUDA-C `xpile_kernel` for `out[i] = clamp(in[i], 0, 5)`
/// — `fminf(fmaxf(in[i], 0.0f), 5.0f)`, the specialist (anti-correlation) half.
/// Matches the `(const float* in, float* out, int n)` harness signature.
const CLAMP_CUDA_C: &str = "\
__global__ void xpile_kernel(const float* in, float* out, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        out[i] = fminf(fmaxf(in[i], 0.0f), 5.0f);
    }
}
";

/// `def xpile_kernel(xs: list[f64]) -> f64: return min(max(xs[i], 0.0), 5.0)`
/// — the REAL per-element clamp kernel composing array read + max + min + the
/// array write the emitter appends.
fn clamp_kernel_fn() -> Function {
    // xs[i]
    let xs_i = Expr::Index {
        collection: Box::new(Expr::Ident("xs".into())),
        index: Box::new(Expr::Ident("i".into())),
    };
    // max(xs[i], 0.0)
    let lower_clamped = Expr::NumBuiltin {
        op: NumBuiltinOp::Max,
        args: vec![xs_i, Expr::LitFloat(CLAMP_LO)],
        of_float: true,
    };
    // min(max(xs[i], 0.0), 5.0)
    let clamped = Expr::NumBuiltin {
        op: NumBuiltinOp::Min,
        args: vec![lower_clamped, Expr::LitFloat(CLAMP_HI)],
        of_float: true,
    };
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
            trailing_return: clamped,
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
        emit_contracts: true,
        target: Target::Ptx,
        profile: Profile::RustOut,
        hardware: Some(HwProfile::Ptx {
            compute_capability: sm.to_string(),
        }),
    }
}

fn kernel_module() -> Module {
    Module {
        name: "array_clamp".into(),
        source_lang: SourceLang::Rust,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

#[test]
fn ptx_array_composition_clamp_assembles_and_matches_on_gpu() {
    let sm = local_compute_capability();

    // ── emit: the composed clamp PTX (always, regardless of GPU presence). ──
    let clamp_ptx = emit_kernel(&clamp_kernel_fn(), &sm).expect("composed clamp kernel emits");

    // The composition must drive the array path AND fuse all four lowerings.
    assert!(
        clamp_ptx.contains("ARRAY element-wise subset"),
        "the composition witness must drive the array path, got:\n{clamp_ptx}"
    );
    // array READ (xs[i]) and array WRITE (out[i]) — both indexed global ld/st.
    assert!(
        clamp_ptx.contains("ld.global.f64") && clamp_ptx.contains("st.global.f64"),
        "composed clamp PTX must do an indexed global load + store, got:\n{clamp_ptx}"
    );
    // the two builtins, fused into the SAME kernel as the array read/write.
    assert!(
        clamp_ptx.contains("max.f64"),
        "composed clamp PTX must lower max() to max.f64, got:\n{clamp_ptx}"
    );
    assert!(
        clamp_ptx.contains("min.f64"),
        "composed clamp PTX must lower min() to min.f64, got:\n{clamp_ptx}"
    );

    // ── ptxas: the well-formedness oracle (runs anywhere ptxas is installed,
    // even without a GPU — e.g. an offline CUDA-toolkit CI image). ──
    if ptxas_available() {
        ptxas_assemble(&clamp_ptx, &sm).unwrap_or_else(|e| {
            panic!("composed clamp PTX failed ptxas-assembly on {sm}: {e}\n{clamp_ptx}")
        });
        eprintln!("PMAT-984: composed clamp PTX ptxas-assembled clean on {sm}.");
    } else {
        eprintln!(
            "PMAT-984: ptxas not present — recording the composed-emit-shape assertions \
             only (a CUDA-toolkit box assembles + runs the kernel)."
        );
    }

    // ── GPU: the executed anti-correlation value-match (only on a CUDA box). ──
    if !cuda_toolchain_available() {
        eprintln!(
            "PMAT-984: skipping GPU composition witness — nvcc/nvidia-smi not present. A \
             CUDA box runs xpile's hand-emitted composed clamp PTX (Driver API: indexed \
             ld.global → max.f64 → min.f64 → indexed st.global) vs nvcc-compiled CUDA-C \
             `fminf(fmaxf(in[i],0),5)` and produces a real DiffExecResult::Match; free CI \
             records the ptxas-assembled emit-shape assertions and stays green."
        );
        return;
    }

    eprintln!("PMAT-984: running composed clamp GPU anti-correlation witness on {sm}");
    let engine = PtxDiffExecEngine::new();
    let result = engine
        .execute_and_compare(
            &clamp_ptx,
            CLAMP_CUDA_C,
            &kernel_module(),
            &ptx_config(&sm),
            1.0e-3,
        )
        .expect("composition witness runs both toolchains on the GPU");

    match result {
        DiffExecResult::Match { max_abs_diff } => {
            assert!(
                max_abs_diff <= 1.0e-3,
                "executed composed-clamp outputs diverged across toolchains: \
                 max_abs_diff={max_abs_diff}"
            );
            eprintln!(
                "PMAT-984: ARRAY COMPOSITION PTX witness PASSED on {sm} — xpile's \
                 hand-emitted clamp PTX (`xs[i]` indexed ld.global → max.f64 → min.f64 → \
                 indexed st.global, Driver API) vs nvcc-compiled CUDA-C \
                 `fminf(fmaxf(in[i],0),5)` agree (max_abs_diff={max_abs_diff}). The array \
                 read + both builtins + the array write COMPOSE correctly on real NVIDIA \
                 silicon, not just ptxas-well-formed."
            );
        }
        DiffExecResult::Divergent { max_abs_diff, .. } => panic!(
            "composed clamp toolchains DIVERGED (a composition lowering falsified): xpile \
             clamp PTX vs nvcc CUDA-C max_abs_diff={max_abs_diff}"
        ),
        other => panic!("expected an executed Match on a GPU box, got {other:?}"),
    }
}
