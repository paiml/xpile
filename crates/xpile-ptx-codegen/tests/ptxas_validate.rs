//! PMAT-961 — OFFLINE PTX validation: the real `ptxas` assembles xpile's
//! hand-emitted PTX.
//!
//! The PTX analog of `wat2wasm`-assembles-WAT (xpile-wasm-codegen) and
//! naga-validates-WGSL (xpile-wgsl/spirv). It does NOT run on the GPU — `ptxas`
//! is the offline NVIDIA assembler, available in any CUDA toolkit install. The
//! hand-emitted PTX from [`xpile_ptx_codegen::emit_kernel`] must assemble clean
//! for the requested compute capability.
//!
//! Graceful-skip posture (mirrors cc/python3/nvcc/WABT): when `ptxas` is absent
//! (free CI runners have no CUDA toolkit), the test records a skip notice and
//! exits OK; the emitted PTX still passes the pure-text structural
//! `validate_ptx` gate so the path stays under test. On a box with `ptxas`
//! (RTX 4090 / sm_89) the emitted PTX is assembled for real.

use xpile_backend::{Backend, BackendConfig, HwProfile, Profile, Target};
use xpile_meta_hir::{Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Type};
use xpile_ptx_codegen::{emit_kernel, ptxas_assemble, ptxas_available, validate_ptx, PtxBackend};

/// `def xpile_kernel(x: f64) -> f64: return (x + x) + 1.0` — the saxpy-like
/// element-wise kernel.
fn saxpy_fn() -> Function {
    let x_plus_x = Expr::FloatBinOp {
        op: FloatOp::Add,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Ident("x".into())),
    };
    Function {
        name: "xpile_kernel".into(),
        params: vec![Param {
            name: "x".into(),
            ty: Type::F64,
            mutable: false,
        }],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(x_plus_x),
                rhs: Box::new(Expr::LitFloat(1.0)),
            },
        },
    }
}

fn module_with(f: Function) -> Module {
    Module {
        name: "saxpy_kernel".into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
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
fn ptxas_assembles_hand_emitted_ptx() {
    // Use the local GPU's compute capability when `nvidia-smi` is present,
    // else the contract floor sm_80 (ptxas in any modern toolkit handles it).
    let sm = local_compute_capability();

    // The backend emits the same real PTX the standalone emitter does.
    let backend = PtxBackend::new();
    let artifact = backend
        .lower(&module_with(saxpy_fn()), &ptx_config(&sm))
        .expect("real PTX emitter lowers the saxpy kernel");
    let ptx = &artifact.primary;

    // Structural gate always runs (pure text, no toolchain).
    assert_eq!(
        validate_ptx(ptx, &sm),
        Ok(()),
        "emitted PTX must pass the offline structural well-formedness gate:\n{ptx}"
    );
    // The standalone emitter agrees byte-for-byte.
    assert_eq!(&emit_kernel(&saxpy_fn(), &sm).unwrap(), ptx);

    if !ptxas_available() {
        eprintln!(
            "PMAT-961: skipping ptxas offline assemble — `ptxas` not present. \
             A box with the CUDA toolkit assembles this PTX clean; free CI \
             records the structural-gate pass and stays green."
        );
        return;
    }

    eprintln!("PMAT-961: assembling hand-emitted PTX with ptxas for {sm}");
    match ptxas_assemble(ptx, &sm) {
        Ok(()) => eprintln!(
            "PMAT-961: ptxas ASSEMBLED the hand-emitted PTX clean for {sm} — \
             the offline-validation witness (PTX analog of wat2wasm/naga)."
        ),
        Err(stderr) => panic!(
            "ptxas REJECTED xpile's hand-emitted PTX for {sm}:\n{stderr}\n--- PTX ---\n{ptx}"
        ),
    }
}

#[test]
fn ptxas_rejects_a_corrupted_kernel() {
    // Teeth check: ptxas catches a deliberately-broken PTX (a typo'd opcode).
    // Only meaningful when ptxas is present.
    if !ptxas_available() {
        eprintln!("PMAT-961: skipping ptxas negative test — `ptxas` not present.");
        return;
    }
    let sm = local_compute_capability();
    let mut broken = emit_kernel(&saxpy_fn(), &sm).unwrap();
    // Corrupt a real instruction into a non-opcode.
    broken = broken.replace("add.rn.f64", "addd.rn.f64");
    assert!(
        ptxas_assemble(&broken, &sm).is_err(),
        "ptxas must reject a corrupted kernel (a typo'd opcode) — proves the assemble step has teeth"
    );
}

/// The local GPU's compute capability via `nvidia-smi` (`sm_<maj><min>`),
/// falling back to the contract floor `sm_80` when unavailable.
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
