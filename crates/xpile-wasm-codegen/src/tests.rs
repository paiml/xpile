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

// ─── PMAT-968: bounds-checked index + len(xs) over a list param ──────

#[test]
fn list_index_emits_bounds_guard_and_offset() {
    // xs[i] now traps on OOB (i < 0 || i >= len → unreachable) and reads
    // from base+8 (the length-prefixed element region).
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(list_get_float_fn())]),
            &wasm_config(),
        )
        .unwrap()
        .primary;
    // Scratch local declared for the bounds-checked index.
    assert!(
        wat.contains("(local $__wasm_idx i64)"),
        "index scratch local declared: {wat}"
    );
    // Bounds guard: header load + extend + compare + trap.
    assert!(wat.contains("i32.load"), "header length loaded: {wat}");
    assert!(
        wat.contains("i64.extend_i32_u"),
        "header extended to i64 for the compare: {wat}"
    );
    assert!(wat.contains("i64.lt_s"), "i < 0 lower guard: {wat}");
    assert!(wat.contains("i64.le_s"), "len <= i upper guard: {wat}");
    assert!(wat.contains("i32.or"), "guards OR'd: {wat}");
    assert!(wat.contains("unreachable"), "OOB trap: {wat}");
    // Elements at base+8.
    assert!(
        wat.contains("i32.const 8"),
        "element region offset by 8: {wat}"
    );
    assert!(wat.contains("f64.load"), "f64 element load: {wat}");
}

#[test]
fn index_scratch_not_declared_without_index() {
    // A function with no Index must NOT pull in the scratch local.
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(add_fn())]), &wasm_config())
        .unwrap()
        .primary;
    assert!(
        !wat.contains("$__wasm_idx"),
        "no index scratch without an Index: {wat}"
    );
}

#[test]
fn len_of_list_param_reads_header() {
    // def length(xs: list[float]) -> int: return len(xs)
    let f = Function {
        name: "length".into(),
        params: vec![param("xs", Type::List(Box::new(Type::F64)))],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Len(Box::new(Expr::Ident("xs".into()))),
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(wat.contains("(param $xs i32)"), "list → i32 base: {wat}");
    assert!(wat.contains("(result i64)"), "len → i64 result: {wat}");
    // len = i32.load header, zero-extended to i64.
    assert!(wat.contains("local.get $xs"), "base ptr loaded: {wat}");
    assert!(wat.contains("i32.load"), "header load: {wat}");
    assert!(
        wat.contains("i64.extend_i32_u"),
        "extended to the i64 int domain: {wat}"
    );
    // No bounds-guard machinery for a bare len().
    assert!(!wat.contains("unreachable"), "len needs no trap: {wat}");
}

#[test]
fn refuses_len_of_scalar() {
    // len() of a scalar local has no length header — refused.
    let f = Function {
        name: "bad".into(),
        params: vec![param("x", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Len(Box::new(Expr::Ident("x".into()))),
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
fn refuses_len_of_list_literal() {
    // len([1]) — a list literal carries no base-pointer header; refused.
    let f = Function {
        name: "bad".into(),
        params: Vec::new(),
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Len(Box::new(Expr::ListLit(vec![Expr::LitInt(1)]))),
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    assert!(
        err.to_string().contains("unsupported"),
        "len of a non-name collection refused: {err}"
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

// ─── PMAT-978: aggregate WRITE — `xs[i] = v` over a list[scalar] param ──

/// `def set(xs: list[<elem>], i: int, v: <elem>) -> None: xs[i] = v`
fn set_fn(elem: Type) -> Function {
    Function {
        name: "set".into(),
        params: vec![
            param("xs", Type::List(Box::new(elem.clone()))),
            param("i", Type::I64),
            param("v", elem),
        ],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![Stmt::IndexAssign {
                list_name: "xs".into(),
                indices: vec![Expr::Ident("i".into())],
                value: Expr::Ident("v".into()),
            }],
            trailing_return: Expr::Unit,
        },
    }
}

#[test]
fn list_write_float_emits_bounds_guard_offset_and_store() {
    // xs[i] = v over a list[float] param: the SAME bounds guard + base+8
    // offset + stride math as the read path, terminating in f64.store.
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(set_fn(Type::F64))]),
            &wasm_config(),
        )
        .unwrap()
        .primary;
    // list param + value are an i32 base-pointer and an f64.
    assert!(wat.contains("(param $xs i32)"), "list → i32 base: {wat}");
    assert!(wat.contains("(param $v f64)"), "value param f64: {wat}");
    // bounds guard (Python IndexError analogue) is present on the write too.
    assert!(wat.contains("unreachable"), "OOB trap guard: {wat}");
    assert!(wat.contains("i64.le_s"), "len <= i upper guard: {wat}");
    // length-prefixed element region offset.
    assert!(wat.contains("i32.const 8"), "base+8 element offset: {wat}");
    // index×stride address math + wrap to i32.
    assert!(wat.contains("i32.wrap_i64"), "i64 index → i32 addr: {wat}");
    // terminates in the natural-width store (the write opcode).
    assert!(wat.contains("f64.store"), "f64 element store: {wat}");
    // the index is evaluated once into the scratch local.
    assert!(
        wat.contains("local.set $__wasm_idx"),
        "index into scratch: {wat}"
    );
    // a void fn (set returns None) declares no result on its own header
    // (the always-present floor helpers DO carry `(result i64)`, so scope
    // the check to the `$set` signature line).
    let set_header = wat
        .lines()
        .find(|l| l.contains("(func $set "))
        .expect("set header present");
    assert!(
        !set_header.contains("(result"),
        "set is void (no result on its header): {set_header}"
    );
}

#[test]
fn list_write_int_uses_i64_store() {
    // xs[i] = v over a list[int] param uses the i64.store opcode + 8-byte
    // stride (i64 elements are 8 bytes).
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(set_fn(Type::I64))]),
            &wasm_config(),
        )
        .unwrap()
        .primary;
    assert!(wat.contains("(param $v i64)"), "i64 value: {wat}");
    assert!(wat.contains("i64.store"), "i64 element store: {wat}");
    assert!(!wat.contains("f64.store"), "no float store for int: {wat}");
}

#[test]
fn refuses_multi_index_write() {
    // xs[i][j] = v — nested-list element write is outside the single-index
    // deliverable; honest refusal.
    let f = Function {
        name: "set2".into(),
        params: vec![
            param("xs", Type::List(Box::new(Type::F64))),
            param("i", Type::I64),
            param("j", Type::I64),
            param("v", Type::F64),
        ],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![Stmt::IndexAssign {
                list_name: "xs".into(),
                indices: vec![Expr::Ident("i".into()), Expr::Ident("j".into())],
                value: Expr::Ident("v".into()),
            }],
            trailing_return: Expr::Unit,
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsupported"), "honest refusal: {msg}");
    assert!(msg.contains("multi-index"), "names the cause: {msg}");
}

#[test]
fn refuses_write_over_non_list_param() {
    // Index-assigning a scalar local (not a list param) is refused.
    let f = Function {
        name: "bad".into(),
        params: vec![param("x", Type::I64), param("v", Type::I64)],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![Stmt::IndexAssign {
                list_name: "x".into(),
                indices: vec![Expr::LitInt(0)],
                value: Expr::Ident("v".into()),
            }],
            trailing_return: Expr::Unit,
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not a `list[scalar]` parameter"),
        "names the cause: {msg}"
    );
}

#[test]
fn refuses_write_to_list_of_bool() {
    // list[bool] has no honest WASM store width — the list param itself is
    // refused (same as the read side).
    let err = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(set_fn(Type::Bool))]),
            &wasm_config(),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("list element type"),
        "list[bool] refused: {err}"
    );
}

/// PMAT-978 EXECUTED WITNESS — assemble + run the REAL-emitted `xs[i] = v`
/// in WABT and assert the mutated element reads back correctly.
///
/// `wasm-interp --run-all-exports` only runs zero-arg exports and can't
/// populate memory from outside, so we wrap the REAL-emitted `set` module in
/// a self-contained driver: a `(data)` segment lays down the length-prefixed
/// region (i32 count header + packed f64 elements), and a zero-arg exported
/// `e0` calls `$set` with base-pointer 0 and the chosen `(i, v)`, then
/// `f64.load`s the mutated element back and returns it. The returned f64 is
/// diffed against the CPython-equivalent (`xs[i] = v; return xs[i]` → `v`).
/// Gated on `wasm_runtime_available()` — a clean skip on a host without WABT.
#[test]
fn list_write_executes_in_wabt() {
    if !wasm_runtime_available() {
        eprintln!("SKIP list_write_executes_in_wabt: WABT (wat2wasm/wasm-interp) not installed");
        return;
    }

    // The list, the index written, and the value — the CPython-equivalent is
    // `xs=[10.0,20.0,30.0]; xs[1]=99.5; xs[1]` == 99.5.
    let elems: [f64; 3] = [10.0, 20.0, 30.0];
    let write_index: i64 = 1;
    let write_value: f64 = 99.5;
    let expected = write_value;

    // REAL-emitted `set` module (carries `(func $set …)` + `(memory …)`).
    let set_wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(set_fn(Type::F64))]),
            &wasm_config(),
        )
        .unwrap()
        .primary;

    // Build the length-prefixed memory image as a WAT data-string:
    //   bytes 0..4  : i32 element count (little-endian)
    //   bytes 4..8  : padding (the elements start at base+8 for alignment)
    //   bytes 8..   : packed f64 elements (little-endian)
    let mut image: Vec<u8> = Vec::new();
    image.extend_from_slice(&(elems.len() as i32).to_le_bytes());
    image.extend_from_slice(&[0u8; 4]); // pad to the base+8 element offset
    for &e in &elems {
        image.extend_from_slice(&e.to_le_bytes());
    }
    let data_str = wat_data_escape(&image);

    // Splice a `(data …)` segment after the exported memory, and a zero-arg
    // `e0` driver before the module's closing paren. The driver calls $set
    // (base=0, i, v) then reads the mutated element back as the result.
    let mem_line = "  (memory (export \"mem\") 1)\n";
    assert!(
        set_wat.contains(mem_line),
        "emitted set module declares the exported memory: {set_wat}"
    );
    let elem_addr = 8 + (write_index * 8); // base(0) + LIST_ELEMS_OFFSET + i*stride
    let driver = format!(
        "  (data (i32.const 0) \"{data_str}\")\n  \
         (func (export \"e0\") (result f64)\n    \
         i32.const 0\n    i64.const {write_index}\n    f64.const {write_value:?}\n    \
         call $set\n    \
         i32.const {elem_addr}\n    f64.load)\n"
    );
    let witness_wat = set_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);

    // Assemble + run via WABT (reuse the engine's parse: e0 → f64).
    let engine = WasmDiffExecEngine::new();
    let out = engine
        .assemble_run_parse(&witness_wat, "list_write")
        .expect("assemble+run REAL-emitted list-write witness");
    assert_eq!(out.len(), 1, "single e0 export: {out:?}");
    let got = out[0];
    assert!(
        (got - expected).abs() <= 1.0e-9,
        "list-write witness: wrote {write_value} at index {write_index}, read back {got}, expected (CPython) {expected}"
    );

    eprintln!("=== PMAT-978 executed witness: REAL xpile xs[i]=v emit → run ===");
    eprintln!("--- witness WAT (REAL-emitted $set + data + e0 driver) ---\n{witness_wat}");
    eprintln!("wrote {write_value} at xs[{write_index}] (init {elems:?}); read back {got}; CPython-equivalent {expected}");
}

/// Escape a byte image into a WAT data-string literal (every byte as a
/// `\\HH` hex escape — unambiguous and WABT-accepted).
fn wat_data_escape(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 4);
    for &b in bytes {
        s.push_str(&format!("\\{b:02x}"));
    }
    s
}
