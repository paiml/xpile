//! PMAT-482 — offline WGSL + SPIR-V validation corpus (no GPU, free CI).
//!
//! The GPU-lane §29 witnesses (`crates/xpile-{wgsl,spirv}-codegen/tests/
//! gpu_witness.rs`, `gpu_real_kernel.rs`) all SKIP when no Vulkan adapter is
//! present — so on a hosted runner they execute ZERO shader artifacts. And
//! `naga` is the SINGLE shared oracle for BOTH the WGSL lane (front-end
//! parse+validate) and the SPIR-V lane (`wgsl_to_spirv_words` = naga
//! `wgsl-in` + `spv-out`), so a naga regression can silently break SPIR-V.
//!
//! This test converts that skip into a VALIDATED gate that runs on any
//! GPU-less `ubuntu-latest` box — there is NO adapter probe and NO skip
//! path. It:
//!
//!   1. emits the FULL supported-construct WGSL corpus (scalar arithmetic,
//!      comparisons, f32, bitwise, logical short-circuit, if/else + let-var,
//!      while→loop+break, if-expression→select, storage-buffer read, store,
//!      and a read-modify-write while kernel) and runs every shape through
//!      the CPU-only naga front-end (`naga_validate_wgsl`);
//!   2. compiles the compute-shaped shaders through the naga WGSL→SPIR-V
//!      BACKEND (`wgsl_to_spirv_words`) and checks the emitted words with the
//!      in-crate CPU header gate (`validate_spirv`) — no `wgpu` device, no
//!      GPU adapter, no external `spirv-val` binary.
//!
//! Net-new coverage over the existing lib unit tests: those compile ONLY the
//! two saxpy shaders (general + `fma` specialist) to SPIR-V; here the
//! control-flow shape (`loop` + buffer read+write) is compiled to SPIR-V for
//! the first time, so a naga `spv` backend regression on control flow is now
//! caught. Every naga bump re-validates the whole corpus in one named place.

use xpile_meta_hir::{
    BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, SourceLang, Stmt, Type,
};
use xpile_spirv_codegen::{spirv_looks_real, validate_spirv, wgsl_to_spirv_words};
use xpile_wgsl_codegen::{emit_wgsl_module, naga_validate_wgsl, wgsl_looks_real};

// ─── meta-HIR builders ─────────────────────────────────────────────────

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.into(),
        ty,
        mutable: false,
    }
}

fn module(name: &str, f: Function) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items: vec![Item::Function(f)],
        ffi_boundaries: Vec::new(),
    }
}

// ─── corpus shapes (one per supported emitter feature) ─────────────────

fn scalar_add_module() -> Module {
    let f = Function {
        name: "add".into(),
        params: vec![param("a", Type::I64), param("b", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: binop(BinOp::Add, ident("a"), ident("b")),
        },
    };
    module("scalar_add_kernel", f)
}

fn comparison_module() -> Module {
    let f = Function {
        name: "lt".into(),
        params: vec![param("a", Type::I64), param("b", Type::I64)],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: binop(BinOp::Lt, ident("a"), ident("b")),
        },
    };
    module("comparison_kernel", f)
}

fn saxpy_module() -> Module {
    let f = Function {
        name: "saxpy".into(),
        params: vec![param("x", Type::F32)],
        return_type: Type::F32,
        body: Block {
            stmts: vec![],
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(Expr::FloatBinOp {
                    op: FloatOp::Mul,
                    lhs: Box::new(ident("x")),
                    rhs: Box::new(Expr::LitFloat(2.0)),
                }),
                rhs: Box::new(Expr::LitFloat(1.0)),
            },
        },
    };
    module("saxpy_kernel", f)
}

fn bitwise_module() -> Module {
    let f = Function {
        name: "mask".into(),
        params: vec![param("a", Type::CUInt), param("b", Type::CUInt)],
        return_type: Type::CUInt,
        body: Block {
            stmts: vec![],
            trailing_return: binop(
                BinOp::BitOr,
                binop(BinOp::BitAnd, ident("a"), ident("b")),
                ident("a"),
            ),
        },
    };
    module("bitwise_kernel", f)
}

fn logical_module() -> Module {
    let f = Function {
        name: "both".into(),
        params: vec![param("p", Type::Bool), param("q", Type::Bool)],
        return_type: Type::Bool,
        body: Block {
            stmts: vec![],
            trailing_return: binop(BinOp::And, ident("p"), ident("q")),
        },
    };
    module("logical_kernel", f)
}

fn clamp_low_module() -> Module {
    let f = Function {
        name: "clamp_low".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "r".into(),
                    ty: Type::I64,
                    value: ident("n"),
                    mutable: true,
                },
                Stmt::If {
                    cond: binop(BinOp::Lt, ident("n"), Expr::LitInt(0)),
                    then_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::LitInt(0),
                    }],
                    else_body: vec![],
                },
            ],
            trailing_return: ident("r"),
        },
    };
    module("clamp_low_kernel", f)
}

fn count_to_module() -> Module {
    let f = Function {
        name: "count_to".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "i".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                Stmt::While {
                    cond: binop(BinOp::Lt, ident("i"), ident("n")),
                    body: vec![
                        Stmt::If {
                            cond: binop(BinOp::Eq, ident("i"), Expr::LitInt(5)),
                            then_body: vec![Stmt::Break],
                            else_body: vec![],
                        },
                        Stmt::Assign {
                            name: "i".into(),
                            value: binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                        },
                    ],
                },
            ],
            trailing_return: ident("i"),
        },
    };
    module("count_to_kernel", f)
}

fn pick_module() -> Module {
    let f = Function {
        name: "pick".into(),
        params: vec![
            param("c", Type::Bool),
            param("a", Type::I64),
            param("b", Type::I64),
        ],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: Expr::IfExpr {
                cond: Box::new(ident("c")),
                then_expr: Box::new(ident("a")),
                else_expr: Box::new(ident("b")),
            },
        },
    };
    module("pick_kernel", f)
}

fn list_read_module() -> Module {
    let f = Function {
        name: "first".into(),
        params: vec![param("xs", Type::List(Box::new(Type::I64)))],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: Expr::Index {
                collection: Box::new(ident("xs")),
                index: Box::new(Expr::LitInt(0)),
            },
        },
    };
    module("list_read_kernel", f)
}

fn list_write_module() -> Module {
    let f = Function {
        name: "set_first".into(),
        params: vec![param("xs", Type::List(Box::new(Type::I64)))],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![Stmt::IndexAssign {
                list_name: "xs".into(),
                indices: vec![Expr::LitInt(0)],
                value: Expr::LitInt(7),
            }],
            trailing_return: Expr::Unit,
        },
    };
    module("list_write_kernel", f)
}

/// The read-modify-write `while` kernel (`loop` + buffer read+write). This is
/// the shape whose SPIR-V compilation NO existing test covers — the net-new
/// coverage this gate adds, and the RED-mutation target.
fn double_all_module() -> Module {
    let f = Function {
        name: "double_all".into(),
        params: vec![
            param("xs", Type::List(Box::new(Type::F32))),
            param("n", Type::I64),
        ],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "i".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                Stmt::While {
                    cond: binop(BinOp::Lt, ident("i"), ident("n")),
                    body: vec![
                        Stmt::IndexAssign {
                            list_name: "xs".into(),
                            indices: vec![ident("i")],
                            value: Expr::FloatBinOp {
                                op: FloatOp::Mul,
                                lhs: Box::new(Expr::Index {
                                    collection: Box::new(ident("xs")),
                                    index: Box::new(ident("i")),
                                }),
                                rhs: Box::new(Expr::LitFloat(2.0)),
                            },
                        },
                        Stmt::Assign {
                            name: "i".into(),
                            value: binop(BinOp::Add, ident("i"), Expr::LitInt(1)),
                        },
                    ],
                },
            ],
            trailing_return: Expr::Unit,
        },
    };
    module("double_all_kernel", f)
}

/// Every supported meta-HIR shape, one entry per emitter feature.
fn wgsl_corpus() -> Vec<(&'static str, Module)> {
    vec![
        ("scalar_add_i32", scalar_add_module()),
        ("comparison_bool", comparison_module()),
        ("f32_saxpy", saxpy_module()),
        ("bitwise_u32", bitwise_module()),
        ("logical_and", logical_module()),
        ("if_else_let_var", clamp_low_module()),
        ("while_break_continue", count_to_module()),
        ("if_expr_select", pick_module()),
        ("list_read_buffer", list_read_module()),
        ("list_write_buffer", list_write_module()),
        ("rmw_while_kernel", double_all_module()),
    ]
}

/// Full `@compute`-shaped WGSL shaders that the naga `spv` backend compiles.
/// Kept to `@compute` entry shapes (proven-compilable by the existing lib
/// tests) so the SPIR-V half needs no assumption about function-only modules.
fn compute_corpus() -> Vec<(&'static str, String)> {
    let mut v = Vec::new();

    // 1. xpile's REAL emitted saxpy compute shader (general slot), via the
    //    SPIR-V crate's exported real-emit path.
    v.push((
        "saxpy_general_real_emit",
        xpile_spirv_codegen::general_real_wgsl()
            .expect("general real WGSL emit (saxpy) must succeed"),
    ));

    // 2. the scalar saxpy fn wrapped as @compute, via the WGSL crate's path.
    v.push((
        "saxpy_scalar_wrapped",
        xpile_wgsl_codegen::real_emitted_compute_wgsl()
            .expect("real_emitted_compute_wgsl must succeed"),
    ));

    // 3. the while-loop read-modify-write kernel wrapped as @compute — the
    //    control-flow shape whose SPIR-V compilation NO existing test covers.
    let emitted = emit_wgsl_module(&double_all_module())
        .expect("double_all kernel lowers through emit_wgsl_module");
    let wrapped = format!(
        "{emitted}\n\
         @compute @workgroup_size(64)\n\
         fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{\n\
         \x20   if (gid.x == 0u) {{\n\
         \x20       double_all(i32(arrayLength(&double_all_xs)));\n\
         \x20   }}\n\
         }}\n"
    );
    v.push(("double_all_while_rmw", wrapped));

    v
}

// ─── the offline gates (NO adapter probe, NO skip path) ────────────────

#[test]
fn wgsl_emitter_corpus_naga_validates_offline() {
    let corpus = wgsl_corpus();
    assert!(
        corpus.len() >= 10,
        "corpus shrank to {} shapes — an empty/shrunk corpus is a vacuous pass",
        corpus.len()
    );
    for (name, m) in &corpus {
        let wgsl = emit_wgsl_module(m).unwrap_or_else(|e| {
            panic!("[{name}] emit_wgsl_module refused an in-subset shape: {e}")
        });
        assert!(
            wgsl_looks_real(&wgsl),
            "[{name}] emitted WGSL must classify as real, not a scaffold:\n{wgsl}"
        );
        naga_validate_wgsl(&wgsl).unwrap_or_else(|e| {
            panic!("[{name}] emitted WGSL failed the CPU-only naga front-end: {e}\n{wgsl}")
        });
    }
}

#[test]
fn wgsl_corpus_compiles_to_spirv_offline() {
    let corpus = compute_corpus();
    assert!(
        corpus.len() >= 3,
        "compute corpus shrank to {} shaders — vacuous-pass guard",
        corpus.len()
    );
    for (name, wgsl) in &corpus {
        // Shared naga WGSL front-end (parse + validate).
        naga_validate_wgsl(wgsl).unwrap_or_else(|e| {
            panic!("[{name}] compute WGSL failed naga validation: {e}\n{wgsl}")
        });
        // naga WGSL->SPIR-V backend — CPU only, no GPU, no external spirv-val.
        let words = wgsl_to_spirv_words(wgsl)
            .unwrap_or_else(|e| panic!("[{name}] WGSL->SPIR-V compilation failed: {e}\n{wgsl}"));
        assert!(
            spirv_looks_real(&words),
            "[{name}] emitted words are not real SPIR-V (missing magic)"
        );
        validate_spirv(&words)
            .unwrap_or_else(|e| panic!("[{name}] emitted SPIR-V failed the header gate: {e}"));
    }
}

#[test]
fn naga_oracle_is_live_not_a_no_op() {
    // Guard the guard: prove the shared naga oracle actually REJECTS malformed
    // input in this CI context, so a green corpus pass is meaningful (not naga
    // silently accepting everything after a regression).
    assert!(
        naga_validate_wgsl("this is not wgsl {{{").is_err(),
        "naga WGSL front-end must reject garbage"
    );
    assert!(
        wgsl_to_spirv_words("this is not wgsl {{{").is_err(),
        "WGSL->SPIR-V must reject garbage at the parse stage"
    );
    assert!(
        validate_spirv(&[0xdead_beef, 1, 2, 3, 4]).is_err(),
        "validate_spirv must reject a non-magic word stream"
    );
}
