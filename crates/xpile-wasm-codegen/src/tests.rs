//! Unit tests for the native WASM (WAT) emitter (PMAT-951).
//!
//! Asserts the emitted WAT structure for a couple of scalar/control
//! functions, and that constructs outside the scalar/control subset are
//! refused (a Lean-style honest refusal, never wrong code). The executed
//! wasm-runtime witness (running the emitted WAT in a wasm engine and
//! diffing two emitters) is deferred to PMAT-952.

use super::*;
use xpile_backend::{BackendConfig, Profile};
use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};

fn module_with(items: Vec<Item>) -> Module {
    Module {
        name: "m".into(),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

fn wasm_config() -> BackendConfig {
    BackendConfig {
        target: Target::Wasm,
        profile: Profile::RustOut,
        hardware: None,
    }
}

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.into(),
        ty,
        mutable: false,
    }
}

/// `def add(a: int, b: int) -> int: return a + b`
fn add_fn() -> Function {
    Function {
        name: "add".into(),
        params: vec![param("a", Type::I64), param("b", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    }
}

#[test]
fn emits_module_and_func_for_add() {
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(add_fn())]), &wasm_config())
        .unwrap();
    let p = &wat.primary;
    assert!(p.starts_with("(module"), "WAT must open a module: {p}");
    assert!(p.contains("(func $add"), "func decl present: {p}");
    assert!(p.contains("(param $a i64)"), "i64 param: {p}");
    assert!(p.contains("(result i64)"), "i64 result: {p}");
    assert!(p.contains("local.get $a"), "Ident → local.get: {p}");
    assert!(p.contains("i64.add"), "Add → i64.add: {p}");
    assert!(p.contains("(export \"add\" (func $add))"), "exported: {p}");
    assert!(p.contains(";; xpile-contract: C-COMPILE-RUST-TO-WASM"));
    assert!(p.trim_end().ends_with(')'));
}

#[test]
fn cites_the_compile_contract() {
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(add_fn())]), &wasm_config())
        .unwrap();
    assert_eq!(wat.citations.len(), 1);
    assert_eq!(wat.citations[0].as_str(), "C-COMPILE-RUST-TO-WASM");
    assert_eq!(
        wat.quorum_status,
        QuorumStatus::Single {
            emitter: "xpile-wasm-codegen".to_string()
        }
    );
}

#[test]
fn backend_metadata() {
    let b = WasmBackend::new();
    assert_eq!(b.name(), "wasm");
    assert_eq!(b.targets(), &[Target::Wasm]);
}

/// A while-loop counting fn exercising Let / While / Assign / If-break /
/// comparison and the block/loop control shape.
///
/// ```text
/// def count(n: int) -> int:
///     i = 0
///     total = 0
///     while i < n:
///         total = total + i
///         i = i + 1
///     return total
/// ```
fn count_fn() -> Function {
    Function {
        name: "count".into(),
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
                Stmt::Let {
                    name: "total".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                Stmt::While {
                    cond: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("i".into())),
                        rhs: Box::new(Expr::Ident("n".into())),
                    },
                    body: vec![
                        Stmt::Assign {
                            name: "total".into(),
                            value: Expr::BinOp {
                                op: BinOp::Add,
                                lhs: Box::new(Expr::Ident("total".into())),
                                rhs: Box::new(Expr::Ident("i".into())),
                            },
                        },
                        Stmt::Assign {
                            name: "i".into(),
                            value: Expr::BinOp {
                                op: BinOp::Add,
                                lhs: Box::new(Expr::Ident("i".into())),
                                rhs: Box::new(Expr::LitInt(1)),
                            },
                        },
                    ],
                },
            ],
            trailing_return: Expr::Ident("total".into()),
        },
    }
}

#[test]
fn emits_while_loop_block_loop_shape() {
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(count_fn())]),
            &wasm_config(),
        )
        .unwrap();
    let p = &wat.primary;
    // Locals declared up front.
    assert!(p.contains("(local $i i64)"), "i local: {p}");
    assert!(p.contains("(local $total i64)"), "total local: {p}");
    // While → (block $brk (loop $cont … i32.eqz br_if $brk … br $cont)).
    assert!(p.contains("(block $brk"), "brk block: {p}");
    assert!(p.contains("(loop $cont"), "cont loop: {p}");
    assert!(p.contains("i32.eqz"), "cond negated: {p}");
    assert!(p.contains("br_if $brk"), "exit branch: {p}");
    assert!(p.contains("br $cont"), "back-edge: {p}");
    assert!(p.contains("i64.lt_s"), "Lt → i64.lt_s: {p}");
    assert!(p.contains("local.set $total"), "Assign → local.set: {p}");
}

#[test]
fn floordiv_routes_through_helper() {
    let f = Function {
        name: "fd".into(),
        params: vec![param("a", Type::I64), param("b", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::FloorDiv,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(
        wat.contains("$__wasm_floordiv_i64"),
        "floordiv helper: {wat}"
    );
    // The helper definitions are always present.
    assert!(wat.contains("(func $__wasm_floordiv_i64"));
    assert!(wat.contains("(func $__wasm_floormod_i64"));
}

#[test]
fn if_expr_emits_typed_if_result() {
    // def m(a: int, b: int) -> int: return a if a > b else b
    let f = Function {
        name: "maxx".into(),
        params: vec![param("a", Type::I64), param("b", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::IfExpr {
                cond: Box::new(Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                }),
                then_expr: Box::new(Expr::Ident("a".into())),
                else_expr: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(wat.contains("i64.gt_s"), "Gt → i64.gt_s: {wat}");
    assert!(wat.contains("if (result i64)"), "typed if-expr: {wat}");
    assert!(wat.contains("else"));
    assert!(wat.contains("end"));
}

#[test]
fn float_arith_emits_f64_ops() {
    use xpile_meta_hir::FloatOp;
    let f = Function {
        name: "scale".into(),
        params: vec![param("x", Type::F64)],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Mul,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::LitFloat(2.0)),
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(wat.contains("(param $x f64)"), "f64 param: {wat}");
    assert!(wat.contains("f64.const 2.0"), "f64 literal: {wat}");
    assert!(wat.contains("f64.mul"), "FloatOp::Mul → f64.mul: {wat}");
}

#[test]
fn bool_logic_short_circuits() {
    // def both(a: bool, b: bool) -> bool: return a and b
    let f = Function {
        name: "both".into(),
        params: vec![param("a", Type::Bool), param("b", Type::Bool)],
        return_type: Type::Bool,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(wat.contains("(param $a i32)"), "bool → i32 param: {wat}");
    assert!(wat.contains("(result i32)"), "bool → i32 result: {wat}");
    // `a and b` → if a then b else 0.
    assert!(wat.contains("if (result i32)"));
}

// ─── refusal tests (Lean-style honest refusal) ──────────────────────

#[test]
fn refuses_string_type() {
    let f = Function {
        name: "s".into(),
        params: vec![param("x", Type::Str)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Ident("x".into()),
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsupported"), "honest refusal: {msg}");
    assert!(msg.to_lowercase().contains("str") || msg.contains("Str"));
}

#[test]
fn refuses_list_literal_expr() {
    let f = Function {
        name: "l".into(),
        params: Vec::new(),
        return_type: Type::I64,
        body: Block {
            stmts: vec![Stmt::Let {
                name: "z".into(),
                ty: Type::I64,
                value: Expr::ListLit(vec![Expr::LitInt(1)]),
                mutable: false,
            }],
            trailing_return: Expr::Ident("z".into()),
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    assert!(err.to_string().contains("unsupported"));
}

// ─── PMAT-966: first aggregate — list[scalar] param indexed by index ─

/// `def get(xs: list[float], i: int) -> float: return xs[i]`
fn list_get_float_fn() -> Function {
    Function {
        name: "get_f".into(),
        params: vec![
            param("xs", Type::List(Box::new(Type::F64))),
            param("i", Type::I64),
        ],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Index {
                collection: Box::new(Expr::Ident("xs".into())),
                index: Box::new(Expr::Ident("i".into())),
            },
        },
    }
}

#[test]
fn list_float_param_lowers_to_i32_base_pointer_and_memory() {
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(list_get_float_fn())]),
            &wasm_config(),
        )
        .unwrap()
        .primary;
    // The list param rides an i32 base-pointer.
    assert!(
        wat.contains("(param $xs i32)"),
        "list → i32 base ptr: {wat}"
    );
    assert!(wat.contains("(param $i i64)"), "index param i64: {wat}");
    assert!(wat.contains("(result f64)"), "f64 element result: {wat}");
    // A memory is declared + exported once.
    assert!(
        wat.contains("(memory (export \"mem\") 1)"),
        "exported linear memory: {wat}"
    );
    // xs[i] → base + i*8 then f64.load.
    assert!(wat.contains("local.get $xs"), "base ptr loaded: {wat}");
    assert!(wat.contains("i32.wrap_i64"), "index narrowed to i32: {wat}");
    assert!(wat.contains("i32.const 8"), "f64 stride 8: {wat}");
    assert!(
        wat.contains("i32.mul") && wat.contains("i32.add"),
        "addr calc: {wat}"
    );
    assert!(wat.contains("f64.load"), "f64 element load: {wat}");
}

#[test]
fn list_int_param_uses_i64_load_and_stride() {
    // def sum2(xs: list[int]) -> int: return xs[0] + xs[1]
    let f = Function {
        name: "sum2".into(),
        params: vec![param("xs", Type::List(Box::new(Type::I64)))],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Index {
                    collection: Box::new(Expr::Ident("xs".into())),
                    index: Box::new(Expr::LitInt(0)),
                }),
                rhs: Box::new(Expr::Index {
                    collection: Box::new(Expr::Ident("xs".into())),
                    index: Box::new(Expr::LitInt(1)),
                }),
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(
        wat.contains("(param $xs i32)"),
        "list[int] → i32 base: {wat}"
    );
    assert!(wat.contains("i64.load"), "i64 element load: {wat}");
    assert!(wat.contains("i32.const 8"), "i64 stride 8: {wat}");
    assert!(wat.contains("i64.add"), "elements summed: {wat}");
}

#[test]
fn no_memory_emitted_without_list_param() {
    // The scalar-only `add` fn must NOT pull in a (memory …) decl.
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(add_fn())]), &wasm_config())
        .unwrap()
        .primary;
    assert!(
        !wat.contains("(memory"),
        "no memory without a list param: {wat}"
    );
}

#[test]
fn refuses_list_of_bool_param() {
    // list[bool] has no honest WASM load width — refused.
    let f = Function {
        name: "lb".into(),
        params: vec![param("xs", Type::List(Box::new(Type::Bool)))],
        return_type: Type::Bool,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Index {
                collection: Box::new(Expr::Ident("xs".into())),
                index: Box::new(Expr::LitInt(0)),
            },
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsupported"), "honest refusal: {msg}");
    assert!(msg.contains("list element type"), "names the cause: {msg}");
}

#[test]
fn refuses_list_return_type() {
    // Returning a list is outside the read-only-index deliverable.
    let f = Function {
        name: "ident".into(),
        params: vec![param("xs", Type::List(Box::new(Type::I64)))],
        return_type: Type::List(Box::new(Type::I64)),
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Ident("xs".into()),
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    assert!(
        err.to_string().contains("unsupported"),
        "list return refused: {err}"
    );
}

#[test]
fn refuses_index_over_non_list() {
    // Indexing a scalar local (not a list param) is refused.
    let f = Function {
        name: "bad".into(),
        params: vec![param("x", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Index {
                collection: Box::new(Expr::Ident("x".into())),
                index: Box::new(Expr::LitInt(0)),
            },
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsupported"), "honest refusal: {msg}");
    assert!(
        msg.contains("not a `list[scalar]` parameter"),
        "names the cause: {msg}"
    );
}

#[test]
fn refuses_struct_item() {
    let err = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Struct {
                name: "P".into(),
                fields: vec![("x".into(), Type::I64)],
                methods: Vec::new(),
                frozen: false,
                order: false,
            }]),
            &wasm_config(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("struct"));
}

#[test]
fn refuses_wrong_target() {
    let cfg = BackendConfig {
        target: Target::Rust,
        profile: Profile::RustOut,
        hardware: None,
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(add_fn())]), &cfg)
        .unwrap_err();
    assert!(matches!(err, BackendError::UnsupportedTarget(Target::Rust)));
}
