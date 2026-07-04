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
use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Stmt, Type,
};
use xpile_ptx_codegen::{emit_kernel, ptxas_assemble, ptxas_available, validate_ptx, PtxBackend};

fn fp(name: &str) -> Param {
    Param {
        name: name.into(),
        ty: Type::F64,
        mutable: false,
    }
}

fn ident(n: &str) -> Box<Expr> {
    Box::new(Expr::Ident(n.into()))
}

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
        emit_contracts: true,
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

// ─── PMAT-962: control-flow + multi-param kernels assemble clean ─────

/// `def xpile_kernel(a, b) -> f64: return a + b` — a multi-input kernel.
fn add_ab_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("a"), fp("b")],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: ident("a"),
                rhs: ident("b"),
            },
        },
    }
}

/// `def xpile_kernel(x) -> f64: r = 0.0; if x > 0.0: r = x else: r = -x;
/// return r` — abs() via an if/else with a comparison.
fn abs_if_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("x")],
        return_type: Type::F64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "r".into(),
                    ty: Type::F64,
                    value: Expr::LitFloat(0.0),
                    mutable: true,
                },
                Stmt::If {
                    cond: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: ident("x"),
                        rhs: Box::new(Expr::LitFloat(0.0)),
                    },
                    then_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::Ident("x".into()),
                    }],
                    else_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::UnOp {
                            op: xpile_meta_hir::UnOp::Neg,
                            operand: ident("x"),
                        },
                    }],
                },
            ],
            trailing_return: Expr::Ident("r".into()),
        },
    }
}

/// `def xpile_kernel(x) -> f64: acc = x; while acc > 1.0: acc = acc - 1.0;
/// return acc` — a counting-down while loop with a guard + back-edge.
fn while_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("x")],
        return_type: Type::F64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "acc".into(),
                    ty: Type::F64,
                    value: Expr::Ident("x".into()),
                    mutable: true,
                },
                Stmt::While {
                    cond: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: ident("acc"),
                        rhs: Box::new(Expr::LitFloat(1.0)),
                    },
                    body: vec![Stmt::Assign {
                        name: "acc".into(),
                        value: Expr::FloatBinOp {
                            op: FloatOp::Sub,
                            lhs: ident("acc"),
                            rhs: Box::new(Expr::LitFloat(1.0)),
                        },
                    }],
                },
            ],
            trailing_return: Expr::Ident("acc".into()),
        },
    }
}

/// `def xpile_kernel(x) -> f64: r = 0.0;
/// if (x > 0.0) and (x < 10.0): r = x else: r = 0.0; return r` — an `and`-composed
/// condition (two `setp` + `and.pred`).
fn and_cond_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("x")],
        return_type: Type::F64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "r".into(),
                    ty: Type::F64,
                    value: Expr::LitFloat(0.0),
                    mutable: true,
                },
                Stmt::If {
                    cond: Expr::BinOp {
                        op: BinOp::And,
                        lhs: Box::new(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: ident("x"),
                            rhs: Box::new(Expr::LitFloat(0.0)),
                        }),
                        rhs: Box::new(Expr::BinOp {
                            op: BinOp::Lt,
                            lhs: ident("x"),
                            rhs: Box::new(Expr::LitFloat(10.0)),
                        }),
                    },
                    then_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::Ident("x".into()),
                    }],
                    else_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::LitFloat(0.0),
                    }],
                },
            ],
            trailing_return: Expr::Ident("r".into()),
        },
    }
}

// ─── PMAT-972: abs / min / max / sqrt scalar builtins assemble clean ──

use xpile_meta_hir::NumBuiltinOp;

/// `def xpile_kernel(x) -> f64: return abs(x)` — the single-op `abs.f64`.
fn abs_builtin_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("x")],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::NumBuiltin {
                op: NumBuiltinOp::Abs,
                args: vec![Expr::Ident("x".into())],
                of_float: true,
            },
        },
    }
}

/// `def xpile_kernel(x) -> f64: return math.sqrt(x)` — `sqrt.rn.f64`.
fn sqrt_builtin_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("x")],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::NumBuiltin {
                op: NumBuiltinOp::Sqrt,
                args: vec![Expr::Ident("x".into())],
                of_float: true,
            },
        },
    }
}

/// `def xpile_kernel(x) -> f64: return max(x, 0.0)` — relu via `max.f64`.
fn relu_max_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("x")],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::NumBuiltin {
                op: NumBuiltinOp::Max,
                args: vec![Expr::Ident("x".into()), Expr::LitFloat(0.0)],
                of_float: true,
            },
        },
    }
}

/// `def xpile_kernel(a, b, c) -> f64: return min(a, b, c)` — variadic min
/// folding into two `min.f64`s.
fn min3_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![fp("a"), fp("b"), fp("c")],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::NumBuiltin {
                op: NumBuiltinOp::Min,
                args: vec![
                    Expr::Ident("a".into()),
                    Expr::Ident("b".into()),
                    Expr::Ident("c".into()),
                ],
                of_float: true,
            },
        },
    }
}

#[test]
fn ptxas_assembles_scalar_builtin_kernels() {
    let sm = local_compute_capability();
    let cases: &[(&str, Function)] = &[
        ("abs(x)", abs_builtin_fn()),
        ("sqrt(x)", sqrt_builtin_fn()),
        ("relu max(x,0)", relu_max_fn()),
        ("min(a,b,c)", min3_fn()),
    ];
    for (label, f) in cases {
        let ptx =
            emit_kernel(f, &sm).unwrap_or_else(|e| panic!("PMAT-972 emit failed for {label}: {e}"));
        assert_eq!(
            validate_ptx(&ptx, &sm),
            Ok(()),
            "PMAT-972 {label}: emitted PTX must pass the structural gate:\n{ptx}"
        );
        if !ptxas_available() {
            eprintln!(
                "PMAT-972: skipping ptxas assemble for `{label}` — ptxas not present. \
                 The structural gate passed; a CUDA box assembles this clean."
            );
            continue;
        }
        match ptxas_assemble(&ptx, &sm) {
            Ok(()) => eprintln!(
                "PMAT-972: ptxas ASSEMBLED `{label}` clean for {sm} — the abs/min/max/sqrt \
                 single-op builtin lowering is well-formed for the NVIDIA assembler."
            ),
            Err(stderr) => {
                panic!("PMAT-972: ptxas REJECTED `{label}` for {sm}:\n{stderr}\n--- PTX ---\n{ptx}")
            }
        }
    }
}

#[test]
fn ptxas_assembles_control_flow_and_multi_param_kernels() {
    let sm = local_compute_capability();
    // Each new PMAT-962 construct must pass the pure-text structural gate, and —
    // when ptxas is present — assemble clean for the real NVIDIA assembler.
    let cases: &[(&str, Function)] = &[
        ("multi-param add(a,b)", add_ab_fn()),
        ("if/else abs", abs_if_fn()),
        ("while countdown", while_fn()),
        ("and-composed condition", and_cond_fn()),
    ];
    for (label, f) in cases {
        let ptx =
            emit_kernel(f, &sm).unwrap_or_else(|e| panic!("PMAT-962 emit failed for {label}: {e}"));
        assert_eq!(
            validate_ptx(&ptx, &sm),
            Ok(()),
            "PMAT-962 {label}: emitted PTX must pass the structural gate:\n{ptx}"
        );
        if !ptxas_available() {
            eprintln!(
                "PMAT-962: skipping ptxas assemble for `{label}` — ptxas not present. \
                 The structural gate passed; a CUDA box assembles this clean."
            );
            continue;
        }
        match ptxas_assemble(&ptx, &sm) {
            Ok(()) => eprintln!(
                "PMAT-962: ptxas ASSEMBLED `{label}` clean for {sm} — control-flow / \
                 multi-param PTX is well-formed for the NVIDIA assembler."
            ),
            Err(stderr) => {
                panic!("PMAT-962: ptxas REJECTED `{label}` for {sm}:\n{stderr}\n--- PTX ---\n{ptx}")
            }
        }
    }
}

// ─── PMAT-980: the ARRAY element-wise kernel (`list[scalar]` param) ─────

fn lp(name: &str, elem: Type) -> Param {
    Param {
        name: name.into(),
        ty: Type::List(Box::new(elem)),
        mutable: false,
    }
}

fn index(coll: &str, idx: &str) -> Expr {
    Expr::Index {
        collection: ident(coll),
        index: ident(idx),
    }
}

/// `def xpile_kernel(xs: list[f64]) -> f64: return xs[i] + 1.0` — the canonical
/// GPU element-wise array kernel (`list[scalar]` param read by `xs[i]` at the
/// per-thread index).
fn array_add_one_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![lp("xs", Type::F64)],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(index("xs", "i")),
                rhs: Box::new(Expr::LitFloat(1.0)),
            },
        },
    }
}

/// `def xpile_kernel(xs, ys: list[f64]) -> f64: return xs[i] + ys[i]` — the
/// two-array element-wise add (two indexed `ld.global`s, one `st.global`).
fn array_add_two_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![lp("xs", Type::F64), lp("ys", Type::F64)],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(index("xs", "i")),
                rhs: Box::new(index("ys", "i")),
            },
        },
    }
}

/// `def xpile_kernel(xs: list[f64]) -> f64: return max(xs[i], 0.0)` — relu over
/// an array (the array path composing with the PMAT-972 `max` builtin).
fn array_relu_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![lp("xs", Type::F64)],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::NumBuiltin {
                op: xpile_meta_hir::NumBuiltinOp::Max,
                args: vec![index("xs", "i"), Expr::LitFloat(0.0)],
                of_float: true,
            },
        },
    }
}

#[test]
fn ptxas_assembles_array_element_wise_kernels() {
    let sm = local_compute_capability();
    let cases: &[(&str, Function)] = &[
        ("array xs[i] + 1.0", array_add_one_fn()),
        ("array xs[i] + ys[i]", array_add_two_fn()),
        ("array relu max(xs[i], 0)", array_relu_fn()),
    ];
    for (label, f) in cases {
        let ptx =
            emit_kernel(f, &sm).unwrap_or_else(|e| panic!("PMAT-980 emit failed for {label}: {e}"));
        // The array path emits a real indexed global load + store, not the
        // implicit-scalar prologue load.
        assert!(
            ptx.contains("ld.global") && ptx.contains("st.global"),
            "PMAT-980 {label}: array kernel must load + store global:\n{ptx}"
        );
        assert_eq!(
            validate_ptx(&ptx, &sm),
            Ok(()),
            "PMAT-980 {label}: emitted PTX must pass the structural gate:\n{ptx}"
        );
        if !ptxas_available() {
            eprintln!(
                "PMAT-980: skipping ptxas assemble for `{label}` — ptxas not present. \
                 The structural gate passed; a CUDA box assembles this clean."
            );
            continue;
        }
        match ptxas_assemble(&ptx, &sm) {
            Ok(()) => eprintln!(
                "PMAT-980: ptxas ASSEMBLED `{label}` clean for {sm} — the `list[scalar]` \
                 param + `xs[i]` array element-wise PTX is well-formed for the NVIDIA \
                 assembler (the PTX analog of PMAT-966's wat2wasm array witness)."
            ),
            Err(stderr) => {
                panic!("PMAT-980: ptxas REJECTED `{label}` for {sm}:\n{stderr}\n--- PTX ---\n{ptx}")
            }
        }
    }
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
