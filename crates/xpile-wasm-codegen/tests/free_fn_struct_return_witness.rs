//! PMAT-1026 — EXECUTED free-function STRUCT-return witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! Sweep #10 (PMAT-1025) found that the PMAT-1023 `AssocFnRegistry` fixed the
//! conservative-i64 call-site mistyping ONLY for `Class::__init__` associated
//! fns — a plain FREE function returning `Type::Struct` (the factory idiom
//! `def make() -> Counter`, and the returns-param identity `def pick(c:
//! Counter) -> Counter: return c`) still claimed i64 at its call sites and
//! refused "expected WASM i32 but expression lowered to i64" at every
//! struct-typed use. This slice threads the PMAT-1024 free-function registry
//! into the VALUE-position `Expr::Call` lowering, so every intra-module call
//! types exactly from its `FnSig` (a `Struct`/`str` return rides an i32 base
//! pointer; bool/float returns type exactly too — the old i64 default refused
//! those uses just the same).
//!
//! An unresolved callee is now a PRECISE refusal naming the function: every
//! module function is in the registry, so a miss means `call $<callee>` would
//! emit invalid WAT (the old path deferred that to a confusing wat2wasm
//! failure downstream).
//!
//! ## Witness shape
//!
//! Zero-arg probe exports, each pinned to the CPython value of the identical
//! program (`PINS`), executed under WABT (`wat2wasm` + `wasm-interp`) when
//! available — a clean skip still asserts the EMIT half.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders ------------------------------------------------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

fn field(obj: &str, f: &str) -> Expr {
    Expr::FieldAccess {
        obj: Box::new(ident(obj)),
        field: f.into(),
    }
}

fn add(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn call(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: callee.into(),
        args,
    }
}

fn self_param(struct_name: &str) -> Param {
    Param {
        name: "self".into(),
        ty: Type::Struct(struct_name.into()),
        mutable: true,
    }
}

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.into(),
        ty,
        mutable: false,
    }
}

fn counter_lit(count: Expr) -> Expr {
    Expr::StructLit {
        name: "Counter".into(),
        fields: vec![("count".into(), count)],
    }
}

fn let_struct(name: &str, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Struct("Counter".into()),
        mutable: true,
        value,
    }
}

fn incr_stmt(obj: &str) -> Stmt {
    Stmt::SideEffectCall {
        call: Expr::MethodCall {
            obj: Box::new(ident(obj)),
            method: "incr".into(),
            args: vec![],
        },
    }
}

fn func(name: &str, ret: Type, params: Vec<Param>, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params,
        return_type: ret,
        body: Block {
            stmts,
            trailing_return: tail,
        },
    })
}

fn module(name: &str, items: Vec<Item>) -> Module {
    Module {
        name: name.into(),
        source_lang: SourceLang::Rust,
        items,
        ffi_boundaries: Vec::new(),
    }
}

/// ```python
/// class Counter:
///     count: int
///     def incr(self) -> None:
///         self.count = self.count + 1
///     def get(self) -> int:
///         return self.count
/// ```
fn counter_def() -> Item {
    let incr = Function {
        name: "incr".into(),
        params: vec![self_param("Counter")],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![Stmt::FieldAssign {
                obj: "self".into(),
                field: "count".into(),
                value: add(field("self", "count"), Expr::LitInt(1)),
            }],
            trailing_return: Expr::Unit,
        },
    };
    let get = Function {
        name: "get".into(),
        params: vec![self_param("Counter")],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: field("self", "count"),
        },
    };
    Item::Struct {
        name: "Counter".into(),
        fields: vec![("count".into(), Type::I64)],
        methods: vec![incr, get],
        frozen: false,
        order: false,
    }
}

/// The zero-arg probe module: FREE functions returning/threading structs,
/// each export pinned to CPython.
fn probe_module() -> Module {
    module(
        "free_fn_struct_return_witness",
        vec![
            counter_def(),
            // def make(start: int) -> Counter: return Counter(start)
            func(
                "make",
                Type::Struct("Counter".into()),
                vec![param("start", Type::I64)],
                vec![],
                counter_lit(ident("start")),
            ),
            // def pick(c: Counter) -> Counter: return c
            func(
                "pick",
                Type::Struct("Counter".into()),
                vec![param("c", Type::Struct("Counter".into()))],
                vec![],
                ident("c"),
            ),
            // def boost(c: Counter, k: int) -> Counter:
            //     c.incr(); return Counter(c.get() + k)
            func(
                "boost",
                Type::Struct("Counter".into()),
                vec![
                    param("c", Type::Struct("Counter".into())),
                    param("k", Type::I64),
                ],
                vec![incr_stmt("c")],
                counter_lit(add(
                    Expr::MethodCall {
                        obj: Box::new(ident("c")),
                        method: "get".into(),
                        args: vec![],
                    },
                    ident("k"),
                )),
            ),
            // def flag() -> bool: return True
            func("flag", Type::Bool, vec![], vec![], Expr::LitBool(true)),
            // ── THE FACTORY ──
            // c = make(3); c.incr(); return c.count                    → 4
            // (the exact shape sweep #10 found refusing "expected WASM i32
            //  but expression lowered to i64")
            func(
                "factory",
                Type::I64,
                vec![],
                vec![
                    let_struct("c", call("make", vec![Expr::LitInt(3)])),
                    incr_stmt("c"),
                ],
                field("c", "count"),
            ),
            // ── THE IDENTITY / RETURNS-PARAM ──
            // a = Counter(10); b = pick(a); b.incr(); b.incr();
            // return a.count                                           → 12
            // (reference semantics survive a free-fn call boundary: pick
            //  returns the SAME i32 base-pointer, so mutations through `b`
            //  are visible through `a` — exactly CPython)
            func(
                "identity",
                Type::I64,
                vec![],
                vec![
                    let_struct("a", counter_lit(Expr::LitInt(10))),
                    let_struct("b", call("pick", vec![ident("a")])),
                    incr_stmt("b"),
                    incr_stmt("b"),
                ],
                field("a", "count"),
            ),
            // ── THE CTOR-ARG CHAIN ──
            // c = boost(Counter(5), 100); c.incr(); return c.count     → 107
            // (a struct LITERAL as a free-fn arg + the struct RESULT bound
            //  and mutated)
            func(
                "chain",
                Type::I64,
                vec![],
                vec![
                    let_struct(
                        "c",
                        call(
                            "boost",
                            vec![counter_lit(Expr::LitInt(5)), Expr::LitInt(100)],
                        ),
                    ),
                    incr_stmt("c"),
                ],
                field("c", "count"),
            ),
            // if flag(): return 1 else: return 0                       → 1
            // (a BOOL-returning free-fn call in a typed i32 position — the
            //  old conservative-i64 default refused this use just the same)
            func(
                "use_flag",
                Type::I64,
                vec![],
                vec![Stmt::If {
                    cond: call("flag", vec![]),
                    then_body: vec![Stmt::Return(Expr::LitInt(1))],
                    else_body: vec![],
                }],
                Expr::LitInt(0),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every zero-arg probe export.
/// Each verified: `python3 -c "…program…; print(result)"`.
const PINS: &[(&str, i64)] = &[
    ("factory", 4),
    ("identity", 12),
    ("chain", 107),
    ("use_flag", 1),
];

// ---- WABT harness -----------------------------------------------------------

fn parse_i64_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let val = line.rsplit_once(':').expect("scalar line").1.trim();
    val.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse i64 for {name} from {line:?}"))
}

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-freefn-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).expect("work dir");
    let wat_path = dir.join("prog.wat");
    let wasm_path = dir.join("prog.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");
    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );
    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        run.status.success(),
    )
}

// ---- CONSTRUCT assertions (hold with or without WABT) -----------------------

#[test]
fn free_fn_struct_returns_type_as_i32_pointers() {
    let wat = emit_module(&probe_module()).expect("free-fn struct-return program lowers");
    assert!(
        wat.contains("(func $make (param $start i64) (result i32)"),
        "a struct-returning free fn carries an i32 (pointer) result:\n{wat}"
    );
    assert!(
        wat.contains("(func $pick (param $c i32) (result i32)"),
        "the identity fn takes and returns the record pointer:\n{wat}"
    );
    assert!(
        wat.contains("call $make") && wat.contains("call $pick") && wat.contains("call $boost"),
        "value-position free-fn calls emit against the plain symbols:\n{wat}"
    );
}

#[test]
fn unknown_value_position_callee_is_refused_by_name() {
    // The old conservative-i64 path emitted `call $mystery` and deferred the
    // failure to wat2wasm; now it is a precise refusal naming the callee.
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![],
            call("mystery", vec![]),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("`mystery`") && err.contains("not a function of this WASM module"),
        "unknown value-position callees refuse honestly, naming the callee: {err}"
    );
}

#[test]
fn free_fn_arity_mismatch_is_refused() {
    let m = module(
        "bad",
        vec![
            counter_def(),
            func(
                "make",
                Type::Struct("Counter".into()),
                vec![param("start", Type::I64)],
                vec![],
                counter_lit(ident("start")),
            ),
            func(
                "f",
                Type::I64,
                vec![],
                vec![let_struct(
                    "c",
                    call("make", vec![Expr::LitInt(1), Expr::LitInt(2)]),
                )],
                field("c", "count"),
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("`make` takes 1 argument(s) but the call passes 2"),
        "free-fn arity mismatches refuse with both counts: {err}"
    );
}

#[test]
fn unit_free_fn_in_value_position_is_refused() {
    let m = module(
        "bad",
        vec![
            func("noop", Type::Unit, vec![], vec![], Expr::Unit),
            func("f", Type::I64, vec![], vec![], call("noop", vec![])),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("`noop`") && err.contains("value position"),
        "unit-returning free fn in a value position refuses honestly: {err}"
    );
}

// ---- EXECUTED witnesses (gated on WABT) ------------------------------------

#[test]
fn free_fn_struct_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("free-fn struct-return program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1026: skipping EXECUTED free-fn witness — WABT absent. The \
             program lowered through emit_module (asserted in \
             `free_fn_struct_returns_type_as_i32_pointers`); a box with WABT \
             runs every export and asserts each == CPython {PINS:?}."
        );
        return;
    }
    let (stdout, ok) = assemble_and_run("probe", &wat);
    assert!(ok, "wasm-interp failed:\n{stdout}\n---WAT---\n{wat}");
    for &(name, expected) in PINS {
        let got = parse_i64_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1026: EXECUTED free-fn struct-return witness PASSED — the \
         factory idiom, the returns-param identity (reference semantics \
         surviving a free-fn boundary), a ctor-arg chain, and an exactly-typed \
         bool return all executed in WABT value-matching CPython {PINS:?}."
    );
}
