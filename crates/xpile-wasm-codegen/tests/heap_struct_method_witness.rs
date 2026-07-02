//! PMAT-1023 — EXECUTED struct-METHOD + FIELD-MUTATION witness for the native
//! WASM EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! PMAT-996 shipped PLAIN-DATA structs (literal construction + field reads).
//! This slice ships the OOP surface the Rust lane gained in PMAT-1016/1022,
//! ported to the WASM heap runtime:
//!
//! - **`Stmt::FieldAssign`** — `obj.field = v` stores through the record's
//!   i32 base-pointer at the field's 8-byte-slot offset;
//! - **struct METHODS** — each `Item::Struct` method (INCLUDING self-mutating
//!   ones) emits as an ordinary WAT function `$<Struct>.<method>` whose `self`
//!   receiver is the instance pointer;
//! - **`Expr::MethodCall`** — `obj.method(args)` pushes the receiver pointer +
//!   typed args and calls it;
//! - **`Stmt::SideEffectCall`** — statement-position `c.incr()`, dropping a
//!   discarded result.
//!
//! ## The headline: Python reference semantics are NATIVE here
//!
//! Every binding of a record holds the SAME i32 base-pointer, so a field
//! write through one binding is visible through every alias — `b = a;
//! b.x = 99; a.x` is 99, exactly CPython. The Rust lane's whole
//! alias-disposition machinery (PMAT-1008/1020: clone/move/refuse) exists
//! because Rust VALUE semantics cannot express this sharing; linear memory
//! expresses it for free. The aliasing witness below executes the exact
//! shape the Rust lane must REFUSE and value-matches CPython.
//!
//! (Honest scope note: the shared Python FRONTEND still applies its
//! target-blind alias-class refusals, so a Python-source aliasing program
//! does not yet reach this backend — the witness drives the backend at the
//! meta-HIR level, like every other witness in this suite. Target-aware
//! alias disposition is a filed follow-up.)
//!
//! ## Witness shape
//!
//! Zero-arg probe functions construct instances locally, mutate via methods /
//! field assigns, and return a readable scalar — `wasm-interp
//! --run-all-exports` runs each and the executed scalar must VALUE-MATCH the
//! CPython value pinned in `PINS` (each verified against the commented Python
//! program). Gated on `wasm_runtime_available()` — a clean skip (still
//! asserting the EMIT path lowers + carries the method machinery) on a host
//! without WABT.

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

fn mul(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Mul,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn self_param(struct_name: &str) -> Param {
    Param {
        name: "self".into(),
        ty: Type::Struct(struct_name.into()),
        mutable: true,
    }
}

fn method_call(obj: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        obj: Box::new(ident(obj)),
        method: method.into(),
        args,
    }
}

fn call_stmt(obj: &str, method: &str, args: Vec<Expr>) -> Stmt {
    Stmt::SideEffectCall {
        call: method_call(obj, method, args),
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
///     def add(self, n: int) -> None:
///         self.count = self.count + n
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
    let add_m = Function {
        name: "add".into(),
        params: vec![
            self_param("Counter"),
            Param {
                name: "n".into(),
                ty: Type::I64,
                mutable: false,
            },
        ],
        return_type: Type::Unit,
        body: Block {
            stmts: vec![Stmt::FieldAssign {
                obj: "self".into(),
                field: "count".into(),
                value: add(field("self", "count"), ident("n")),
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
        methods: vec![incr, add_m, get],
        frozen: false,
        order: false,
    }
}

/// `@dataclass class Point: x: int; y: int` + `def dist2(self): return
/// self.x*self.x + self.y*self.y` (read-only method).
fn point_def() -> Item {
    let dist2 = Function {
        name: "dist2".into(),
        params: vec![self_param("Point")],
        return_type: Type::I64,
        body: Block {
            stmts: vec![],
            trailing_return: add(
                mul(field("self", "x"), field("self", "x")),
                mul(field("self", "y"), field("self", "y")),
            ),
        },
    };
    Item::Struct {
        name: "Point".into(),
        fields: vec![("x".into(), Type::I64), ("y".into(), Type::I64)],
        methods: vec![dist2],
        frozen: false,
        order: false,
    }
}

fn let_counter(name: &str, count: i64) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Struct("Counter".into()),
        mutable: true,
        value: Expr::StructLit {
            name: "Counter".into(),
            fields: vec![("count".into(), Expr::LitInt(count))],
        },
    }
}

fn let_point(name: &str, x: i64, y: i64) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Struct("Point".into()),
        mutable: true,
        value: Expr::StructLit {
            name: "Point".into(),
            fields: vec![("x".into(), Expr::LitInt(x)), ("y".into(), Expr::LitInt(y))],
        },
    }
}

/// The zero-arg probe module: one export per assertion, every value pinned to
/// the CPython result of the identical program.
fn probe_module() -> Module {
    module(
        "struct_method_witness",
        vec![
            counter_def(),
            point_def(),
            // c = Counter(0); c.incr(); c.incr(); return c.count       → 2
            // (statement-position unit method calls MUTATE the record)
            func(
                "incr_twice",
                Type::I64,
                vec![],
                vec![
                    let_counter("c", 0),
                    call_stmt("c", "incr", vec![]),
                    call_stmt("c", "incr", vec![]),
                ],
                field("c", "count"),
            ),
            // a = Counter(10); a.add(5); a.add(7); return a.get()      → 22
            // (method ARG + value-position method call for the read-back)
            func(
                "add_args",
                Type::I64,
                vec![],
                vec![
                    let_counter("a", 10),
                    call_stmt("a", "add", vec![Expr::LitInt(5)]),
                    call_stmt("a", "add", vec![Expr::LitInt(7)]),
                ],
                method_call("a", "get", vec![]),
            ),
            // p = Point(3, 4); return p.dist2()                        → 25
            // (read-only method in a value position)
            func(
                "dist2",
                Type::I64,
                vec![],
                vec![let_point("p", 3, 4)],
                method_call("p", "dist2", vec![]),
            ),
            // p = Point(3, 4); p.x = 99; return p.x + p.y              → 103
            // (bare FieldAssign outside a method)
            func(
                "field_write",
                Type::I64,
                vec![],
                vec![
                    let_point("p", 3, 4),
                    Stmt::FieldAssign {
                        obj: "p".into(),
                        field: "x".into(),
                        value: Expr::LitInt(99),
                    },
                ],
                add(field("p", "x"), field("p", "y")),
            ),
            // ── THE HEADLINE ──
            // a = Point(3, 4); b = a; b.x = 99; return a.x             → 99
            // CPython: b IS a (reference), so a.x reads 99. The Rust lane
            // must REFUSE this shape (alias + mutate + read-original,
            // PMAT-1008/1020); WASM linear memory executes it EXACTLY —
            // `b = a` copies the i32 base-pointer, sharing the record.
            func(
                "alias_mutate",
                Type::I64,
                vec![],
                vec![
                    let_point("a", 3, 4),
                    Stmt::Let {
                        name: "b".into(),
                        ty: Type::Struct("Point".into()),
                        mutable: true,
                        value: ident("a"),
                    },
                    Stmt::FieldAssign {
                        obj: "b".into(),
                        field: "x".into(),
                        value: Expr::LitInt(99),
                    },
                ],
                field("a", "x"),
            ),
            // c = Counter(0); c.incr(); d = c; d.incr(); return c.count → 2
            // (aliasing THROUGH a mutating method: d.incr() bumps c too)
            func(
                "alias_method",
                Type::I64,
                vec![],
                vec![
                    let_counter("c", 0),
                    call_stmt("c", "incr", vec![]),
                    Stmt::Let {
                        name: "d".into(),
                        ty: Type::Struct("Counter".into()),
                        mutable: true,
                        value: ident("c"),
                    },
                    call_stmt("d", "incr", vec![]),
                ],
                field("c", "count"),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every zero-arg probe export.
/// Each verified: `python3 -c "…program…; print(result)"`.
const PINS: &[(&str, i64)] = &[
    ("incr_twice", 2),
    ("add_args", 22),
    ("dist2", 25),
    ("field_write", 103),
    ("alias_mutate", 99),
    ("alias_method", 2),
];

// ---- WABT harness -----------------------------------------------------------

fn parse_i64_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let val = line.rsplit_once(':').expect("scalar line").1.trim();
    // wasm-interp prints i64 unsigned; reinterpret as signed.
    val.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse i64 for {name} from {line:?}"))
}

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-method-{}-{}", tag, std::process::id()));
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
fn methods_emit_as_wat_functions() {
    let wat = emit_module(&probe_module()).expect("struct-method program lowers");
    // Each method is an ordinary WAT function whose `self` is an i32 pointer.
    assert!(
        wat.contains("(func $Counter.incr (param $self i32)"),
        "a unit method emits with a $Struct.method symbol + i32 self:\n{wat}"
    );
    assert!(
        wat.contains("(func $Counter.add (param $self i32) (param $n i64)"),
        "method args follow the self pointer:\n{wat}"
    );
    assert!(
        wat.contains("(func $Point.dist2 (param $self i32) (result i64)"),
        "a value-returning method carries its result type:\n{wat}"
    );
    // Self-mutation is a store through the pointer; call sites call the symbol.
    assert!(
        wat.contains("call $Counter.incr") && wat.contains("call $Point.dist2"),
        "method calls target the mangled symbols:\n{wat}"
    );
    assert!(
        wat.contains("i64.store offset=0"),
        "self.count = … stores through the receiver pointer:\n{wat}"
    );
}

#[test]
fn unknown_method_is_refused() {
    let m = module(
        "bad",
        vec![
            point_def(),
            func(
                "f",
                Type::I64,
                vec![],
                vec![let_point("p", 1, 2)],
                method_call("p", "norm", vec![]),
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("no method `norm`"),
        "unknown method names the miss: {err}"
    );
}

#[test]
fn unit_method_in_value_position_is_refused() {
    let m = module(
        "bad",
        vec![
            counter_def(),
            func(
                "f",
                Type::I64,
                vec![],
                vec![let_counter("c", 0)],
                method_call("c", "incr", vec![]),
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("value position"),
        "unit-method-as-value refused honestly: {err}"
    );
}

#[test]
fn struct_equality_is_refused_not_pointer_compared() {
    // p == q over two records: a naive lowering would i32.eq the BASE
    // POINTERS (always false for distinct records — silently diverging from
    // Python's structural ==). Must refuse.
    let m = module(
        "bad",
        vec![
            point_def(),
            func(
                "f",
                Type::Bool,
                vec![],
                vec![let_point("p", 1, 2), let_point("q", 1, 2)],
                Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(ident("p")),
                    rhs: Box::new(ident("q")),
                },
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("struct operand"),
        "struct == must refuse, never pointer-compare: {err}"
    );
}

#[test]
fn non_method_side_effect_call_is_refused() {
    // A bare function-call statement (not a struct method) stays refused.
    let m = module(
        "bad",
        vec![
            counter_def(),
            func(
                "f",
                Type::I64,
                vec![],
                vec![
                    let_counter("c", 0),
                    Stmt::SideEffectCall {
                        call: Expr::Call {
                            callee: "free_fn".into(),
                            args: vec![],
                        },
                    },
                ],
                field("c", "count"),
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("not a struct method call"),
        "bare call statements refuse honestly: {err}"
    );
}

// ---- EXECUTED witnesses (gated on WABT) ------------------------------------

#[test]
fn method_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("struct-method program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1023: skipping EXECUTED method witness — WABT absent. The \
             program lowered through emit_module (asserted in \
             `methods_emit_as_wat_functions`); a box with WABT runs every \
             export and asserts each == CPython {PINS:?}."
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
        "PMAT-1023: EXECUTED struct-method witness PASSED — mutating methods, \
         method args, value-position calls, bare field writes, AND the \
         alias-mutate-read shape the Rust lane must refuse all executed in \
         WABT value-matching CPython {PINS:?}. Python reference semantics \
         are native to the WASM heap."
    );
}
