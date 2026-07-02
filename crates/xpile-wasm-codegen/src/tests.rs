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
