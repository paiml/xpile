//! PMAT-996 (slice 4) — EXECUTED plain-data-struct witness for the native WASM
//! EMIT lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The heap-runtime epic (PMAT-986) shipped scalars → control → list → strings
//! → dict/set. This slice adds PLAIN-DATA STRUCTS (Python `@dataclass` / class):
//! a struct instance is a bump-heap record — each field in a uniform 8-byte slot
//! at `base + field_index*8` (definition order; an i32/f32/bool field uses the
//! low 4 bytes). `StructLit` (`Name(f=v, …)`) `$__alloc`s the record + writes the
//! fields; `FieldAccess` (`obj.field`) loads a field. A struct LOCAL and a struct
//! PARAM both ride an `i32` base-pointer.
//!
//! ## Witness shape
//!
//! A struct-LOCAL program (`p = Point(3,4); return p.x + p.y`) is a ZERO-ARG
//! function returning a readable scalar — `wasm-interp --run-all-exports` runs it
//! directly (no driver), and the executed scalar must VALUE-MATCH CPython. A
//! struct-PARAM program (`def px(p): return p.x`) takes an `i32` base-pointer, so
//! (like the string witness) a driver preloads a Point record via `(data)`
//! segments and calls it. Every program lowers through the production
//! `emit_module`; the test assembles (`wat2wasm`) + runs (`wasm-interp`) it.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the struct layout) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, Param, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders ------------------------------------------------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `@dataclass class Point: x: int; y: int` — a two-int-field struct.
fn point_def() -> Item {
    Item::Struct {
        name: "Point".into(),
        fields: vec![("x".into(), Type::I64), ("y".into(), Type::I64)],
        methods: vec![],
        frozen: false,
        order: false,
    }
}

/// `@dataclass class Rec: n: int; f: float` — a mixed int/float struct.
fn rec_def() -> Item {
    Item::Struct {
        name: "Rec".into(),
        fields: vec![("n".into(), Type::I64), ("f".into(), Type::F64)],
        methods: vec![],
        frozen: false,
        order: false,
    }
}

fn field(obj: &str, f: &str) -> Expr {
    Expr::FieldAccess {
        obj: Box::new(ident(obj)),
        field: f.into(),
    }
}

/// `p = Point(x, y)` bound to a struct local `p`.
fn let_point(name: &str, x: i64, y: i64) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Struct("Point".into()),
        mutable: false,
        value: Expr::StructLit {
            name: "Point".into(),
            fields: vec![("x".into(), Expr::LitInt(x)), ("y".into(), Expr::LitInt(y))],
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

/// The zero-arg struct-LOCAL probe module: one export per assertion.
fn probe_module() -> Module {
    let add = |l: Expr, r: Expr| Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(l),
        rhs: Box::new(r),
    };
    module(
        "struct_witness",
        vec![
            point_def(),
            // p = Point(3, 4); return p.x            → 3
            func(
                "getx",
                Type::I64,
                vec![],
                vec![let_point("p", 3, 4)],
                field("p", "x"),
            ),
            // return p.y                             → 4
            func(
                "gety",
                Type::I64,
                vec![],
                vec![let_point("p", 3, 4)],
                field("p", "y"),
            ),
            // return p.x + p.y                       → 7
            func(
                "sum",
                Type::I64,
                vec![],
                vec![let_point("p", 3, 4)],
                add(field("p", "x"), field("p", "y")),
            ),
            // two independent instances: return a.x + b.y  → 10 + 200 = 210
            func(
                "two",
                Type::I64,
                vec![],
                vec![let_point("a", 10, 20), let_point("b", 100, 200)],
                add(field("a", "x"), field("b", "y")),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every zero-arg probe export.
const PINS: &[(&str, i64)] = &[("getx", 3), ("gety", 4), ("sum", 7), ("two", 210)];

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
        std::env::temp_dir().join(format!("xpile-wasm-struct-{}-{}", tag, std::process::id()));
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
fn struct_program_lowers_and_carries_layout() {
    let wat = emit_module(&probe_module()).expect("struct program lowers through emit_module");
    assert!(
        wat.contains("(func $__alloc") && wat.contains("(global $__heap_ptr (mut i32)"),
        "a struct instance needs the bump allocator:\n{wat}"
    );
    // Fields stored at 8-byte-slot offsets and read back at the same offsets.
    assert!(
        wat.contains("i64.store offset=0") && wat.contains("i64.store offset=8"),
        "two i64 fields must store at offsets 0 and 8:\n{wat}"
    );
    assert!(
        wat.contains("i64.load offset=0") && wat.contains("i64.load offset=8"),
        "field reads must load at the field offsets:\n{wat}"
    );
    // A struct DEFINITION emits no `(func …)` of its own.
    assert!(
        !wat.contains("$Point"),
        "a struct definition is pure layout — no WAT symbol:\n{wat}"
    );
}

#[test]
fn non_scalar_field_struct_is_refused() {
    // A struct with a `str` field has no flat 8-byte-slot layout → refused.
    let m = module(
        "bad",
        vec![
            Item::Struct {
                name: "Named".into(),
                fields: vec![("id".into(), Type::I64), ("name".into(), Type::Str)],
                methods: vec![],
                frozen: false,
                order: false,
            },
            func(
                "gid",
                Type::I64,
                vec![],
                vec![Stmt::Let {
                    name: "n".into(),
                    ty: Type::Struct("Named".into()),
                    mutable: false,
                    value: Expr::StructLit {
                        name: "Named".into(),
                        fields: vec![
                            ("id".into(), Expr::LitInt(7)),
                            ("name".into(), Expr::LitStr("hi".into())),
                        ],
                    },
                }],
                field("n", "id"),
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(err.contains("unsupported"), "honest refusal: {err}");
    assert!(
        err.contains("field `name`") && err.contains("Str"),
        "names the offending non-scalar field: {err}"
    );
}

#[test]
fn struct_return_lowers_as_heap_pointer() {
    // PMAT-996 REFUSED a struct return; PMAT-1023 upgrades it — the record
    // rides an i32 base-pointer (required by the desugared explicit
    // `__init__` ctor, and free `def make(): return Point(1, 2)` gets it
    // for free: the trailing StructLit leaves exactly that pointer).
    let m = module(
        "ret",
        vec![
            point_def(),
            func(
                "make",
                Type::Struct("Point".into()),
                vec![],
                vec![],
                Expr::StructLit {
                    name: "Point".into(),
                    fields: vec![("x".into(), Expr::LitInt(1)), ("y".into(), Expr::LitInt(2))],
                },
            ),
        ],
    );
    let wat = emit_module(&m).expect("struct return lowers (PMAT-1023)");
    assert!(
        wat.contains("(func $make (result i32)"),
        "a struct return is an i32 heap pointer:\n{wat}"
    );
}

// ---- EXECUTED witnesses (gated on WABT) ------------------------------------

#[test]
fn struct_local_program_executes_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("struct program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-996: skipping EXECUTED struct witness — WABT absent. The program \
             lowered through emit_module (asserted in `struct_program_lowers_and_\
             carries_layout`); a box with WABT runs every export and asserts each \
             == CPython {PINS:?}."
        );
        return;
    }
    let (stdout, ok) = assemble_and_run("local", &wat);
    assert!(ok, "wasm-interp failed:\n{stdout}\n---WAT---\n{wat}");
    for &(name, expected) in PINS {
        let got = parse_i64_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-996: EXECUTED struct witness PASSED — Point(x,y) construct + field \
         read lowered through emit_module, bump-allocated an 8-byte-slot record, \
         executed in WABT value-matching CPython {PINS:?}. Structs are real."
    );
}

#[test]
fn float_field_executes_and_matches_cpython() {
    // r = Rec(5, 2.5); return r.f  → f64 2.5. Exercises an f64 field slot.
    let m = module(
        "rec",
        vec![
            rec_def(),
            func(
                "getf",
                Type::F64,
                vec![],
                vec![Stmt::Let {
                    name: "r".into(),
                    ty: Type::Struct("Rec".into()),
                    mutable: false,
                    value: Expr::StructLit {
                        name: "Rec".into(),
                        fields: vec![
                            ("n".into(), Expr::LitInt(5)),
                            ("f".into(), Expr::LitFloat(2.5)),
                        ],
                    },
                }],
                field("r", "f"),
            ),
        ],
    );
    let wat = emit_module(&m).expect("mixed int/float struct lowers");
    assert!(
        wat.contains("f64.store offset=8") && wat.contains("f64.load offset=8"),
        "the float field rides an f64 slot at offset 8:\n{wat}"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-996: skipping executed float-field witness — WABT absent");
        return;
    }
    let (stdout, ok) = assemble_and_run("float", &wat);
    assert!(ok, "wasm-interp failed:\n{stdout}");
    let line = stdout
        .lines()
        .find(|l| l.starts_with("getf() => "))
        .unwrap_or_else(|| panic!("no getf export:\n{stdout}"));
    assert!(
        line.contains("f64:2.5"),
        "float field: WASM {line:?} but CPython Rec(5, 2.5).f = 2.5"
    );
}

#[test]
fn struct_param_field_read_executes_and_matches_cpython() {
    // def px(p: Point) -> int: return p.x + p.y  — a struct PARAM (i32 base
    // pointer). Splice a driver that preloads a Point {x:11, y:31} at address 0
    // and calls px(0); the executed sum must == CPython 11 + 31 = 42.
    let add = Expr::BinOp {
        op: BinOp::Add,
        lhs: Box::new(field("p", "x")),
        rhs: Box::new(field("p", "y")),
    };
    let m = module(
        "param",
        vec![
            point_def(),
            func(
                "px",
                Type::I64,
                vec![Param {
                    name: "p".into(),
                    ty: Type::Struct("Point".into()),
                    mutable: false,
                }],
                vec![],
                add,
            ),
        ],
    );
    let kernel = emit_module(&m).expect("struct-param program lowers");
    assert!(
        kernel.contains("(func $px (param $p i32) (result i64)"),
        "struct param is an i32 base-pointer:\n{kernel}"
    );

    if !wasm_runtime_available() {
        eprintln!("PMAT-996: skipping executed struct-param witness — WABT absent");
        return;
    }

    // Splice a driver: preload Point{x:11,y:31} at address 0 (x @ +0 i64, y @ +8
    // i64), and a zero-arg `run` export calling px(0).
    let close = kernel.rfind(')').expect("closing paren");
    let mut wat = String::new();
    wat.push_str(&kernel[..close]);
    // x = 11 (i64 LE @ 0), y = 31 (i64 LE @ 8).
    let le = |v: i64| -> String {
        v.to_le_bytes()
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>()
    };
    wat.push_str(&format!("  (data (i32.const 0) \"{}\")\n", le(11)));
    wat.push_str(&format!("  (data (i32.const 8) \"{}\")\n", le(31)));
    wat.push_str("  (func (export \"run\") (result i64)\n    i32.const 0\n    call $px)\n");
    wat.push_str(")\n");

    let (stdout, ok) = assemble_and_run("param", &wat);
    assert!(ok, "wasm-interp failed:\n{stdout}\n---WAT---\n{wat}");
    let got = parse_i64_export(&stdout, "run");
    assert_eq!(
        got, 42,
        "struct-param p.x+p.y: WASM {got} but CPython 11+31 = 42"
    );
    eprintln!(
        "PMAT-996: EXECUTED struct-param witness PASSED — px(p) read p.x + p.y from \
         a preloaded heap record and executed to 42 == CPython."
    );
}
