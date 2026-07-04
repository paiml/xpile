//! Unit tests for the native WASM (WAT) emitter (PMAT-951).
//!
//! Asserts the emitted WAT structure for a couple of scalar/control
//! functions, and that constructs outside the scalar/control subset are
//! refused (a Lean-style honest refusal, never wrong code). The executed
//! wasm-runtime witness (running the emitted WAT in a wasm engine and
//! diffing two emitters) is deferred to PMAT-952.

use super::*;
use xpile_backend::{BackendConfig, Profile};
use xpile_meta_hir::{
    BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, StrMethodOp, Type, UnOp,
};

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
fn str_return_of_param_is_identity() {
    // PMAT-993 (slice 2): RETURNING a `str` is now supported — the function's
    // result is an i32 base-pointer. Returning a str PARAM directly is the
    // identity case (return the same string), lowering to `(result i32)` +
    // `local.get $x`. Slice 1 refused this; slice 2's heap path accepts it.
    let f = Function {
        name: "s".into(),
        params: vec![param("x", Type::Str)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Ident("x".into()),
        },
    };
    let art = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .expect("str return (identity over a str param) lowers in slice 2");
    assert!(
        art.primary.contains("(func $s (param $x i32) (result i32)"),
        "str return → i32 result:\n{}",
        art.primary
    );
    assert!(
        art.primary.contains("local.get $x"),
        "identity str return → local.get the param pointer:\n{}",
        art.primary
    );
}

#[test]
fn str_return_of_non_str_local_is_refused() {
    // A `str` return whose trailing expr is NOT string-valued (an int local)
    // must be refused honestly — the heap path returns a string pointer, not a
    // scalar reinterpreted as one.
    let f = Function {
        name: "s".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Ident("n".into()),
        },
    };
    let err = emit_module(&module_with(vec![Item::Function(f)])).unwrap_err();
    assert!(
        err.to_string().contains("not a `str` parameter"),
        "str return of an int local is refused: {err}"
    );
}

// ─── PMAT-1060: str(int) — i64 → decimal-ASCII heap string ───────────

/// `def to_s(n: int) -> str: return str(n)` — the supported int→str shape.
fn to_s_fn() -> Function {
    Function {
        name: "to_s".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::ToStr {
                value: Box::new(Expr::Ident("n".into())),
                of_float: false,
            },
        },
    }
}

#[test]
fn str_int_emits_helper_call_and_heap() {
    let wat = emit_module(&module_with(vec![Item::Function(to_s_fn())]))
        .expect("str(int) program lowers");
    // The self-contained int→str helper + its call site + the bump allocator
    // (str(int) materialises a fresh heap string).
    assert!(
        wat.contains("(func $__wasm_int_to_str (param $n i64) (result i32)"),
        "int→str helper emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_int_to_str"),
        "$to_s calls the helper:\n{wat}"
    );
    assert!(wat.contains("(func $__alloc"), "bump heap present:\n{wat}");
    assert!(
        wat.contains("(func $to_s (param $n i64) (result i32)"),
        "str return → i32 heap pointer:\n{wat}"
    );
}

#[test]
fn str_int_binds_a_str_local_and_returns_it() {
    // `def f(n: int) -> str: s = str(n); return s` — str(int) feeding a str
    // local, then returned. Exercises the str-Let path (emit_str_expr over
    // ToStr) plus the str-name registration.
    let f = Function {
        name: "f".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::Str,
        body: Block {
            stmts: vec![Stmt::Let {
                name: "s".into(),
                ty: Type::Str,
                value: Expr::ToStr {
                    value: Box::new(Expr::Ident("n".into())),
                    of_float: false,
                },
                mutable: false,
            }],
            trailing_return: Expr::Ident("s".into()),
        },
    };
    let wat =
        emit_module(&module_with(vec![Item::Function(f)])).expect("s = str(n); return s lowers");
    assert!(
        wat.contains("call $__wasm_int_to_str"),
        "helper call:\n{wat}"
    );
    assert!(wat.contains("local.set $s"), "str local bound:\n{wat}");
}

#[test]
fn str_float_is_refused() {
    // `str(x)` over a FLOAT is refused — a float→decimal repr (shortest
    // round-trip) is a separate, larger job, never a silent str(int) reuse.
    let f = Function {
        name: "g".into(),
        params: vec![param("x", Type::F64)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::ToStr {
                value: Box::new(Expr::Ident("x".into())),
                of_float: true,
            },
        },
    };
    let err = emit_module(&module_with(vec![Item::Function(f)])).unwrap_err();
    assert!(
        err.to_string().contains("str(float)"),
        "str(float) is refused honestly: {err}"
    );
}

#[test]
fn str_of_bool_operand_is_refused_as_type_mismatch() {
    // `str(b)` where `b` is a bool (i32) — a bool lowers to i32, not the i64
    // the int→str helper needs, so the operand type check refuses it rather
    // than converting a 0/1 as if it were an int (Python str(bool) is
    // "True"/"False", a distinct desugar this lane does not implement).
    let f = Function {
        name: "h".into(),
        params: vec![param("b", Type::Bool)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::ToStr {
                value: Box::new(Expr::Ident("b".into())),
                of_float: false,
            },
        },
    };
    let err = emit_module(&module_with(vec![Item::Function(f)])).unwrap_err();
    assert!(
        err.to_string().contains("type mismatch"),
        "str(bool) is refused as a type mismatch: {err}"
    );
}

// ─── PMAT-986: str param + len(s) + ord(s[i]) ───────────────────────

/// `def code_sum(s: str) -> int:
///      total = 0; i = 0
///      while i < len(s): total = total + ord(s[i]); i = i + 1
///      return total`
fn code_sum_module() -> Module {
    let acc_step = Stmt::Assign {
        name: "total".into(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("total".into())),
            rhs: Box::new(Expr::Ord {
                value: Box::new(Expr::StrCharAt {
                    string: Box::new(Expr::Ident("s".into())),
                    index: Box::new(Expr::Ident("i".into())),
                }),
            }),
        },
    };
    let i_step = Stmt::Assign {
        name: "i".into(),
        value: Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::LitInt(1)),
        },
    };
    let f = Function {
        name: "code_sum".into(),
        params: vec![param("s", Type::Str)],
        return_type: Type::I64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "total".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                Stmt::Let {
                    name: "i".into(),
                    ty: Type::I64,
                    value: Expr::LitInt(0),
                    mutable: true,
                },
                Stmt::While {
                    cond: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("i".into())),
                        rhs: Box::new(Expr::Len(Box::new(Expr::Ident("s".into())))),
                    },
                    body: vec![acc_step, i_step],
                },
            ],
            trailing_return: Expr::Ident("total".into()),
        },
    };
    module_with(vec![Item::Function(f)])
}

#[test]
fn str_param_lowers_to_i32_base_pointer_and_memory() {
    let wat = emit_module(&code_sum_module()).expect("str-param program lowers");
    assert!(
        wat.contains("(param $s i32)"),
        "str param → i32 base-pointer: {wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\") 1)"),
        "str param triggers the linear-memory declaration: {wat}"
    );
}

#[test]
fn len_over_str_param_counts_chars() {
    let wat = emit_module(&code_sum_module()).expect("str-param program lowers");
    // PMAT-1032: len(s) over a STR is the CHAR count (Python counts code
    // points) via the charlen helper — NOT the byte-count header read a
    // list/dict len does ("héllo" is 6 bytes but len 5).
    assert!(
        wat.contains("call $__wasm_str_charlen") && wat.contains("i64.extend_i32_u"),
        "len(s) → charlen helper + i64 extend: {wat}"
    );
    assert!(
        wat.contains("(func $__wasm_str_charlen"),
        "the charlen helper is emitted for a str-touching module: {wat}"
    );
}

#[test]
fn ord_str_index_decodes_char_with_bounds_guard() {
    let wat = emit_module(&code_sum_module()).expect("str-param program lowers");
    // PMAT-1032: ord(s[i]) is a CHAR-indexed UTF-8 decode via the ord_at
    // helper (negative-index normalisation + bounds trap live inside it).
    assert!(
        wat.contains("call $__wasm_str_ord_at"),
        "ord(s[i]) → char-indexed decode helper: {wat}"
    );
    assert!(
        wat.contains("(func $__wasm_str_ord_at") && wat.contains("i32.load8_u"),
        "the ord_at helper decodes UTF-8 bytes: {wat}"
    );
    assert!(
        wat.contains("unreachable ;; string index out of range"),
        "char indexing carries the bounds trap (Python IndexError): {wat}"
    );
}

#[test]
fn strcharat_as_string_materialises_one_char() {
    // PMAT-994 (slice 3a): `s[i]` used as a 1-char STRING (a StrCharAt NOT
    // wrapped in ord) now materialises a NEW 1-char heap string — the `chr`
    // mirror, copying byte `i` of `s` (bounds-checked) into a fresh alloc(9).
    // `def first(s: str) -> str: return s[0]`.
    let f = Function {
        name: "first".into(),
        params: vec![param("s", Type::Str)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrCharAt {
                string: Box::new(Expr::Ident("s".into())),
                index: Box::new(Expr::LitInt(0)),
            },
        },
    };
    let wat =
        emit_module(&module_with(vec![Item::Function(f)])).expect("s[i] -> 1-char str lowers");
    assert!(
        wat.contains("call $__alloc"),
        "s[i] allocates a new 1-char heap string:\n{wat}"
    );
    assert!(
        wat.contains("i32.load8_u") && wat.contains("i32.store8"),
        "s[i] copies the source byte (load8_u) into the new string (store8):\n{wat}"
    );
    assert!(
        wat.contains("unreachable"),
        "s[i] is bounds-checked (traps on OOB, the IndexError analogue):\n{wat}"
    );
    assert!(
        wat.contains("(func $first (param $s i32) (result i32)"),
        "s[i]-returning fn → i32 result (the new string pointer):\n{wat}"
    );
}

#[test]
fn chr_returns_a_new_one_char_string() {
    // PMAT-993 (slice 2), char-exact since PMAT-1032: `chr(n)` materialises a
    // NEW 1-char string in the bump heap — the full 1..4-byte UTF-8 encoding
    // via the `$__wasm_chr` helper, range-guarded to Python's
    // `0..=0x10FFFF` (the pre-PMAT-1032 lowering masked `n & 0xFF` into a
    // single byte, silently wrong for every n > 127).
    let f = Function {
        name: "to_char".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Chr {
                value: Box::new(Expr::Ident("n".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("chr(n) -> str lowers");
    assert!(
        wat.contains("call $__wasm_chr"),
        "chr(n) delegates to the char-exact helper:\n{wat}"
    );
    assert!(
        wat.contains("call $__alloc") && wat.contains("i32.store8"),
        "the chr helper allocates and writes UTF-8 bytes:\n{wat}"
    );
    assert!(
        wat.contains("i64.const 1114111")
            && wat.contains("unreachable ;; chr() arg not in range(0x110000)"),
        "chr carries the 0..=0x10FFFF range trap (Python ValueError):\n{wat}"
    );
    assert!(
        !wat.contains("i32.const 255"),
        "the old n & 0xFF single-byte mask must be gone:\n{wat}"
    );
    assert!(
        wat.contains("(func $to_char (param $n i64) (result i32)"),
        "chr-returning fn → i32 result (the string pointer):\n{wat}"
    );
}

#[test]
fn chr_bound_to_int_local_is_type_mismatch() {
    // `chr(n)` returns an i32 string pointer; binding it to an `int` (i64)
    // local is an HONEST type mismatch (NOT a silent reinterpret). This guards
    // against a code path that would treat a string pointer as an integer.
    let f = Function {
        name: "c".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::I64,
        body: Block {
            stmts: vec![Stmt::Let {
                name: "x".into(),
                ty: Type::I64,
                value: Expr::Chr {
                    value: Box::new(Expr::Ident("n".into())),
                },
                mutable: false,
            }],
            trailing_return: Expr::Ident("x".into()),
        },
    };
    let err = emit_module(&module_with(vec![Item::Function(f)])).unwrap_err();
    assert!(
        err.to_string().contains("type mismatch"),
        "chr(n) bound to an int local is a type mismatch: {err}"
    );
}

#[test]
fn concat_lowers_to_alloc_and_memory_copy() {
    // PMAT-993: `def join(a, b) -> str: return a + b` lowers to the heap
    // allocator + a memory.copy of each operand's bytes, returning the new
    // string's i32 pointer.
    let f = Function {
        name: "join".into(),
        params: vec![param("a", Type::Str), param("b", Type::Str)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Concat {
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("concat -> str lowers");
    assert!(
        wat.contains("(global $__heap_ptr (mut i32)") && wat.contains("(func $__alloc"),
        "the bump allocator is emitted:\n{wat}"
    );
    assert!(
        wat.contains("call $__alloc") && wat.contains("memory.copy"),
        "concat allocates + byte-copies:\n{wat}"
    );
    assert!(
        wat.contains("(func $join (param $a i32) (param $b i32) (result i32)"),
        "str-returning concat fn → i32 result:\n{wat}"
    );
    assert!(
        wat.contains("(local $__wasm_concat_dst i32)"),
        "concat declares its DEDICATED destination scratch local (PMAT-998: \
         distinct from $__wasm_str_dst so an operand's string-returning eval \
         cannot clobber it):\n{wat}"
    );
}

#[test]
fn concat_of_three_str_params_is_single_pass() {
    // Left-nested `(a + b) + c` flattens to one alloc + three memory.copy's.
    let f = Function {
        name: "join3".into(),
        params: vec![
            param("a", Type::Str),
            param("b", Type::Str),
            param("c", Type::Str),
        ],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Concat {
                lhs: Box::new(Expr::Concat {
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                }),
                rhs: Box::new(Expr::Ident("c".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("3-way concat lowers");
    // Count within the FUNCTION body only — the PMAT-1032 char helpers
    // ($__wasm_str_char_at / $__wasm_chr) legitimately carry their own
    // `call $__alloc`, so a module-wide count would see those too.
    let body_start = wat.find("(func $join3").expect("join3 emitted");
    let body = &wat[body_start..];
    let body_end = body.find("(export").unwrap_or(body.len());
    let body = &body[..body_end];
    assert_eq!(
        body.matches("call $__alloc").count(),
        1,
        "3-way concat allocates ONCE (single pass), not per nesting:\n{wat}"
    );
    assert_eq!(
        body.matches("memory.copy").count(),
        3,
        "3-way concat copies each of the 3 operands' bytes:\n{wat}"
    );
}

#[test]
fn string_literal_in_concat_lowers_to_static_data() {
    // PMAT-994 (slice 3a): a string LITERAL operand (`"Hi " + s`) is
    // materialised into a static `(data …)` segment in [LITERAL_BASE, HEAP_BASE)
    // and lowered to a constant `i32.const <base>` pointer — so the literal
    // composes with the concat heap path. `def g(s: str) -> str: return "Hi " + s`.
    let f = Function {
        name: "g".into(),
        params: vec![param("s", Type::Str)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Concat {
                lhs: Box::new(Expr::LitStr("Hi ".into())),
                rhs: Box::new(Expr::Ident("s".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("literal concat lowers");
    // The literal "Hi " (3 bytes) is laid down at LITERAL_BASE (512): a
    // length-prefixed (data) — the i32 count header (\03\00\00\00) then the
    // UTF-8 bytes (\48\69\20 = "Hi ").
    assert!(
        wat.contains("(data (i32.const 512) \"\\03\\00\\00\\00\")"),
        "literal byte-count header data segment @ 512:\n{wat}"
    );
    assert!(
        wat.contains("(data (i32.const 520) \"\\48\\69\\20\")"),
        "literal UTF-8 bytes data segment @ 520:\n{wat}"
    );
    // The literal lowers to a constant pointer (i32.const 512); the concat then
    // bump-allocates + memory.copies it with the param.
    assert!(
        wat.contains("i32.const 512")
            && wat.contains("call $__alloc")
            && wat.contains("memory.copy"),
        "literal pointer (i32.const 512) flows into the concat heap path:\n{wat}"
    );
}

#[test]
fn no_heap_emitted_without_string_returning_op() {
    // A read-only str program (slice 1's code_sum) must NOT pull in the bump
    // allocator — the heap is gated on string MATERIALISATION only.
    let wat = emit_module(&code_sum_module()).expect("code_sum lowers");
    assert!(
        !wat.contains("$__heap_ptr") && !wat.contains("$__alloc"),
        "no allocator for a read-only str program:\n{wat}"
    );
}

#[test]
fn refuses_binop_add_over_str_pointers_as_pointer_arithmetic() {
    // PMAT-993: a raw `BinOp::Add` over two str BASE-POINTERS is meaningless
    // pointer arithmetic, NOT string concat (string `+` lowers as
    // `Expr::Concat`, which IS supported via the heap path). Refuse it honestly
    // and point the caller at `Concat` — never silently add the pointers.
    let f = Function {
        name: "cat".into(),
        params: vec![param("a", Type::Str), param("b", Type::Str)],
        return_type: Type::I64,
        body: Block {
            stmts: vec![Stmt::Let {
                name: "z".into(),
                ty: Type::I64,
                value: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
                mutable: false,
            }],
            trailing_return: Expr::Ident("z".into()),
        },
    };
    let err = emit_module(&module_with(vec![Item::Function(f)])).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("pointer arithmetic") && msg.contains("Concat"),
        "BinOp::Add over str pointers refused, pointing at Concat: {msg}"
    );
}

#[test]
fn string_equality_uses_content_compare_not_pointer_compare() {
    // PMAT-994 (slice 3a): `a == b` over str params lowers to a CONTENT compare
    // (`$__wasm_str_eq`: length check + byte-compare loop → i32 bool), NEVER a
    // base-pointer `i32.eq`. `def eq(a: str, b: str) -> bool: return a == b`.
    let f = Function {
        name: "eq".into(),
        params: vec![param("a", Type::Str), param("b", Type::Str)],
        return_type: Type::Bool,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("str == str lowers");
    assert!(
        wat.contains("(func $__wasm_str_eq"),
        "str equality emits the content-compare helper:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_eq"),
        "str == str routes to the content-compare helper:\n{wat}"
    );
    // Crucially: the function body must NOT compare the two pointers directly
    // with `i32.eq` (that would be a base-pointer compare, the wrong answer).
    // The `$eq` body should be `local.get a; local.get b; call $__wasm_str_eq`.
    let eq_body = wat
        .split("(func $eq ")
        .nth(1)
        .expect("the $eq function is emitted");
    assert!(
        !eq_body.contains("i32.eq\n") && !eq_body.contains("i32.eq "),
        "no raw pointer-compare leaked in $eq body:\n{eq_body}"
    );
}

#[test]
fn string_inequality_negates_content_compare() {
    // PMAT-994: `a != b` is `!(a == b)` — the content compare, negated via
    // i32.eqz. `def neq(a: str, b: str) -> bool: return a != b`.
    let f = Function {
        name: "neq".into(),
        params: vec![param("a", Type::Str), param("b", Type::Str)],
        return_type: Type::Bool,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::NotEq,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("str != str lowers");
    let neq_body = wat
        .split("(func $neq ")
        .nth(1)
        .expect("the $neq function is emitted");
    assert!(
        neq_body.contains("call $__wasm_str_eq") && neq_body.contains("i32.eqz"),
        "str != str is the content compare negated (i32.eqz):\n{neq_body}"
    );
}

#[test]
fn string_ordering_is_lexicographic_cmp_not_pointer_compare() {
    // PMAT-1059: string ORDERING (`a < b`) lowers to the byte-wise lexicographic
    // 3-way compare `$__wasm_str_cmp` tested against 0 with a signed op — REAL
    // string-content logic, NEVER a base-pointer compare on the i32 pointers.
    let f = Function {
        name: "lt".into(),
        params: vec![param("a", Type::Str), param("b", Type::Str)],
        return_type: Type::Bool,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::Lt,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("str < str lowers");
    // The helper is emitted and the $lt body calls it, comparing the 3-way
    // result against 0 with i32.lt_s (the compare is on the CMP RESULT, not a
    // raw base-pointer compare of $a/$b before the call).
    assert!(
        wat.contains("(func $__wasm_str_cmp (param $a i32) (param $b i32) (result i32)"),
        "the lexicographic cmp helper must be emitted:\n{wat}"
    );
    let lt_body = wat
        .split("(func $lt ")
        .nth(1)
        .expect("the $lt function is emitted");
    assert!(
        lt_body.contains("call $__wasm_str_cmp"),
        "str < str must call the cmp helper:\n{lt_body}"
    );
    assert!(
        lt_body.contains("i32.const 0") && lt_body.contains("i32.lt_s"),
        "the cmp result is tested against 0 with i32.lt_s:\n{lt_body}"
    );
    // A pure-ordering module reads memory but allocates nothing.
    assert!(
        !wat.contains("(func $__alloc"),
        "a pure ordering module must NOT carry the bump allocator:\n{wat}"
    );
}

#[test]
fn startswith_endswith_emit_helper_and_call_no_alloc() {
    // PMAT-1126: `s.startswith(p)` / `s.endswith(p)` lower to the non-allocating
    // byte prefix/suffix helper + call — a bool (i32) result, NEVER a
    // base-pointer compare, and NO bump allocator (a predicate allocates nothing).
    for (op, helper) in [
        (StrMethodOp::StartsWith, "$__wasm_str_startswith"),
        (StrMethodOp::EndsWith, "$__wasm_str_endswith"),
    ] {
        let f = Function {
            name: "pred".into(),
            params: vec![param("s", Type::Str), param("p", Type::Str)],
            return_type: Type::Bool,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::StrMethod {
                    recv: Box::new(Expr::Ident("s".into())),
                    op,
                    args: vec![Expr::Ident("p".into())],
                },
            },
        };
        let wat = emit_module(&module_with(vec![Item::Function(f)]))
            .unwrap_or_else(|e| panic!("s.{op:?}(p) lowers: {e:?}"));
        assert!(
            wat.contains(&format!(
                "(func {helper} (param $s i32) (param $p i32) (result i32)"
            )),
            "the {helper} helper is emitted for {op:?}:\n{wat}"
        );
        let body = wat
            .split("(func $pred ")
            .nth(1)
            .expect("the $pred function is emitted");
        assert!(
            body.contains(&format!("call {helper}")),
            "$pred calls {helper} for {op:?}:\n{body}"
        );
        assert!(
            wat.contains("(memory"),
            "the predicate reads str bytes → memory declared:\n{wat}"
        );
        assert!(
            !wat.contains("(func $__alloc"),
            "a pure predicate module carries no bump allocator:\n{wat}"
        );
    }
}

#[test]
fn startswith_only_module_carries_no_endswith_helper() {
    // PMAT-1126: each helper is gated separately — a startswith-only module
    // must NOT carry a dead endswith helper (the "no dead helper" discipline).
    let f = Function {
        name: "pred".into(),
        params: vec![param("s", Type::Str), param("p", Type::Str)],
        return_type: Type::Bool,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::Ident("s".into())),
                op: StrMethodOp::StartsWith,
                args: vec![Expr::Ident("p".into())],
            },
        },
    };
    let wat = emit_module(&module_with(vec![Item::Function(f)])).expect("startswith lowers");
    assert!(
        wat.contains("$__wasm_str_startswith"),
        "startswith helper present:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_endswith"),
        "no dead endswith helper in a startswith-only module:\n{wat}"
    );
}

#[test]
fn refuses_string_mul_no_pointer_arithmetic() {
    // PMAT-1059: an unwired string binop (e.g. `s * n` reaching BinOp::Mul over
    // a str operand) is still an honest refusal, never pointer arithmetic.
    let f = Function {
        name: "bad".into(),
        params: vec![param("a", Type::Str), param("b", Type::Str)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::BinOp {
                op: BinOp::Mul,
                lhs: Box::new(Expr::Ident("a".into())),
                rhs: Box::new(Expr::Ident("b".into())),
            },
        },
    };
    let err = emit_module(&module_with(vec![Item::Function(f)])).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unsupported") && msg.contains("str"),
        "str * str honestly refused (not wired): {msg}"
    );
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

/// `def total(xs: list[<elem>]) -> <ret>: return sum(xs)` — the PMAT-1248
/// list-INT-SUM fixture. `of_float` selects the `Expr::Sum` variant; a mismatch
/// with `elem` is used deliberately by the refusal tests.
fn list_sum_fn(elem: Type, ret: Type, of_float: bool) -> Function {
    Function {
        name: "total".into(),
        params: vec![param("xs", Type::List(Box::new(elem)))],
        return_type: ret,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Sum {
                list: Box::new(Expr::Ident("xs".into())),
                of_float,
                start: None,
            },
        },
    }
}

#[test]
fn list_int_sum_emits_reduction_helper_and_call() {
    // PMAT-1248: `sum(xs)` over a list[int] emits the `$__wasm_list_sum_i64`
    // reduction helper, a `call` to it over the list base-pointer, and the
    // exported linear memory the payload loads read.
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(list_sum_fn(
                Type::I64,
                Type::I64,
                false,
            ))]),
            &wasm_config(),
        )
        .unwrap()
        .primary;
    assert!(
        wat.contains("(func $__wasm_list_sum_i64 (param $base i32) (result i64)"),
        "reduction helper declared: {wat}"
    );
    assert!(
        wat.contains("call $__wasm_list_sum_i64"),
        "sum lowers to a helper call: {wat}"
    );
    assert!(
        wat.contains("(param $xs i32)"),
        "list[int] param is an i32 base-pointer: {wat}"
    );
    assert!(
        wat.contains("(memory (export \"mem\") 1)"),
        "list payload loads need the exported memory: {wat}"
    );
    // The reduction folds with i64.add over an 8-byte stride (i64 elements).
    assert!(wat.contains("i64.add"), "i64 accumulation: {wat}");
    assert!(wat.contains("i64.load"), "i64 element load: {wat}");
}

#[test]
fn list_int_sum_helper_absent_without_use() {
    // No `sum(xs)` → no dead reduction helper (the codebase's no-dead-helper
    // discipline). A plain `xs[0] + xs[1]` module reads memory but never sums.
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(list_sum_fn2_index())]),
            &wasm_config(),
        )
        .unwrap()
        .primary;
    assert!(
        !wat.contains("$__wasm_list_sum_i64"),
        "no reduction helper without a sum(): {wat}"
    );
}

/// `def total(xs: list[int]) -> int: return xs[0] + xs[1]` — a list module that
/// reads memory but performs NO `sum()`, guarding the no-dead-helper gate.
fn list_sum_fn2_index() -> Function {
    Function {
        name: "total".into(),
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
    }
}

#[test]
fn list_int_sum_nested_in_str_repeat_count_gates_helper() {
    // PMAT-1248 gate-hole regression (found by adversarial self-review): a
    // `sum(xs)` nested in a `seq * count` repeat's COUNT (`"ab" * sum(xs)`) must
    // still declare `$__wasm_list_sum_i64` — the `expr_has_list_sum` gate walker
    // has to recurse into `Expr::Repeat.n`, or the emitted `call` would reference
    // an undeclared helper (a hard wat2wasm failure). Before the `Expr::Repeat`
    // arm was added the walker fell through `_ => false` here.
    let f = Function {
        name: "f".into(),
        params: vec![param("xs", Type::List(Box::new(Type::I64)))],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Repeat {
                seq: Box::new(Expr::LitStr("ab".into())),
                n: Box::new(Expr::Sum {
                    list: Box::new(Expr::Ident("xs".into())),
                    of_float: false,
                    start: None,
                }),
                of_str: true,
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap()
        .primary;
    assert!(
        wat.contains("(func $__wasm_list_sum_i64 (param $base i32) (result i64)"),
        "nested-in-repeat-count sum still declares the reduction helper: {wat}"
    );
    assert!(
        wat.contains("call $__wasm_list_sum_i64"),
        "the nested sum lowers to a helper call: {wat}"
    );
}

#[test]
fn list_float_sum_refused_honestly() {
    // PMAT-1248: `sum(xs)` over a list[float] (`Expr::Sum { of_float: true }`)
    // is refused — the i64 helper would mis-reduce f64 elements. Honest error,
    // never a silent miscompile.
    let err = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(list_sum_fn(
                Type::F64,
                Type::F64,
                true,
            ))]),
            &wasm_config(),
        )
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("list[float]") && err.contains("sum"),
        "float sum refused honestly: {err}"
    );
}

#[test]
fn list_sum_with_start_refused_honestly() {
    // PMAT-1248: `sum(xs, start)` with an explicit start is refused — only the
    // 1-arg form is emitted.
    let f = Function {
        name: "total".into(),
        params: vec![param("xs", Type::List(Box::new(Type::I64)))],
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Sum {
                list: Box::new(Expr::Ident("xs".into())),
                of_float: false,
                start: Some(Box::new(Expr::LitInt(100))),
            },
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err()
        .to_string();
    assert!(err.contains("start"), "explicit start refused: {err}");
}

#[test]
fn list_sum_of_non_name_refused_honestly() {
    // PMAT-1248: `sum([1, 2, 3])` (a list LITERAL, not a name) is refused —
    // the WASM subset sums a named list base-pointer.
    let f = Function {
        name: "total".into(),
        params: Vec::new(),
        return_type: Type::I64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::Sum {
                list: Box::new(Expr::ListLit(vec![
                    Expr::LitInt(1),
                    Expr::LitInt(2),
                    Expr::LitInt(3),
                ])),
                of_float: false,
                start: None,
            },
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(f)]), &wasm_config())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("non-name list") || err.contains("bind it to a name"),
        "list-literal sum refused: {err}"
    );
}

/// PMAT-1248 EXECUTED WITNESS — assemble + run the REAL-emitted `sum(xs)` over a
/// list[int] in WABT and diff the executed total against CPython.
///
/// `wasm-interp --run-all-exports` can't populate memory from outside, so the
/// REAL-emitted `$total` module is wrapped in a self-contained driver: a `(data)`
/// segment lays down TWO length-prefixed list regions — a NON-EMPTY list at base
/// 0 and an EMPTY list (count 0) at base 128 — and two zero-arg exports call
/// `$total` on each, converting the i64 total to f64 (`f64.convert_i64_s`) so the
/// engine's `=> f64:` parser reads it. The results are diffed against the
/// CPython-equivalent (`sum(elems)` and `sum([]) == 0`). Gated on
/// `wasm_runtime_available()` — a clean skip on a host without WABT.
#[test]
fn list_int_sum_executes_in_wabt() {
    if !wasm_runtime_available() {
        eprintln!("SKIP list_int_sum_executes_in_wabt: WABT (wat2wasm/wasm-interp) not installed");
        return;
    }

    // CPython-equivalent: sum([5, -3, 10, 7, -1]) == 18; sum([]) == 0.
    let elems: [i64; 5] = [5, -3, 10, 7, -1];
    let expected_nonempty: i64 = elems.iter().sum();
    let expected_empty: i64 = 0;

    let total_wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(list_sum_fn(
                Type::I64,
                Type::I64,
                false,
            ))]),
            &wasm_config(),
        )
        .unwrap()
        .primary;

    // Build a 136-byte memory image: a non-empty list at base 0 (i32 count +
    // 4 pad + packed i64 elements) and an empty list (count 0) at base 128.
    let mut image = vec![0u8; 136];
    image[0..4].copy_from_slice(&(elems.len() as i32).to_le_bytes());
    for (i, &e) in elems.iter().enumerate() {
        let off = 8 + i * 8;
        image[off..off + 8].copy_from_slice(&e.to_le_bytes());
    }
    // base 128: count 0 (already zero from the zero-init) → an empty list.
    let data_str = wat_data_escape(&image);

    let mem_line = "  (memory (export \"mem\") 1)\n";
    assert!(
        total_wat.contains(mem_line),
        "emitted sum module declares the exported memory: {total_wat}"
    );
    let driver = format!(
        "  (data (i32.const 0) \"{data_str}\")\n  \
         (func (export \"e0\") (result f64)\n    \
         i32.const 0\n    call $total\n    f64.convert_i64_s)\n  \
         (func (export \"e1\") (result f64)\n    \
         i32.const 128\n    call $total\n    f64.convert_i64_s)\n"
    );
    let witness_wat = total_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);

    let engine = WasmDiffExecEngine::new();
    let out = engine
        .assemble_run_parse(&witness_wat, "list_int_sum")
        .expect("assemble+run REAL-emitted list-sum witness");
    assert_eq!(
        out.len(),
        2,
        "two exports (e0 non-empty, e1 empty): {out:?}"
    );
    assert!(
        (out[0] - expected_nonempty as f64).abs() <= 1.0e-9,
        "sum({elems:?}) executed {}, expected (CPython) {expected_nonempty}",
        out[0]
    );
    assert!(
        (out[1] - expected_empty as f64).abs() <= 1.0e-9,
        "sum([]) executed {}, expected (CPython) {expected_empty}",
        out[1]
    );

    eprintln!("=== PMAT-1248 executed witness: REAL xpile sum(xs) emit → run ===");
    eprintln!("--- witness WAT (REAL-emitted $total + data + e0/e1 drivers) ---\n{witness_wat}");
    eprintln!(
        "sum({elems:?}) = {} (CPython {expected_nonempty}); sum([]) = {} (CPython {expected_empty})",
        out[0], out[1]
    );
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
        msg.contains("not a `list[scalar]` param/local"),
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
        msg.contains(
            "not a `list[scalar]` param/local, a `str` param/local, or a `dict`/`set` local"
        ),
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
fn accepts_scalar_struct_refuses_nonscalar_field() {
    // PMAT-996 (slice 4): a scalar-field struct DEFINITION now lowers (pure
    // layout, no WAT symbol of its own) — it is no longer refused.
    let ok = WasmBackend::new().lower(
        &module_with(vec![Item::Struct {
            name: "P".into(),
            fields: vec![("x".into(), Type::I64)],
            methods: Vec::new(),
            frozen: false,
            order: false,
        }]),
        &wasm_config(),
    );
    assert!(ok.is_ok(), "a scalar-field struct is now supported: {ok:?}");
    // But a struct with a non-scalar field (no flat 8-byte-slot layout) is still
    // refused honestly, naming the offending field.
    let err = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Struct {
                name: "Q".into(),
                fields: vec![("s".into(), Type::Str)],
                methods: Vec::new(),
                frozen: false,
                order: false,
            }]),
            &wasm_config(),
        )
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("struct") && msg.contains("Str"),
        "honest refusal: {msg}"
    );
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
        msg.contains("not a `list[scalar]` param/local"),
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

// ─── PMAT-990: regression guard — the §29 WASM witness's GENERAL emitter
// drives the REAL `emit_module`, NOT the hardcoded `saxpy_module()` template ──
//
// PMAT-976 CLAIMED to rewire `WasmSaxpyGeneralEmitter` (the GENERAL side of
// the executed §29 DiffExec quorum, `WasmBackend::new_wasm_diffexec_witness`)
// to drive xpile's real meta-HIR → WAT lowering. The pre-existing tests in
// `wasm_diffexec.rs` only exercised `general_module_wat()` *directly*; nothing
// pinned the emitter the quorum ACTUALLY invokes — `WasmSaxpyGeneralEmitter::
// try_emit`. A future revert of `try_emit` back to `saxpy_module(...)` (the
// hand-written template the SPECIALIST side still legitimately uses) would
// silently turn the "real emit" witness hollow again with no failing test.
// These tests close that gap: they assert the WAT the GENERAL EMITTER ITSELF
// produces carries `emit_module`'s fingerprint and is NOT the bare template,
// then (graceful-skip on no-WABT) assemble+run that exact emitter output to
// prove the EXECUTED bytes came from `emit_module`.

/// The fingerprints ONLY `emit_module` emits (and `saxpy_module` never does):
/// the module banner comment, the `;; contract:` per-module citation (the
/// template writes `;; xpile-contract:` instead), and a named, zero-arg
/// `(export "eN" (func $eN))` per fixture element (the template emits an
/// ANONYMOUS `(func (export "eN") …)` with no `$eN`). Asserting all of these
/// means a revert to `saxpy_module(...)` cannot pass.
fn assert_carries_emit_module_fingerprint(wat: &str) {
    assert!(
        wat.contains("xpile-wasm-codegen — native WAT (scalar/control subset)"),
        "executed §29 WASM witness WAT must carry the REAL emit_module module \
         banner (the hardcoded saxpy_module template never emits it): {wat}"
    );
    assert!(
        wat.contains(";; contract: C-COMPILE-RUST-TO-WASM"),
        "executed §29 WASM witness WAT must carry emit_module's per-module \
         `;; contract:` citation (saxpy_module writes `;; xpile-contract:`): {wat}"
    );
    for i in 0..FIXTURE_INPUT.len() {
        assert!(
            wat.contains(&format!("(func $e{i} ")),
            "emit_module emits a NAMED zero-arg $e{i} func (saxpy_module's are \
             anonymous): {wat}"
        );
        assert!(
            wat.contains(&format!("(export \"e{i}\" (func $e{i}))")),
            "emit_module emits a named-func export for e{i} (saxpy_module \
             exports an anonymous func): {wat}"
        );
    }
    // And it must NOT be the hardcoded template: that template tags its module
    // with a `;; wasm-saxpy-…` comment, which emit_module never produces.
    assert!(
        !wat.contains(";; wasm-saxpy-"),
        "executed §29 WASM witness WAT is the HARDCODED saxpy_module template, \
         NOT emit_module output — PMAT-976 rewire reverted: {wat}"
    );
}

#[test]
fn general_witness_emitter_drives_real_emit_module_not_template() {
    // The actual §29 quorum emitter — call its `try_emit` directly (the path
    // `WasmBackend::new_wasm_diffexec_witness` wires into the DiffExec quorum)
    // and pin that its WAT is `emit_module` output, not the hand-written
    // `saxpy_module(...)` template. This is the guard that would FAIL if a
    // future change reverted `WasmSaxpyGeneralEmitter::try_emit` to the
    // hardcoded path PMAT-976 removed.
    let emitter = WasmSaxpyGeneralEmitter;
    let emitted = emitter
        .try_emit(&module_with(vec![]), &wasm_config())
        .expect("general witness emitter is wired for Target::Wasm")
        .expect("general witness emit succeeds");
    assert_carries_emit_module_fingerprint(&emitted.primary);

    // Cross-check the discriminator is real: the SPECIALIST side (which still
    // legitimately uses the hardcoded `saxpy_module` template) must NOT carry
    // the emit_module fingerprint — otherwise the guard above would be vacuous.
    let specialist = WasmSaxpySpecialistEmitter
        .try_emit(&module_with(vec![]), &wasm_config())
        .expect("specialist emitter wired for Target::Wasm")
        .expect("specialist emit succeeds");
    assert!(
        !specialist
            .primary
            .contains("xpile-wasm-codegen — native WAT (scalar/control subset)"),
        "the hardcoded saxpy_module template must NOT carry emit_module's banner \
         (else the fingerprint discriminator is vacuous): {}",
        specialist.primary
    );
    assert!(
        specialist.primary.contains(";; wasm-saxpy-"),
        "specialist is the hardcoded template (tagged `;; wasm-saxpy-`): {}",
        specialist.primary
    );
}

#[test]
fn general_witness_executed_wat_came_from_emit_module() {
    // The load-bearing executed half: assemble + run the EXACT WAT the §29
    // GENERAL witness emitter produces (via its `try_emit`), and prove the
    // executed bytes (a) came from `emit_module` (fingerprint) and (b) compute
    // the correct `2*x + 1` over FIXTURE_INPUT. A revert to the hardcoded
    // template would change the asserted fingerprint and fail (a), so this
    // pins that the RUNTIME-EXECUTED WAT is xpile's real emission.
    if !wasm_runtime_available() {
        eprintln!(
            "SKIP general_witness_executed_wat_came_from_emit_module: \
             WABT (wat2wasm/wasm-interp) not installed"
        );
        return;
    }

    let emitter = WasmSaxpyGeneralEmitter;
    let general_wat = emitter
        .try_emit(&module_with(vec![]), &wasm_config())
        .expect("general witness emitter wired for Target::Wasm")
        .expect("general witness emit succeeds")
        .primary;

    // (a) The WAT we are about to ASSEMBLE+RUN carries the emit_module
    //     fingerprint — so the executed bytes provably came from emit_module.
    assert_carries_emit_module_fingerprint(&general_wat);

    // (b) Assemble + run THAT exact WAT in WABT and diff against the trusted
    //     CPython-equivalent `2*x + 1` reference vector.
    let engine = WasmDiffExecEngine::new();
    let executed = engine
        .assemble_run_parse(&general_wat, "pmat990_general")
        .expect("assemble+run the §29 general witness emitter's REAL WAT");
    let expected: Vec<f64> = FIXTURE_INPUT.iter().map(|&x| 2.0 * x + 1.0).collect();
    assert_eq!(
        executed.len(),
        expected.len(),
        "executed witness WAT exports one e_i per fixture element: {executed:?}"
    );
    for (i, (g, e)) in executed.iter().zip(expected.iter()).enumerate() {
        assert!(
            (g - e).abs() <= 1.0e-9,
            "e{i}: §29 general witness REAL-emit WAT executed {g}, expected (CPython) {e}"
        );
    }

    eprintln!(
        "=== PMAT-990 regression guard: §29 WASM general witness emitter → emit_module → run ==="
    );
    eprintln!("--- WAT produced by WasmSaxpyGeneralEmitter::try_emit (carries emit_module banner + named $eN exports) ---");
    eprintln!("{general_wat}");
    eprintln!("--- executed output (wasm-interp) ---\n{executed:?}");
    eprintln!("--- CPython-equivalent 2*x+1 expected ---\n{expected:?}");
}

// ─── PMAT-1153: `s.removeprefix(p)` / `s.removesuffix(p)` — the allocating
// "strip a fixed prefix/suffix" pair on the native-WASM string lane ───────────
//
// Both RETURN a new heap string (`s` with a leading / trailing `p` removed when
// present, else a fresh copy of `s`). Byte-level: the prefix/suffix test is a
// byte compare and the retained range starts/ends on a code-point boundary
// (Python `p` is whole code points), so the pure byte copy is char-exact for any
// valid UTF-8 — no byte→code-point conversion (unlike find/rfind). Each wraps the
// matching predicate helper (`removeprefix` FORCES `$__wasm_str_startswith`,
// `removesuffix` FORCES `$__wasm_str_endswith`) and the bump allocator.

/// `def f() -> str: return <base>.<op>(<fix>)` — a zero-arg str-returning
/// function whose body is a single removeprefix/removesuffix over two literals.
/// Used both by the emit assertions and (wrapped in an f64 driver) the executed
/// WABT witness.
fn str_remove_fn(base: &str, op: StrMethodOp, fix: &str) -> Function {
    Function {
        name: "f".into(),
        params: Vec::new(),
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::LitStr(base.into())),
                op,
                args: vec![Expr::LitStr(fix.into())],
            },
        },
    }
}

#[test]
fn removeprefix_removesuffix_emit_helper_call_and_forced_predicate() {
    // PMAT-1153: each remove op emits its allocating helper + a call to it, FORCES
    // the matching byte-predicate helper it wraps, and pulls in the bump allocator
    // (it materialises a fresh heap string — unlike the non-allocating predicates).
    for (op, helper, forced_pred) in [
        (
            StrMethodOp::RemovePrefix,
            "$__wasm_str_removeprefix",
            "$__wasm_str_startswith",
        ),
        (
            StrMethodOp::RemoveSuffix,
            "$__wasm_str_removesuffix",
            "$__wasm_str_endswith",
        ),
    ] {
        let wat = emit_module(&module_with(vec![Item::Function(str_remove_fn(
            "unhappy", op, "un",
        ))]))
        .unwrap_or_else(|e| panic!("s.{op:?}(p) lowers: {e:?}"));
        assert!(
            wat.contains(&format!(
                "(func {helper} (param $s i32) (param $p i32) (result i32)"
            )),
            "the {helper} helper is emitted for {op:?}:\n{wat}"
        );
        let body = wat
            .split("(func $f ")
            .nth(1)
            .expect("the $f function is emitted");
        assert!(
            body.contains(&format!("call {helper}")),
            "$f calls {helper} for {op:?}:\n{body}"
        );
        // The remove helper wraps its predicate — so the predicate helper MUST be
        // co-emitted even though the module never calls it directly (the
        // needs_removeprefix ⇒ needs_startswith fold, mirroring index ⇒ find).
        assert!(
            wat.contains(&format!("(func {forced_pred} ")),
            "{op:?} forces the {forced_pred} helper:\n{wat}"
        );
        // An allocating op → the bump allocator is present (the helper calls it).
        assert!(
            wat.contains("(func $__alloc"),
            "a materialising remove op pulls in the bump allocator:\n{wat}"
        );
        assert!(
            wat.contains("(memory"),
            "str bytes → memory declared:\n{wat}"
        );
    }
}

#[test]
fn removeprefix_only_module_carries_no_removesuffix_helper() {
    // PMAT-1153: no-dead-helper discipline — a removeprefix-only module emits the
    // removeprefix helper + its forced startswith predicate, but NEITHER the
    // removesuffix helper NOR a dead endswith predicate.
    let wat = emit_module(&module_with(vec![Item::Function(str_remove_fn(
        "unhappy",
        StrMethodOp::RemovePrefix,
        "un",
    ))]))
    .expect("removeprefix lowers");
    assert!(
        wat.contains("$__wasm_str_removeprefix") && wat.contains("$__wasm_str_startswith"),
        "removeprefix + its forced startswith present:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_removesuffix"),
        "no dead removesuffix helper in a removeprefix-only module:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_endswith"),
        "no dead endswith helper in a removeprefix-only module:\n{wat}"
    );
}

#[test]
fn removesuffix_only_module_carries_no_removeprefix_helper() {
    // PMAT-1153: the mirror gating check for removesuffix.
    let wat = emit_module(&module_with(vec![Item::Function(str_remove_fn(
        "happiness",
        StrMethodOp::RemoveSuffix,
        "ness",
    ))]))
    .expect("removesuffix lowers");
    assert!(
        wat.contains("$__wasm_str_removesuffix") && wat.contains("$__wasm_str_endswith"),
        "removesuffix + its forced endswith present:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_removeprefix"),
        "no dead removeprefix helper in a removesuffix-only module:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_str_startswith"),
        "no dead startswith helper in a removesuffix-only module:\n{wat}"
    );
}

/// PMAT-1153 EXECUTED WITNESS — assemble + run the REAL-emitted `removeprefix` /
/// `removesuffix` in WABT and assert the produced heap string's byte length AND
/// its first/last payload bytes match CPython.
///
/// `wasm-interp --run-all-exports` runs only zero-arg exports and parses `f64`
/// results, so we wrap the REAL-emitted str-returning `$f` (which leaves an i32
/// base-pointer to the fresh heap string) in three zero-arg f64 drivers:
///   * `e0` — `f64.convert_i32_s` of the i32 byte-count header at `ptr+0`,
///   * `e1` — the first payload byte (`ptr+8`),
///   * `e2` — the last payload byte (`ptr+8+len-1`).
///
/// The three f64s are diffed against the CPython-equivalent
/// (`"unhappy".removeprefix("un")` == `"happy"` → len 5, `'h'`=104, `'y'`=121;
/// `"happiness".removesuffix("ness")` == `"happi"` → len 5, `'h'`=104, `'i'`=105).
/// Gated on `wasm_runtime_available()` — a clean skip on a host without WABT.
#[test]
fn removeprefix_removesuffix_execute_in_wabt() {
    if !wasm_runtime_available() {
        eprintln!("SKIP removeprefix_removesuffix_execute_in_wabt: WABT not installed");
        return;
    }
    // (op, base, fix, expected result string) — each result is 5 ASCII bytes, so
    // byte length == char length and the header read IS the visible len.
    let cases = [
        (StrMethodOp::RemovePrefix, "unhappy", "un", "happy"),
        (StrMethodOp::RemoveSuffix, "happiness", "ness", "happi"),
    ];
    let engine = WasmDiffExecEngine::new();
    let mem_line = "  (memory (export \"mem\") 1)\n";
    for (op, base, fix, expected) in cases {
        let f_wat = WasmBackend::new()
            .lower(
                &module_with(vec![Item::Function(str_remove_fn(base, op, fix))]),
                &wasm_config(),
            )
            .unwrap_or_else(|e| panic!("{op:?} lowers: {e:?}"))
            .primary;
        assert!(
            f_wat.contains(mem_line),
            "emitted {op:?} module declares the exported memory:\n{f_wat}"
        );
        let exp_bytes = expected.as_bytes();
        let last_addr = 8 + (exp_bytes.len() as i32 - 1);
        // Three zero-arg f64 drivers reading the heap string $f produces. Each
        // `call $f` re-materialises the (identical) result — fine under the
        // bump allocator (no free); we only read, never alias across calls.
        let driver = format!(
            "  (func (export \"e0\") (result f64)\n    \
             call $f\n    i32.load\n    f64.convert_i32_s)\n  \
             (func (export \"e1\") (result f64)\n    \
             call $f\n    i32.const 8\n    i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n  \
             (func (export \"e2\") (result f64)\n    \
             call $f\n    i32.const {last_addr}\n    i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n"
        );
        let witness_wat = f_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);
        let out = engine
            .assemble_run_parse(&witness_wat, &format!("remove_{op:?}"))
            .unwrap_or_else(|e| panic!("assemble+run {op:?} witness: {e}"));
        let expected_vec = [
            exp_bytes.len() as f64,
            f64::from(exp_bytes[0]),
            f64::from(exp_bytes[exp_bytes.len() - 1]),
        ];
        assert_eq!(
            out.len(),
            expected_vec.len(),
            "{op:?} witness exports e0/e1/e2: {out:?}"
        );
        for (i, (g, e)) in out.iter().zip(expected_vec.iter()).enumerate() {
            assert!(
                (g - e).abs() <= 1.0e-9,
                "{op:?}(\"{base}\", \"{fix}\") e{i}: executed {g}, expected (CPython, result \"{expected}\") {e}"
            );
        }
        eprintln!(
            "=== PMAT-1153 executed witness: {op:?}(\"{base}\",\"{fix}\") → \"{expected}\" \
             [len={}, first={}, last={}] ===",
            expected_vec[0], expected_vec[1], expected_vec[2]
        );
    }
}

// ─── PMAT-1159: `s.replace(old, new)` — the allocating substring-replace on the
// native-WASM string lane ─────────────────────────────────────────────────────
//
// RETURNS a NEW heap string with EVERY non-overlapping `old` replaced by `new`,
// scanned left to right. Non-empty `old` is two byte passes (count, then
// copy-with-substitution): a byte-substring match IS a code-point-substring match
// for valid UTF-8 (`old[0]` is a LEAD byte, so a match starts on a char boundary
// and — `old` being whole code points — spans whole chars), so the byte machinery
// is char-exact. Empty `old` is the ONE code-point-aware regime: Python
// interleaves `new` between every code point and at both ends
// (`"ab".replace("", "-")` == `"-a-b-"`), walked via `$__wasm_str_char_width`.
// Trapping on empty `old` would be WRONG (Python never raises there — a trap would
// be a silent divergence, not a ValueError analogue), so it is implemented.

/// `def f() -> str: return <s>.replace(<old>, <new>)` — a zero-arg str-returning
/// function whose body is a single replace over three literals. Used by the emit
/// assertions and (wrapped in per-byte f64 drivers) the executed WABT witness.
fn str_replace_fn(s: &str, old: &str, new: &str) -> Function {
    Function {
        name: "f".into(),
        params: Vec::new(),
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::LitStr(s.into())),
                op: StrMethodOp::Replace,
                args: vec![Expr::LitStr(old.into()), Expr::LitStr(new.into())],
            },
        },
    }
}

#[test]
fn replace_emit_helper_call_and_allocator() {
    // PMAT-1159: the replace op emits its allocating helper + a call to it, and —
    // since it MATERIALISES a fresh heap string — pulls in the bump allocator, the
    // exported memory, and (for the empty-`old` interleave) the char helpers
    // (`$__wasm_str_charlen` sizes the output, `$__wasm_str_char_width` walks it).
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(str_replace_fn("banana", "a", "AA"))]),
            &wasm_config(),
        )
        .expect("replace lowers")
        .primary;
    for needle in [
        "(func $__wasm_str_replace (param $s i32) (param $old i32) (param $new i32) (param $count i64) (result i32)",
        "call $__wasm_str_replace",
        "(func $__alloc",
        "(memory (export \"mem\")",
        "(func $__wasm_str_charlen",
        "(func $__wasm_str_char_width",
    ] {
        assert!(
            wat.contains(needle),
            "the replace module must contain {needle:?}:\n{wat}"
        );
    }
}

#[test]
fn replace_only_module_carries_no_dead_helpers() {
    // PMAT-1159: no-dead-helper discipline — a replace-only module emits the
    // replace helper but NEITHER the removeprefix/removesuffix helpers NOR the
    // string-repeat helper (each string op is gated separately on its own use).
    let wat = WasmBackend::new()
        .lower(
            &module_with(vec![Item::Function(str_replace_fn("abcabc", "b", "X"))]),
            &wasm_config(),
        )
        .expect("replace lowers")
        .primary;
    assert!(
        wat.contains("$__wasm_str_replace"),
        "replace helper present:\n{wat}"
    );
    for absent in [
        "$__wasm_str_removeprefix",
        "$__wasm_str_removesuffix",
        "$__wasm_str_repeat",
    ] {
        assert!(
            !wat.contains(absent),
            "no dead {absent} helper in a replace-only module:\n{wat}"
        );
    }
}

#[test]
fn replace_n_lowers_and_malformed_arity_refused() {
    // PMAT-1161: the bounded `.replace(old, new, count)` form (op `ReplaceN`, the
    // node the frontend produces for a 3-arg call) is now WIRED — it lowers through
    // the SAME `$__wasm_str_replace` helper (the count rides its 4th i64 param).
    let ok = Function {
        name: "f".into(),
        params: Vec::new(),
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::LitStr("aaaa".into())),
                op: StrMethodOp::ReplaceN,
                args: vec![
                    Expr::LitStr("a".into()),
                    Expr::LitStr("b".into()),
                    Expr::LitInt(1),
                ],
            },
        },
    };
    let wat = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(ok)]), &wasm_config())
        .expect("a 3-arg ReplaceN lowers via the shared helper")
        .primary;
    assert!(
        wat.contains("call $__wasm_str_replace") && wat.contains("(func $__wasm_str_replace"),
        "ReplaceN reuses the replace helper:\n{wat}"
    );

    // Arity guard: a malformed `Replace` op carrying 3 args (which the frontend
    // never emits — it produces `ReplaceN`) still refuses honestly, never silently
    // dropping the extra arg into a wrong unbounded replace.
    let bad = Function {
        name: "bad".into(),
        params: Vec::new(),
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::LitStr("aaaa".into())),
                op: StrMethodOp::Replace,
                args: vec![
                    Expr::LitStr("a".into()),
                    Expr::LitStr("b".into()),
                    Expr::LitInt(1),
                ],
            },
        },
    };
    let err = WasmBackend::new()
        .lower(&module_with(vec![Item::Function(bad)]), &wasm_config())
        .expect_err("a malformed 3-arg Replace must be refused, not miscompiled");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("string position"),
        "the malformed-arity refusal must be an honest string-position refusal: {msg}"
    );
}

/// One `(s, old, new)` replace fixture. The expected result is Rust's
/// `str::replace`, which equals CPython's `str.replace` for EVERY valid-UTF-8
/// triple — including the empty-`old` interleave (`"ab".replace("", "-")` ==
/// `"-a-b-"`), asserted in `cpython_replace_pins_match_rust`.
struct ReplaceCase {
    s: &'static str,
    old: &'static str,
    new: &'static str,
}

/// The witness fixtures: basic single/multi match, deletion (empty `new`),
/// GROWTH (`new` longer than `old`), non-overlapping (`"aaaa".replace("aa","b")`
/// == `"bb"`, NOT `"b"`), absent / longer-than-`s` old (→ a fresh copy), the
/// empty-`old` interleave, and MULTI-BYTE fixtures where a byte-blind replace
/// could split a char or false-positive on a shared continuation byte
/// (`"héllo".replace("l", "LL")`, `"🎉a🎉".replace("🎉", "!")`,
/// `"café".replace("é", "e")`).
const REPLACE_CASES: &[ReplaceCase] = &[
    ReplaceCase {
        s: "banana",
        old: "a",
        new: "AA",
    },
    ReplaceCase {
        s: "hello world",
        old: "o",
        new: "0",
    },
    ReplaceCase {
        s: "aaaa",
        old: "aa",
        new: "b",
    },
    ReplaceCase {
        s: "aXbXc",
        old: "X",
        new: "",
    },
    ReplaceCase {
        s: "mississippi",
        old: "iss",
        new: "X",
    },
    ReplaceCase {
        s: "xyz",
        old: "q",
        new: "Q",
    },
    ReplaceCase {
        s: "ab",
        old: "abc",
        new: "x",
    },
    ReplaceCase {
        s: "",
        old: "a",
        new: "b",
    },
    ReplaceCase {
        s: "ab",
        old: "",
        new: "-",
    },
    ReplaceCase {
        s: "",
        old: "",
        new: "-",
    },
    ReplaceCase {
        s: "aaa",
        old: "a",
        new: "aa",
    },
    ReplaceCase {
        s: "héllo",
        old: "l",
        new: "LL",
    },
    ReplaceCase {
        s: "café",
        old: "é",
        new: "e",
    },
    ReplaceCase {
        s: "🎉a🎉",
        old: "🎉",
        new: "!",
    },
];

#[test]
fn cpython_replace_pins_match_rust() {
    // Rust `str::replace` operates on the byte sequence of valid UTF-8 and equals
    // Python's code-point `str.replace` for every triple — the ground truth the
    // executed witness diffs against. Assert at least one MULTI-BYTE fixture and
    // one EMPTY-`old` fixture are present, so the char-exactness + interleave claims
    // are genuinely exercised (a witness over ASCII-only / non-empty-old would not
    // test them).
    assert!(
        REPLACE_CASES
            .iter()
            .any(|c| !c.s.is_ascii() || !c.old.is_ascii()),
        "a multi-byte fixture must be present"
    );
    assert!(
        REPLACE_CASES.iter().any(|c| c.old.is_empty()),
        "an empty-`old` interleave fixture must be present"
    );
    // The interleave pin is the corner most likely to diverge — assert it directly.
    assert_eq!("ab".replace("", "-"), "-a-b-");
    assert_eq!("".replace("", "-"), "-");
}

#[test]
fn replace_executes_in_wabt_and_matches_cpython() {
    // PMAT-1159 EXECUTED WITNESS — assemble + run the REAL-emitted `replace` in WABT
    // and assert the produced heap string's byte LENGTH and EVERY payload byte match
    // Rust's `str::replace` (== CPython). Prove the emit path lowers first (holds
    // without WABT), then — with WABT — drive it for real.
    for c in REPLACE_CASES {
        WasmBackend::new()
            .lower(
                &module_with(vec![Item::Function(str_replace_fn(c.s, c.old, c.new))]),
                &wasm_config(),
            )
            .unwrap_or_else(|e| panic!("replace({:?},{:?},{:?}) lowers: {e:?}", c.s, c.old, c.new));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1159: skipping EXECUTED replace witness — WABT (wat2wasm / \
             wasm-interp) absent. All {} fixtures lowered through the production \
             emitter above; a box with WABT also runs each and byte-matches \
             CPython (== Rust str::replace). Free CI skips execution and stays green.",
            REPLACE_CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1159: running EXECUTED replace witness via WABT");
    let engine = WasmDiffExecEngine::new();
    let mem_line = "  (memory (export \"mem\") 1)\n";
    let mut checked = 0usize;
    for c in REPLACE_CASES {
        let expected = c.s.replace(c.old, c.new);
        let exp_bytes = expected.as_bytes();
        let f_wat = WasmBackend::new()
            .lower(
                &module_with(vec![Item::Function(str_replace_fn(c.s, c.old, c.new))]),
                &wasm_config(),
            )
            .unwrap_or_else(|e| panic!("replace lowers: {e:?}"))
            .primary;
        assert!(
            f_wat.contains(mem_line),
            "emitted replace module declares the exported memory:\n{f_wat}"
        );
        // Zero-arg f64 drivers: e0 = the i32 byte-count header; eK = payload byte
        // K-1. Reading EVERY byte (not just first/last) makes the witness catch a
        // corrupted middle. Each `call $f` re-materialises the (identical) result —
        // fine under the bump allocator (no free); we only read, never alias.
        let mut driver = String::from(
            "  (func (export \"e0\") (result f64)\n    call $f\n    i32.load\n    f64.convert_i32_s)\n",
        );
        for k in 0..exp_bytes.len() {
            let addr = 8 + k as i32;
            driver.push_str(&format!(
                "  (func (export \"e{}\") (result f64)\n    call $f\n    i32.const {addr}\n    \
                 i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n",
                k + 1
            ));
        }
        let witness_wat = f_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);
        let out = engine
            .assemble_run_parse(&witness_wat, &format!("replace_{checked}"))
            .unwrap_or_else(|e| panic!("assemble+run replace witness for {:?}: {e}", c.s));
        // wasm-interp emits the exports in definition order: e0 (len), then e1.. .
        let mut want: Vec<f64> = vec![exp_bytes.len() as f64];
        want.extend(exp_bytes.iter().map(|&b| f64::from(b)));
        assert_eq!(
            out.len(),
            want.len(),
            "replace({:?},{:?},{:?}) exports e0..e{}: got {out:?}",
            c.s,
            c.old,
            c.new,
            exp_bytes.len()
        );
        for (i, (g, e)) in out.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - e).abs() <= 1.0e-9,
                "replace({:?},{:?},{:?}) → {expected:?}: e{i} executed {g}, expected {e}",
                c.s,
                c.old,
                c.new
            );
        }
        checked += 1;
    }
    eprintln!(
        "PMAT-1159: EXECUTED replace witness PASSED — {checked} fixtures lowered \
         through emit_module and executed in WABT, each byte-matching CPython (== \
         Rust str::replace), including the empty-`old` interleave \
         (\"ab\".replace(\"\",\"-\")==\"-a-b-\"), deletion, growth (\"a\"→\"aa\"), \
         non-overlapping (\"aaaa\".replace(\"aa\",\"b\")==\"bb\"), and the MULTI-BYTE \
         fixtures (\"héllo\".replace(\"l\",\"LL\"), \"🎉a🎉\".replace(\"🎉\",\"!\")) — \
         byte substring replace == code-point replace, proven on silicon."
    );
}

// ─── PMAT-1161: `s.replace(old, new, count)` — the BOUNDED substring-replace ────
//
// The 3-arg form (op `ReplaceN`) replaces only the first `count` non-overlapping
// occurrences, left to right (count < 0 → unlimited, matching Python). It reuses
// the SAME `$__wasm_str_replace` helper — the count rides its 4th i64 param, and
// the 2-arg `.replace` passes -1 (so replace-all is `count == -1`). The cap bounds
// BOTH regimes: non-empty `old` (PASS 1 counts min(matches, cap), PASS 2 stops
// substituting at the cap and copies the rest verbatim) and empty `old` (only the
// first `count` of the `charlen+1` interleave gaps get `new`).

/// `def f() -> str: return <s>.replace(<old>, <new>, <count>)` — the ReplaceN
/// node the frontend produces for a 3-arg call, over three literals + an int cap.
fn str_replace_n_fn(s: &str, old: &str, new: &str, count: i64) -> Function {
    Function {
        name: "f".into(),
        params: Vec::new(),
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::StrMethod {
                recv: Box::new(Expr::LitStr(s.into())),
                op: StrMethodOp::ReplaceN,
                args: vec![
                    Expr::LitStr(old.into()),
                    Expr::LitStr(new.into()),
                    Expr::LitInt(count),
                ],
            },
        },
    }
}

/// The CPython `str.replace(old, new, count)` ground truth: for `count >= 0`,
/// Rust's `str::replacen` (first `count` non-overlapping matches); for `count <
/// 0`, unlimited (`str::replace`). Verified byte-identical to CPython over every
/// fixture below (and the empty-`old` interleave) in `cpython_replace_n_pins`.
fn py_replacen(s: &str, old: &str, new: &str, count: i64) -> String {
    if count < 0 {
        s.replace(old, new)
    } else {
        s.replacen(old, new, count as usize)
    }
}

/// One `(s, old, new, count)` bounded-replace fixture.
struct ReplaceNCase {
    s: &'static str,
    old: &'static str,
    new: &'static str,
    count: i64,
}

/// Fixtures spanning the cap regimes: count 0 (no-op copy), count 1 / 2 over a
/// multi-match string (only the first N replaced, the rest kept), count >= matches
/// and count < 0 (== unlimited), deletion / growth / non-overlapping UNDER a cap,
/// the empty-`old` interleave capped to k gaps (`"ab".replace("","-",2)`=="-a-b"),
/// and MULTI-BYTE fixtures where a cap must not split a char (`"héllo".replace("l",
/// "LL",1)`=="héLLlo", `"🎉a🎉".replace("🎉","!",1)`=="!a🎉").
const REPLACE_N_CASES: &[ReplaceNCase] = &[
    // ── non-empty old, count cap over a multi-match string ──────────────────
    ReplaceNCase {
        s: "banana",
        old: "a",
        new: "AA",
        count: 0,
    }, // no-op copy
    ReplaceNCase {
        s: "banana",
        old: "a",
        new: "AA",
        count: 1,
    }, // first only
    ReplaceNCase {
        s: "banana",
        old: "a",
        new: "AA",
        count: 2,
    }, // first two
    ReplaceNCase {
        s: "banana",
        old: "a",
        new: "AA",
        count: 3,
    }, // == all
    ReplaceNCase {
        s: "banana",
        old: "a",
        new: "AA",
        count: 9,
    }, // cap > matches
    ReplaceNCase {
        s: "banana",
        old: "a",
        new: "AA",
        count: -1,
    }, // unlimited
    // ── deletion / growth / non-overlapping under a cap ─────────────────────
    ReplaceNCase {
        s: "aXbXcX",
        old: "X",
        new: "",
        count: 2,
    }, // delete first 2
    ReplaceNCase {
        s: "aaa",
        old: "a",
        new: "aa",
        count: 2,
    }, // growth, first 2
    ReplaceNCase {
        s: "aaaa",
        old: "aa",
        new: "b",
        count: 1,
    }, // non-overlap, 1
    ReplaceNCase {
        s: "aaaa",
        old: "aa",
        new: "b",
        count: 2,
    }, // non-overlap, 2
    ReplaceNCase {
        s: "mississippi",
        old: "iss",
        new: "X",
        count: 1,
    },
    ReplaceNCase {
        s: "mississippi",
        old: "iss",
        new: "X",
        count: 2,
    },
    // ── absent / longer-than-s old (a fresh copy regardless of count) ────────
    ReplaceNCase {
        s: "xyz",
        old: "q",
        new: "Q",
        count: 3,
    },
    ReplaceNCase {
        s: "ab",
        old: "abc",
        new: "x",
        count: 2,
    },
    // ── empty old — interleave capped to the first `count` of charlen+1 gaps ─
    ReplaceNCase {
        s: "ab",
        old: "",
        new: "-",
        count: 0,
    }, // no gaps → "ab"
    ReplaceNCase {
        s: "ab",
        old: "",
        new: "-",
        count: 1,
    }, // "-ab"
    ReplaceNCase {
        s: "ab",
        old: "",
        new: "-",
        count: 2,
    }, // "-a-b"
    ReplaceNCase {
        s: "ab",
        old: "",
        new: "-",
        count: 3,
    }, // "-a-b-"
    ReplaceNCase {
        s: "ab",
        old: "",
        new: "-",
        count: 9,
    }, // capped at 3
    ReplaceNCase {
        s: "ab",
        old: "",
        new: "-",
        count: -1,
    }, // all gaps
    ReplaceNCase {
        s: "",
        old: "",
        new: "-",
        count: 1,
    }, // "-"
    ReplaceNCase {
        s: "",
        old: "",
        new: "-",
        count: 0,
    }, // ""
    // ── multi-byte: a cap must land on a char boundary, never split a char ───
    ReplaceNCase {
        s: "héllo",
        old: "l",
        new: "LL",
        count: 1,
    }, // "héLLlo"
    ReplaceNCase {
        s: "héllo",
        old: "l",
        new: "LL",
        count: -1,
    }, // "héLLLLo"
    ReplaceNCase {
        s: "café",
        old: "é",
        new: "e",
        count: 1,
    },
    ReplaceNCase {
        s: "🎉a🎉",
        old: "🎉",
        new: "!",
        count: 1,
    }, // "!a🎉"
    ReplaceNCase {
        s: "abécdé",
        old: "é",
        new: "E",
        count: 1,
    }, // first é only
];

#[test]
fn cpython_replace_n_pins() {
    // Pin the corner cases most likely to diverge directly against hand-verified
    // CPython outputs (each also cross-checked live with python3): the empty-`old`
    // interleave under a cap, a multi-match cap, a multi-byte cap, and the count<0
    // == unlimited equivalence. These pin that `py_replacen` (the witness oracle)
    // IS CPython's `str.replace(old, new, count)`.
    assert_eq!(py_replacen("ab", "", "-", 1), "-ab");
    assert_eq!(py_replacen("ab", "", "-", 2), "-a-b");
    assert_eq!(py_replacen("ab", "", "-", 0), "ab");
    assert_eq!(py_replacen("ab", "", "-", 9), "-a-b-");
    assert_eq!(py_replacen("banana", "a", "AA", 2), "bAAnAAna");
    assert_eq!(py_replacen("banana", "a", "AA", -1), "bAAnAAnAA");
    assert_eq!(py_replacen("aaaa", "aa", "b", 1), "baa");
    assert_eq!(py_replacen("héllo", "l", "LL", 1), "héLLlo");
    assert_eq!(py_replacen("🎉a🎉", "🎉", "!", 1), "!a🎉");
    // count < 0 is exactly the unbounded 2-arg replace.
    for (s, o, n) in [("banana", "a", "AA"), ("héllo", "l", "LL"), ("ab", "", "-")] {
        assert_eq!(py_replacen(s, o, n, -1), s.replace(o, n));
    }
    // At least one empty-`old` and one multi-byte capped fixture must be present.
    assert!(REPLACE_N_CASES
        .iter()
        .any(|c| c.old.is_empty() && c.count >= 0));
    assert!(REPLACE_N_CASES
        .iter()
        .any(|c| !c.s.is_ascii() && c.count >= 0));
}

#[test]
fn replace_n_executes_in_wabt_and_matches_cpython() {
    // PMAT-1161 EXECUTED WITNESS — assemble + run the REAL-emitted bounded
    // `replace(old, new, count)` in WABT and assert the produced heap string's byte
    // LENGTH and EVERY payload byte match CPython (== `py_replacen`). Prove the emit
    // path lowers first (holds without WABT), then — with WABT — drive it for real.
    for c in REPLACE_N_CASES {
        WasmBackend::new()
            .lower(
                &module_with(vec![Item::Function(str_replace_n_fn(
                    c.s, c.old, c.new, c.count,
                ))]),
                &wasm_config(),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "replace({:?},{:?},{:?},{}) lowers: {e:?}",
                    c.s, c.old, c.new, c.count
                )
            });
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1161: skipping EXECUTED bounded-replace witness — WABT absent. All \
             {} fixtures lowered through the production emitter above; a box with \
             WABT also runs each and byte-matches CPython (== Rust str::replacen). \
             Free CI skips execution and stays green.",
            REPLACE_N_CASES.len()
        );
        return;
    }

    eprintln!("PMAT-1161: running EXECUTED bounded-replace witness via WABT");
    let engine = WasmDiffExecEngine::new();
    let mem_line = "  (memory (export \"mem\") 1)\n";
    let mut checked = 0usize;
    for c in REPLACE_N_CASES {
        let expected = py_replacen(c.s, c.old, c.new, c.count);
        let exp_bytes = expected.as_bytes();
        let f_wat = WasmBackend::new()
            .lower(
                &module_with(vec![Item::Function(str_replace_n_fn(
                    c.s, c.old, c.new, c.count,
                ))]),
                &wasm_config(),
            )
            .unwrap_or_else(|e| panic!("replace_n lowers: {e:?}"))
            .primary;
        assert!(
            f_wat.contains(mem_line),
            "emitted replace_n module declares the exported memory:\n{f_wat}"
        );
        let mut driver = String::from(
            "  (func (export \"e0\") (result f64)\n    call $f\n    i32.load\n    f64.convert_i32_s)\n",
        );
        for k in 0..exp_bytes.len() {
            let addr = 8 + k as i32;
            driver.push_str(&format!(
                "  (func (export \"e{}\") (result f64)\n    call $f\n    i32.const {addr}\n    \
                 i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n",
                k + 1
            ));
        }
        let witness_wat = f_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);
        let out = engine
            .assemble_run_parse(&witness_wat, &format!("replace_n_{checked}"))
            .unwrap_or_else(|e| panic!("assemble+run replace_n witness for {:?}: {e}", c.s));
        let mut want: Vec<f64> = vec![exp_bytes.len() as f64];
        want.extend(exp_bytes.iter().map(|&b| f64::from(b)));
        assert_eq!(
            out.len(),
            want.len(),
            "replace({:?},{:?},{:?},{}) exports e0..e{}: got {out:?}",
            c.s,
            c.old,
            c.new,
            c.count,
            exp_bytes.len()
        );
        for (i, (g, e)) in out.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - e).abs() <= 1.0e-9,
                "replace({:?},{:?},{:?},{}) → {expected:?}: e{i} executed {g}, expected {e}",
                c.s,
                c.old,
                c.new,
                c.count
            );
        }
        checked += 1;
    }
    eprintln!(
        "PMAT-1161: EXECUTED bounded-replace witness PASSED — {checked} fixtures \
         lowered through emit_module and executed in WABT, each byte-matching CPython \
         (== Rust str::replacen): count 0 / 1 / 2 / >=matches / <0, deletion / growth \
         / non-overlapping under a cap, the empty-`old` interleave capped to k gaps \
         (\"ab\".replace(\"\",\"-\",2)==\"-a-b\"), and MULTI-BYTE caps that land on a \
         char boundary (\"héllo\".replace(\"l\",\"LL\",1)==\"héLLlo\") — bounded byte \
         replace == bounded code-point replace, proven on silicon."
    );
}

// ─── PMAT-1162: gate-exhaustiveness matrix — an ALLOCATING str op in EVERY
// emittable compound position, adversarially locking the recurring gate-hole
// class shut ─────────────────────────────────────────────────────────────────
//
// The per-op witnesses above each drive the newest allocating string ops
// (`Replace` / `ReplaceN` / `RemovePrefix` / `RemoveSuffix`) in the SIMPLEST
// position — a bare `return <op>`. But the recurring gate-hole (PMAT-1128 /
// 1148 / 1149 / 1150 / 1151) lives in the COMPOUND positions: an op nested in a
// concat / ternary / repeat / nested-recv / method-arg / `in`-haystack, or bound
// to a `let`. If a `stmt_uses_str_method` / `expr_has_heap_op` walker fails to
// recurse into one of those, the backend still EMITS the `call
// $__wasm_str_replace` / `call $__alloc` — but never DECLARES the helper /
// allocator, a hole the bare-return witnesses cannot see. That emits a `.wat`
// wat2wasm REJECTS ("undefined function") — the exact class three consecutive
// slices kept re-finding one position at a time. This matrix probes ALL of them
// at once: every (op × position) pair must (a) lower, (b) declare `$__alloc` +
// the op's helper, and (c) — with WABT — assemble under wat2wasm. Adding a new
// allocating str op (or a new compound Expr node) without threading it through
// the gate walkers now fails HERE, not in the field.

/// A fresh allocating-str-op `Expr` for `op` over string literals — the node
/// whose helper + `$__alloc` every gate walker must detect in any position.
fn alloc_str_op(op: StrMethodOp) -> Expr {
    let recv = Box::new(Expr::LitStr("banana".into()));
    match op {
        StrMethodOp::Replace => Expr::StrMethod {
            recv,
            op,
            args: vec![Expr::LitStr("a".into()), Expr::LitStr("o".into())],
        },
        StrMethodOp::ReplaceN => Expr::StrMethod {
            recv,
            op,
            args: vec![
                Expr::LitStr("a".into()),
                Expr::LitStr("o".into()),
                Expr::LitInt(1),
            ],
        },
        StrMethodOp::RemovePrefix => Expr::StrMethod {
            recv,
            op,
            args: vec![Expr::LitStr("ba".into())],
        },
        StrMethodOp::RemoveSuffix => Expr::StrMethod {
            recv,
            op,
            args: vec![Expr::LitStr("na".into())],
        },
        other => unreachable!("gate matrix covers the 4 allocating str ops, not {other:?}"),
    }
}

/// The declared helper name for an allocating str op (the 2- and 3-arg replace
/// share `$__wasm_str_replace`).
fn alloc_str_op_helper(op: StrMethodOp) -> &'static str {
    match op {
        StrMethodOp::Replace | StrMethodOp::ReplaceN => "$__wasm_str_replace",
        StrMethodOp::RemovePrefix => "$__wasm_str_removeprefix",
        StrMethodOp::RemoveSuffix => "$__wasm_str_removesuffix",
        other => unreachable!("not an allocating str op: {other:?}"),
    }
}

/// A `Fn(inner) -> (return_type, stmts, trailing_return)` that drops the inner
/// allocating op into one compound emittable position.
type GatePos = fn(Expr) -> (Type, Vec<Stmt>, Expr);

/// Every compound position a str-typed (or, for `in`, bool-typed) allocating op
/// can legally sit in and still lower — the surface the gate walkers must cover.
fn gate_positions() -> Vec<(&'static str, GatePos)> {
    vec![
        // Baseline: bare `return <op>` (what the per-op witnesses already drive).
        ("bare_return", |i| (Type::Str, Vec::new(), i)),
        // `let x = <op>; return x` — the Stmt::Let value arm.
        ("let_binding", |i| {
            (
                Type::Str,
                vec![Stmt::Let {
                    name: "x".into(),
                    ty: Type::Str,
                    value: i,
                    mutable: false,
                }],
                Expr::Ident("x".into()),
            )
        }),
        // `return "z" + <op>` / `return <op> + "z"` — both Concat operands.
        ("concat_rhs", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::Concat {
                    lhs: Box::new(Expr::LitStr("z".into())),
                    rhs: Box::new(i),
                },
            )
        }),
        ("concat_lhs", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::Concat {
                    lhs: Box::new(i),
                    rhs: Box::new(Expr::LitStr("z".into())),
                },
            )
        }),
        // `return <op> if c else "z"` / `return "z" if c else <op>` — both IfExpr arms.
        ("ternary_then", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::IfExpr {
                    cond: Box::new(Expr::LitBool(true)),
                    then_expr: Box::new(i),
                    else_expr: Box::new(Expr::LitStr("z".into())),
                },
            )
        }),
        ("ternary_else", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::IfExpr {
                    cond: Box::new(Expr::LitBool(false)),
                    then_expr: Box::new(Expr::LitStr("z".into())),
                    else_expr: Box::new(i),
                },
            )
        }),
        // `return <op> * 2` — the Repeat seq (string repeat, of_str: true).
        ("repeat_seq", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::Repeat {
                    seq: Box::new(i),
                    n: Box::new(Expr::LitInt(2)),
                    of_str: true,
                },
            )
        }),
        // `return <op>.replace("z","w")` — the op as a NESTED StrMethod recv.
        ("nested_recv", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::StrMethod {
                    recv: Box::new(i),
                    op: StrMethodOp::Replace,
                    args: vec![Expr::LitStr("z".into()), Expr::LitStr("w".into())],
                },
            )
        }),
        // `return "banana".replace(<op>, "z")` — the op as a heap-constructed arg.
        ("method_arg", |i| {
            (
                Type::Str,
                Vec::new(),
                Expr::StrMethod {
                    recv: Box::new(Expr::LitStr("banana".into())),
                    op: StrMethodOp::Replace,
                    args: vec![i, Expr::LitStr("z".into())],
                },
            )
        }),
        // `return "o" in <op>` — the op as the StrContains haystack (bool result).
        ("strcontains_haystack", |i| {
            (
                Type::Bool,
                Vec::new(),
                Expr::StrContains {
                    haystack: Box::new(i),
                    needle: Box::new(Expr::LitStr("o".into())),
                },
            )
        }),
    ]
}

#[test]
fn allocating_str_op_in_every_position_declares_alloc_and_helper() {
    // PMAT-1162: the DECLARATION half of the matrix — runs with or without WABT.
    // Every (op × position) must lower AND declare both `$__alloc` (heap gate,
    // `expr_has_heap_op`) and the op's helper (str-method gate,
    // `module_uses_str_method`). A walker that fails to recurse into a position
    // emits the call but omits the declaration → this catches it.
    let ops = [
        StrMethodOp::Replace,
        StrMethodOp::ReplaceN,
        StrMethodOp::RemovePrefix,
        StrMethodOp::RemoveSuffix,
    ];
    let positions = gate_positions();
    let mut checked = 0usize;
    for op in ops {
        let helper = alloc_str_op_helper(op);
        for (pname, build) in &positions {
            let (ret, stmts, trailing) = build(alloc_str_op(op));
            let f = Function {
                name: "f".into(),
                params: Vec::new(),
                return_type: ret,
                body: Block {
                    stmts,
                    trailing_return: trailing,
                },
            };
            let wat = emit_module(&module_with(vec![Item::Function(f)])).unwrap_or_else(|e| {
                panic!("gate matrix: {op:?} in position `{pname}` must lower, got: {e:?}")
            });
            assert!(
                wat.contains("(func $__alloc"),
                "gate matrix: {op:?} @ `{pname}` emits an allocating op → `$__alloc` must be \
                 DECLARED (else a `call $__alloc` against an undeclared allocator = a wat2wasm \
                 hole):\n{wat}"
            );
            assert!(
                wat.contains(&format!("(func {helper} ")),
                "gate matrix: {op:?} @ `{pname}` → its helper {helper} must be DECLARED (the \
                 module_uses_str_method walker must recurse into this position):\n{wat}"
            );
            // The body must actually CALL what we just proved is declared — otherwise
            // the assertions above pass vacuously on a dead declaration.
            let body = wat
                .split("(func $f ")
                .nth(1)
                .expect("the $f function is emitted");
            assert!(
                body.contains(&format!("call {helper}")),
                "gate matrix: {op:?} @ `{pname}` → `$f` must CALL {helper}:\n{body}"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        ops.len() * positions.len(),
        "every (op × position) pair is checked"
    );
    eprintln!(
        "PMAT-1162: gate-exhaustiveness DECLARATION matrix PASSED — {checked} (op × position) \
         pairs ({} allocating ops × {} compound positions), each declaring `$__alloc` + its \
         helper and calling it.",
        ops.len(),
        positions.len()
    );
}

#[test]
fn allocating_str_op_in_every_position_assembles_under_wat2wasm() {
    // PMAT-1162: the ASSEMBLE half — the DECISIVE gate-hole catch. Each
    // real-emitted module is fed to wat2wasm; a `call` against an undeclared
    // helper/`$__alloc` (the recurring hole) is a HARD assembly error here. This
    // is exactly the failure the bare-return per-op witnesses cannot surface,
    // and the class PMAT-1149/1150/1151 kept re-finding one position at a time.
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1162: skipping wat2wasm gate-matrix assemble — WABT absent. The \
             DECLARATION matrix (allocating_str_op_in_every_position_declares_alloc_and_helper) \
             ran WITHOUT WABT and already catches a missing `$__alloc`/helper declaration; a \
             box with WABT also assembles every (op × position) module. Free CI stays green."
        );
        return;
    }
    let ops = [
        StrMethodOp::Replace,
        StrMethodOp::ReplaceN,
        StrMethodOp::RemovePrefix,
        StrMethodOp::RemoveSuffix,
    ];
    let positions = gate_positions();
    let engine = WasmDiffExecEngine::new();
    let mut assembled = 0usize;
    for op in ops {
        for (pname, build) in &positions {
            let (ret, stmts, trailing) = build(alloc_str_op(op));
            let f = Function {
                name: "f".into(),
                params: Vec::new(),
                return_type: ret,
                body: Block {
                    stmts,
                    trailing_return: trailing,
                },
            };
            let wat = emit_module(&module_with(vec![Item::Function(f)]))
                .unwrap_or_else(|e| panic!("gate matrix: {op:?} @ `{pname}` lowers: {e:?}"));
            engine
                .assemble(&wat, &format!("gate_{op:?}_{pname}"))
                .unwrap_or_else(|e| {
                    panic!(
                        "PMAT-1162 GATE HOLE: {op:?} in position `{pname}` emitted a `.wat` \
                         wat2wasm REJECTED — an emitted call against an undeclared \
                         helper/`$__alloc` (the recurring gate-hole class). wat2wasm said:\n{e}\
                         \n\n--- emitted module ---\n{wat}"
                    )
                });
            assembled += 1;
        }
    }
    eprintln!(
        "PMAT-1162: gate-exhaustiveness ASSEMBLE matrix PASSED — {assembled} (op × position) \
         modules real-emitted and accepted by wat2wasm; no undeclared-helper hole in any \
         compound position for Replace / ReplaceN / RemovePrefix / RemoveSuffix."
    );
}

// ─── PMAT-1167: bare single-interpolation int f-string `f"{n}"` ────────────────
//
// A LONE int f-string field `f"{n}"` (no surrounding literal text, no format
// spec) reaches the WASM lane neither as a `Concat` (there is no literal to
// anchor one) NOR as a `StrFormat` (that is only `.format(...)` / `%`) — the
// frontend's `stringify_lone_fstring_field` wraps the int in
// `Expr::FormatSpec { value, rust_spec: "", of_float: false }` (rendered
// `format!("{:}", n)`). Before PMAT-1167 the lane refused EVERY `FormatSpec`, so
// `f"{n}"` / `f"{a+b}"` / `f"{len(s)}"` refused even though each is exactly
// `str(int)` (already emitted). The `normalize_expr_fstring_ints` FormatSpec arm
// now rewrites the empty-spec, non-float, int-valued case into
// `ToStr{of_float:false}`; a real spec (`f"{x:>5}"`) or a float field stays
// refused. The injected `ToStr` is seen by the return/let-scanning
// `expr_has_int_to_str` / `expr_has_heap_op` gates, so `$__wasm_int_to_str` +
// `$__alloc` + `(memory)` stay declared (no gate hole).

/// `def f() -> str: return <FormatSpec{value, rust_spec, of_float}>`.
fn fstr_formatspec_fn(value: Expr, rust_spec: &str, of_float: bool) -> Function {
    Function {
        name: "f".into(),
        params: Vec::new(),
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FormatSpec {
                value: Box::new(value),
                rust_spec: rust_spec.into(),
                of_float,
            },
        },
    }
}

#[test]
fn bare_fstring_lone_int_formatspec_lowers_and_gates_helpers() {
    // The lone-int empty-spec `FormatSpec` (`f"{n}"`, `f"{a+b}"`) now lowers (no
    // refusal), emits the int→str helper + the bump allocator (str(int)
    // materialises a decimal-ASCII heap string), and — with WABT — assembles
    // clean (no undeclared-helper gate hole).
    let cases: [Expr; 2] = [
        Expr::LitInt(42),
        Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::LitInt(7)),
            rhs: Box::new(Expr::LitInt(5)),
        },
    ];
    for value in cases {
        let wat = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
            value, "", false,
        ))]))
        .unwrap_or_else(|e| panic!("PMAT-1167: lone-int FormatSpec lowers: {e:?}"));
        assert!(
            wat.contains("$__wasm_int_to_str"),
            "the int→str helper is emitted for a lone-int f-string:\n{wat}"
        );
        assert!(
            wat.contains("(func $__alloc "),
            "the bump allocator is declared (str(int) materialises a heap string):\n{wat}"
        );
        if wasm_runtime_available() {
            WasmDiffExecEngine::new()
                .assemble(&wat, "fstr_lone_int_gate")
                .unwrap_or_else(|e| {
                    panic!(
                        "PMAT-1167 GATE HOLE: lone-int f-string module rejected by \
                         wat2wasm:\n{e}\n\n--- emitted module ---\n{wat}"
                    )
                });
        }
    }
}

#[test]
fn formatspec_with_real_spec_or_float_still_refuses() {
    // A REAL format spec (`f"{n:>5}"`) and a FLOAT lone field (`f"{x}"`, whose
    // Python `nan`/`3.0` vs Rust `NaN`/`3` `Display` disagree) MUST still refuse
    // — the empty-spec int fold must not widen to formatting the lane does not
    // model.
    let spec = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
        Expr::LitInt(42),
        ">5",
        false,
    ))]));
    assert!(
        spec.is_err(),
        "a real width/alignment spec (`f\"{{n:>5}}\"`) still refuses on the WASM lane"
    );
    let float = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
        Expr::LitFloat(3.0),
        "",
        true,
    ))]));
    assert!(
        float.is_err(),
        "a lone float f-string field (`f\"{{x}}\"`) still refuses (str(float) unsupported)"
    );
}

/// PMAT-1167 EXECUTED WITNESS — assemble + run a lone-int f-string in WABT and
/// assert the produced heap string's byte-count header AND first/last payload
/// bytes equal CPython's `str(<value>)`. Mirrors the PMAT-1153 removeprefix
/// witness recipe (three zero-arg f64 drivers reading the str `$f` returns).
#[test]
fn bare_fstring_lone_int_executes_in_wabt() {
    if !wasm_runtime_available() {
        eprintln!("SKIP bare_fstring_lone_int_executes_in_wabt: WABT not installed");
        return;
    }
    // (lone-int value expr, CPython `str(value)`).
    let cases: [(Expr, &str); 3] = [
        (Expr::LitInt(42), "42"),
        (
            Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::LitInt(7)),
                rhs: Box::new(Expr::LitInt(5)),
            },
            "12",
        ),
        (Expr::LitInt(1_000_000), "1000000"),
    ];
    let engine = WasmDiffExecEngine::new();
    let mem_line = "  (memory (export \"mem\") 1)\n";
    for (value, expected) in cases {
        let f_wat = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
            value, "", false,
        ))]))
        .unwrap_or_else(|e| panic!("PMAT-1167: lone-int FormatSpec lowers: {e:?}"));
        let exp_bytes = expected.as_bytes();
        let last_addr = 8 + (exp_bytes.len() as i32 - 1);
        let driver = format!(
            "  (func (export \"e0\") (result f64)\n    \
             call $f\n    i32.load\n    f64.convert_i32_s)\n  \
             (func (export \"e1\") (result f64)\n    \
             call $f\n    i32.const 8\n    i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n  \
             (func (export \"e2\") (result f64)\n    \
             call $f\n    i32.const {last_addr}\n    i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n"
        );
        let witness_wat = f_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);
        let out = engine
            .assemble_run_parse(&witness_wat, "fstr_lone_int_exec")
            .unwrap_or_else(|e| panic!("assemble+run lone-int witness: {e}"));
        let expected_vec = [
            exp_bytes.len() as f64,
            f64::from(exp_bytes[0]),
            f64::from(exp_bytes[exp_bytes.len() - 1]),
        ];
        assert_eq!(
            out.len(),
            expected_vec.len(),
            "lone-int witness exports e0/e1/e2: {out:?}"
        );
        for (i, (g, e)) in out.iter().zip(expected_vec.iter()).enumerate() {
            assert!(
                (g - e).abs() <= 1.0e-9,
                "lone-int f-string → \"{expected}\" e{i}: executed {g}, expected (CPython) {e}"
            );
        }
        eprintln!(
            "=== PMAT-1167 executed witness: bare int f-string → \"{expected}\" \
             [len={}, first={}, last={}] ===",
            expected_vec[0], expected_vec[1], expected_vec[2]
        );
    }
}

// ─── PMAT-1169: inline unary-neg / bitwise-not int f-string `f"{-n}"` ──────────
//
// PMAT-1167 folded a bare int f-string `f"{n}"` (a `FormatSpec{rust_spec:"",
// of_float:false}`), but its classifier `concat_operand_is_int` had no `UnOp`
// arm, so a UNARY-op field — `f"{-n}"` (`UnOp::Neg`) or `f"{~n}"`
// (`UnOp::BitNot`) — stayed refused even though `-x` / `~x` over an int is
// itself an int and `$__wasm_int_to_str` already renders the sign. The
// classifier now recurses: `Neg`/`BitNot` over an int-classified operand is
// int (so `f"{-n}"`, `f"{~n}"`, `f"{-(a+b)}"` fold to `str(int)`), while a
// float operand (`-3.0`) or a logical `not` (bool) stays unwrapped → the honest
// refusal at `emit_str_expr`. No new runtime, no gate hole (the injected
// `ToStr` is seen by the existing `expr_has_int_to_str` / `expr_has_heap_op`
// walkers, which already recurse through `UnOp`).

/// `def f(n: I64) -> str: return <FormatSpec{UnOp{op, Ident("n")}, "", false}>`
/// — the realistic `f"{-n}"` shape whose operand classifies as int via ctx.
fn fstr_unop_ident_fn(op: UnOp) -> Function {
    Function {
        name: "f".into(),
        params: vec![param("n", Type::I64)],
        return_type: Type::Str,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FormatSpec {
                value: Box::new(Expr::UnOp {
                    op,
                    operand: Box::new(Expr::Ident("n".into())),
                }),
                rust_spec: String::new(),
                of_float: false,
            },
        },
    }
}

#[test]
fn inline_neg_fstring_formatspec_lowers_and_gates_helpers() {
    // `f"{-n}"` (Neg over an I64 param), `f"{~n}"` (BitNot), and `f"{-(7+5)}"`
    // (Neg over an int-arith BinOp) all fold to `str(int)`: the int→str helper
    // + bump allocator are gated, and — with WABT — each assembles clean (no
    // undeclared-helper gate hole).
    let fns: [Function; 3] = [
        fstr_unop_ident_fn(UnOp::Neg),
        fstr_unop_ident_fn(UnOp::BitNot),
        fstr_formatspec_fn(
            Expr::UnOp {
                op: UnOp::Neg,
                operand: Box::new(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::LitInt(7)),
                    rhs: Box::new(Expr::LitInt(5)),
                }),
            },
            "",
            false,
        ),
    ];
    for f in fns {
        let wat = emit_module(&module_with(vec![Item::Function(f)]))
            .unwrap_or_else(|e| panic!("PMAT-1169: inline-neg FormatSpec lowers: {e:?}"));
        assert!(
            wat.contains("$__wasm_int_to_str"),
            "the int→str helper is emitted for an inline-neg f-string:\n{wat}"
        );
        assert!(
            wat.contains("(func $__alloc "),
            "the bump allocator is declared (str(int) materialises a heap string):\n{wat}"
        );
        if wasm_runtime_available() {
            WasmDiffExecEngine::new()
                .assemble(&wat, "fstr_inline_neg_gate")
                .unwrap_or_else(|e| {
                    panic!(
                        "PMAT-1169 GATE HOLE: inline-neg f-string module rejected by \
                         wat2wasm:\n{e}\n\n--- emitted module ---\n{wat}"
                    )
                });
        }
    }
}

#[test]
fn inline_neg_fstring_over_float_still_refuses() {
    // Neg over a FLOAT operand (`f"{-3.0}"`) is NOT int — the classifier
    // recurses into the `LitFloat` operand, declines, and the FormatSpec stays
    // unfolded → the honest refusal at `emit_str_expr` (str(float) unmodelled).
    // The empty-spec, non-float FormatSpec framing (of_float:false) proves the
    // refusal comes from the OPERAND classification, not the outer float guard.
    let neg_float = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
        Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::LitFloat(3.0)),
        },
        "",
        false,
    ))]));
    assert!(
        neg_float.is_err(),
        "a unary-neg over a float (`f\"{{-3.0}}\"`) still refuses on the WASM lane"
    );
    // A REAL spec over an inline-neg field (`f"{-n:>5}"`) also stays refused —
    // the fold is empty-spec-only.
    let neg_spec = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
        Expr::UnOp {
            op: UnOp::Neg,
            operand: Box::new(Expr::LitInt(7)),
        },
        ">5",
        false,
    ))]));
    assert!(
        neg_spec.is_err(),
        "a width/alignment spec over an inline-neg field still refuses"
    );
}

/// PMAT-1169 EXECUTED WITNESS — assemble + run an inline-neg / bitwise-not int
/// f-string in WABT and assert the produced heap string's byte-count header AND
/// first/last payload bytes equal CPython's `str(<value>)`. Mirrors the
/// PMAT-1167 lone-int witness recipe. Uses genuine `UnOp` nodes (built directly,
/// not constant-folded) so the sign-aware `$__wasm_int_to_str` path is executed.
#[test]
fn inline_neg_fstring_executes_in_wabt() {
    if !wasm_runtime_available() {
        eprintln!("SKIP inline_neg_fstring_executes_in_wabt: WABT not installed");
        return;
    }
    // (value expr, CPython `str(value)`):
    //   -42        → "-42"    (Neg over a literal)
    //   ~5 == -6   → "-6"     (BitNot: Python ~x == -(x+1))
    //   -(7+5)     → "-12"    (Neg over an int-arith BinOp)
    let cases: [(Expr, &str); 3] = [
        (
            Expr::UnOp {
                op: UnOp::Neg,
                operand: Box::new(Expr::LitInt(42)),
            },
            "-42",
        ),
        (
            Expr::UnOp {
                op: UnOp::BitNot,
                operand: Box::new(Expr::LitInt(5)),
            },
            "-6",
        ),
        (
            Expr::UnOp {
                op: UnOp::Neg,
                operand: Box::new(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::LitInt(7)),
                    rhs: Box::new(Expr::LitInt(5)),
                }),
            },
            "-12",
        ),
    ];
    let engine = WasmDiffExecEngine::new();
    let mem_line = "  (memory (export \"mem\") 1)\n";
    for (value, expected) in cases {
        let f_wat = emit_module(&module_with(vec![Item::Function(fstr_formatspec_fn(
            value, "", false,
        ))]))
        .unwrap_or_else(|e| panic!("PMAT-1169: inline-neg FormatSpec lowers: {e:?}"));
        let exp_bytes = expected.as_bytes();
        let last_addr = 8 + (exp_bytes.len() as i32 - 1);
        let driver = format!(
            "  (func (export \"e0\") (result f64)\n    \
             call $f\n    i32.load\n    f64.convert_i32_s)\n  \
             (func (export \"e1\") (result f64)\n    \
             call $f\n    i32.const 8\n    i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n  \
             (func (export \"e2\") (result f64)\n    \
             call $f\n    i32.const {last_addr}\n    i32.add\n    i32.load8_u\n    f64.convert_i32_u)\n"
        );
        let witness_wat = f_wat.replacen(mem_line, &format!("{mem_line}{driver}"), 1);
        let out = engine
            .assemble_run_parse(&witness_wat, "fstr_inline_neg_exec")
            .unwrap_or_else(|e| panic!("assemble+run inline-neg witness: {e}"));
        let expected_vec = [
            exp_bytes.len() as f64,
            f64::from(exp_bytes[0]),
            f64::from(exp_bytes[exp_bytes.len() - 1]),
        ];
        assert_eq!(
            out.len(),
            expected_vec.len(),
            "inline-neg witness exports e0/e1/e2: {out:?}"
        );
        for (i, (g, e)) in out.iter().zip(expected_vec.iter()).enumerate() {
            assert!(
                (g - e).abs() <= 1.0e-9,
                "inline-neg f-string → \"{expected}\" e{i}: executed {g}, expected (CPython) {e}"
            );
        }
        eprintln!(
            "=== PMAT-1169 executed witness: inline-neg f-string → \"{expected}\" \
             [len={}, first={}, last={}] ===",
            expected_vec[0], expected_vec[1], expected_vec[2]
        );
    }
}
