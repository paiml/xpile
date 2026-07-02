//! PMAT-1028 — EXECUTED str-LOCAL witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + the PMAT-986 string runtime).
//!
//! Before this slice the WASM string subset carried str PARAMS, string
//! LITERALS, and heap-constructed `Concat`/`Chr`/`s[i]` results — but NO str
//! locals: `out: str = ""` refused at `map_type`, which killed the single
//! most common real-Python string shape, the ACCUMULATOR
//! (`out = out + chr(…)` in a loop). This slice:
//!
//! - registers str-annotated `Let` locals in the scope's str-name set (the
//!   same length-prefixed i32 base-pointer a param carries, so every read
//!   path — len/ord/concat/eq/`s[i]` — is shared);
//! - routes str-name `Let`/`Assign` values through the dedicated string
//!   lowering (`emit_str_expr`), NOT the generic i32-typed path — a bool is
//!   i32 too, and the typed path could silently bind a 0/1 as a "pointer";
//! - admits CALLS of PROVEN str-returning callables (free fns, assoc fns,
//!   struct methods — a new `StrReturners` registry) into string positions:
//!   the factory-composition idiom `s: str = build(5)`. An i32 result alone
//!   is ambiguous (bool/struct returns are i32 too), so the gate is the
//!   registry, never the WAT type;
//! - extends the `$__wasm_str_eq` pre-scan and `binop_operand_is_string` to
//!   see str-returning calls, so `dup("a") == "aa"` (no str name anywhere)
//!   still pulls in the content-compare helper — an UNDER-detection there is
//!   a hard wat2wasm failure.
//!
//! Reassignment is CPython-exact by construction: strings are immutable, so
//! rebinding the local to the fresh Concat pointer IS Python's rebind; no
//! alias disposition is needed (unlike lists/structs).
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

fn lit_i(v: i64) -> Expr {
    Expr::LitInt(v)
}

fn lit_s(s: &str) -> Expr {
    Expr::LitStr(s.into())
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

fn lt(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn eq(l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn concat(l: Expr, r: Expr) -> Expr {
    Expr::Concat {
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn chr(v: Expr) -> Expr {
    Expr::Chr { value: Box::new(v) }
}

fn ord_at(name: &str, idx: Expr) -> Expr {
    Expr::Ord {
        value: Box::new(Expr::StrCharAt {
            string: Box::new(ident(name)),
            index: Box::new(idx),
        }),
    }
}

fn len_of(name: &str) -> Expr {
    Expr::Len(Box::new(ident(name)))
}

fn call(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: callee.into(),
        args,
    }
}

fn let_ty(name: &str, ty: Type, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty,
        mutable: true,
        value,
    }
}

fn let_str(name: &str, value: Expr) -> Stmt {
    let_ty(name, Type::Str, value)
}

fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: name.into(),
        value,
    }
}

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.into(),
        ty,
        mutable: false,
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

fn if_expr(cond: Expr, then_v: i64, else_v: i64) -> Expr {
    Expr::IfExpr {
        cond: Box::new(cond),
        then_expr: Box::new(lit_i(then_v)),
        else_expr: Box::new(lit_i(else_v)),
    }
}

/// ```python
/// class Tag:
///     code: int
///     def tag(self) -> str:
///         return chr(self.code)
/// ```
fn tag_def() -> Item {
    let tag = Function {
        name: "tag".into(),
        params: vec![Param {
            name: "self".into(),
            ty: Type::Struct("Tag".into()),
            mutable: false,
        }],
        return_type: Type::Str,
        body: Block {
            stmts: vec![],
            trailing_return: chr(Expr::FieldAccess {
                obj: Box::new(ident("self")),
                field: "code".into(),
            }),
        },
    };
    Item::Struct {
        name: "Tag".into(),
        fields: vec![("code".into(), Type::I64)],
        methods: vec![tag],
        frozen: false,
        order: false,
    }
}

/// The zero-arg probe module: str locals, reassignment, the accumulator
/// loop, aliasing/content-eq, and str-returning calls feeding str locals.
fn probe_module() -> Module {
    module(
        "str_local_witness",
        vec![
            tag_def(),
            // def dup(s: str) -> str: return s + s
            func(
                "dup",
                Type::Str,
                vec![param("s", Type::Str)],
                vec![],
                concat(ident("s"), ident("s")),
            ),
            // ── THE ACCUMULATOR (the headline) ──
            // out = ""; i = 0
            // while i < 5: out = out + chr(65 + i); i = i + 1
            // return len(out)*100 + ord(out[4])                       → 569
            func(
                "accum",
                Type::I64,
                vec![],
                vec![
                    let_str("out", lit_s("")),
                    let_ty("i", Type::I64, lit_i(0)),
                    Stmt::While {
                        cond: lt(ident("i"), lit_i(5)),
                        body: vec![
                            assign("out", concat(ident("out"), chr(add(lit_i(65), ident("i"))))),
                            assign("i", add(ident("i"), lit_i(1))),
                        ],
                    },
                ],
                add(mul(len_of("out"), lit_i(100)), ord_at("out", lit_i(4))),
            ),
            // ── CONTENT equality, not pointer equality ──
            // a = "hi"; b = a; c = a + ""
            // return 1 if b == c else 0                               → 1
            // (`c` is a FRESH heap string; a pointer compare would give 0)
            func(
                "content_eq",
                Type::I64,
                vec![],
                vec![
                    let_str("a", lit_s("hi")),
                    let_str("b", ident("a")),
                    let_str("c", concat(ident("a"), lit_s(""))),
                ],
                if_expr(eq(ident("b"), ident("c")), 1, 0),
            ),
            // ── THE FACTORY COMPOSITION ──
            // t = dup("ab")
            // return (100 if t == "abab" else 0) + len(t)             → 104
            // (a str local bound to a str-returning CALL — refused before
            //  this slice; the gate is the StrReturners registry, not the
            //  ambiguous i32 result)
            func(
                "factory_len",
                Type::I64,
                vec![],
                vec![
                    let_str("t", call("dup", vec![lit_s("ab")])),
                    let_ty(
                        "hit",
                        Type::I64,
                        if_expr(eq(ident("t"), lit_s("abab")), 100, 0),
                    ),
                ],
                add(ident("hit"), len_of("t")),
            ),
            // ── A str-returning METHOD feeding a str local ──
            // g = Tag(66); u = g.tag(); return ord(u[0])              → 66
            func(
                "method_tag",
                Type::I64,
                vec![],
                vec![
                    let_ty(
                        "g",
                        Type::Struct("Tag".into()),
                        Expr::StructLit {
                            name: "Tag".into(),
                            fields: vec![("code".into(), lit_i(66))],
                        },
                    ),
                    let_str(
                        "u",
                        Expr::MethodCall {
                            obj: Box::new(ident("g")),
                            method: "tag".into(),
                            args: vec![],
                        },
                    ),
                ],
                ord_at("u", lit_i(0)),
            ),
            // ── DIRECT call-operand equality, NO str name in the fn ──
            // return 1 if dup("a") == "aa" else 0                     → 1
            // (the $__wasm_str_eq pre-scan must see the str-returning
            //  CALL — an under-detection is a missing helper and a hard
            //  wat2wasm failure, not a refusal)
            func(
                "eq_direct",
                Type::I64,
                vec![],
                vec![],
                if_expr(eq(call("dup", vec![lit_s("a")]), lit_s("aa")), 1, 0),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every zero-arg probe export.
/// Verified: `python3 -c "…identical program…; print(result)"` → 569 1 104 66 1.
const PINS: &[(&str, i64)] = &[
    ("accum", 569),
    ("content_eq", 1),
    ("factory_len", 104),
    ("method_tag", 66),
    ("eq_direct", 1),
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
    let dir = std::env::temp_dir().join(format!(
        "xpile-wasm-strlocal-{}-{}",
        tag,
        std::process::id()
    ));
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
fn str_locals_declare_as_i32_pointer_slots() {
    let wat = emit_module(&probe_module()).expect("str-local program lowers");
    assert!(
        wat.contains("(local $out i32)"),
        "the accumulator's str local is an i32 pointer slot:\n{wat}"
    );
    assert!(
        wat.contains("call $__wasm_str_eq"),
        "content equality over str locals routes through the helper:\n{wat}"
    );
    assert!(
        wat.contains("(func $__wasm_str_eq"),
        "the eq helper is emitted (pre-scan saw locals + calls):\n{wat}"
    );
    assert!(
        wat.contains("call $dup"),
        "str-returning free-fn calls emit against the plain symbol:\n{wat}"
    );
}

#[test]
fn eq_helper_pre_scan_sees_direct_call_operands() {
    // A module whose ONLY string equality is over a CALL result (no str
    // name anywhere): the pre-scan must still emit $__wasm_str_eq — a miss
    // is invalid WAT (a call against a missing helper), not a refusal.
    let m = module(
        "eq_only",
        vec![
            func(
                "dup",
                Type::Str,
                vec![param("s", Type::Str)],
                vec![],
                concat(ident("s"), ident("s")),
            ),
            func(
                "probe",
                Type::I64,
                vec![],
                vec![],
                if_expr(eq(call("dup", vec![lit_s("a")]), lit_s("aa")), 1, 0),
            ),
        ],
    );
    let wat = emit_module(&m).expect("call-operand equality lowers");
    assert!(
        wat.contains("(func $__wasm_str_eq"),
        "the pre-scan detects a str-returning CALL as an eq operand:\n{wat}"
    );
}

#[test]
fn non_str_call_in_string_position_is_refused() {
    // s: str = get_n() where get_n returns int — the i32/i64 result is NOT
    // a string; the str-returner gate must refuse (never bind a scalar as
    // a "pointer").
    let m = module(
        "bad",
        vec![
            func("get_n", Type::I64, vec![], vec![], lit_i(7)),
            func(
                "f",
                Type::I64,
                vec![],
                vec![let_str("s", call("get_n", vec![]))],
                len_of("s"),
            ),
        ],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("string position"),
        "a non-str-returning call must not feed a str local: {err}"
    );
}

#[test]
fn non_str_name_in_string_position_is_refused() {
    // flag: bool = True; s: str = flag — a bool is i32 like a str pointer;
    // the generic typed path would silently bind 0/1 as an "address". The
    // string lowering must refuse by NAME classification, not WAT type.
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![
                let_ty("flag", Type::Bool, Expr::LitBool(true)),
                let_str("s", ident("flag")),
            ],
            len_of("s"),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("not a `str` parameter or str-annotated local"),
        "a non-str name in a string position refuses honestly: {err}"
    );
}

// ---- EXECUTED witnesses (gated on WABT) ------------------------------------

#[test]
fn str_local_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("str-local program lowers");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1028: skipping EXECUTED str-local witness — WABT absent. The \
             program lowered through emit_module (asserted in \
             `str_locals_declare_as_i32_pointer_slots`); a box with WABT runs \
             every export and asserts each == CPython {PINS:?}."
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
        "PMAT-1028: EXECUTED str-local witness PASSED — the accumulator loop, \
         alias/content equality over a fresh heap copy, the factory \
         composition (str local ← str-returning call), a str-returning \
         method, and a direct call-operand equality all executed in WABT \
         value-matching CPython {PINS:?}."
    );
}
