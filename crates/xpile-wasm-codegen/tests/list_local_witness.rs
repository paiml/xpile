//! PMAT-1033 — EXECUTED list-LOCAL witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + the PMAT-968 list runtime).
//!
//! Before this slice a `list[scalar]` could only arrive as a PARAM — which
//! wasm-interp cannot supply (scalar args only) — so the whole for-loop
//! scan/filter/checksum family was unwitnessable end-to-end (sweep #11
//! finding 3: 11 of 35 programs refused on the one `map_type` List gate
//! while the RUST lane executed all of them == CPython). This slice:
//!
//! * a `list[scalar]` **LET** registers in the SAME `scope.list_elem`
//!   registry a param uses, so `xs[i]` / `xs[i] = v` / `len(xs)` and the
//!   PMAT-1030 ForEach desugar ride the bounds-guarded machinery verbatim;
//! * an `Expr::ListLit` materialises a fresh length-prefixed record on the
//!   bump heap (`$__alloc(8 + n*elem_size)`, i32 count @ base+0, packed
//!   elements @ base+8 — the exact param ABI);
//! * a list-NAME binding (`ys = xs`) is a bare pointer copy — Python's
//!   aliasing, native to linear memory (the PMAT-1024 reference posture);
//! * a list-literal ITERABLE (`for x in [5, 10, 15]`) binds ONCE into a
//!   synthetic `__wasm_fe_l_<k>` local (the PMAT-1028 str-literal pattern);
//! * **append / growth stays REFUSED** precisely (a fixed-size record
//!   cannot grow in place; relocation would break aliases — PMAT-999).
//!
//! ## Witness shape
//!
//! Zero-arg exports mirroring the sweep-#11 a-series (sum, multi-continue,
//! break, nested, shadow, index-write) pinned to CPython (`PINS`, verified
//! by executing the identical Python through python3), executed under WABT
//! (`wat2wasm` + `wasm-interp`) when available — a clean skip still asserts
//! the EMIT half.

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

fn list_lit(vals: &[i64]) -> Expr {
    Expr::ListLit(vals.iter().copied().map(Expr::LitInt).collect())
}

fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

fn add(l: Expr, r: Expr) -> Expr {
    binop(BinOp::Add, l, r)
}

fn mul(l: Expr, r: Expr) -> Expr {
    binop(BinOp::Mul, l, r)
}

fn index(name: &str, i: Expr) -> Expr {
    Expr::Index {
        collection: Box::new(ident(name)),
        index: Box::new(i),
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

fn let_list(name: &str, vals: &[i64]) -> Stmt {
    let_ty(name, Type::List(Box::new(Type::I64)), list_lit(vals))
}

fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: name.into(),
        value,
    }
}

fn if_then(cond: Expr, then_body: Vec<Stmt>) -> Stmt {
    Stmt::If {
        cond,
        then_body,
        else_body: vec![],
    }
}

/// `for var in <iter>: body` over a named `list[int]` local or a literal.
fn for_list(var: &str, iter: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::ForEach {
        var: var.into(),
        iter,
        elem_ty: Type::I64,
        body,
        over_keys: false,
        dict_guard: None,
        mutate_elems: false,
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

// ---- probe module -----------------------------------------------------------

/// Every zero-arg export mirrors this Python (executed via python3 to pin —
/// prints 23 4 10 180 3 50 10 30):
///
/// ```python
/// def scan_sum() -> int:             # a01: sum [3,7,11,2]      → 23
///     xs = [3, 7, 11, 2]
///     t = 0
///     for x in xs:
///         t = t + x
///     return t
///
/// def multi_continue() -> int:       # a03: odd non-5 of [1..6] → 4
///     xs = [1, 2, 3, 4, 5, 6]
///     t = 0
///     for x in xs:
///         if x % 2 == 0:
///             continue
///         if x == 5:
///             continue
///         t = t + x
///     return t
///
/// def break_early() -> int:          # a04: sum until >10       → 10
///     xs = [3, 7, 11, 2]
///     t = 0
///     for x in xs:
///         if x > 10:
///             break
///         t = t + x
///     return t
///
/// def nested_loops() -> int:         # a05: Σ x*y               → 180
///     xs = [1, 2, 3]
///     ys = [10, 20]
///     t = 0
///     for x in xs:
///         for y in ys:
///             t = t + x * y
///     return t
///
/// def shadow() -> int:               # a13: loop var shadows    → 3
///     x = 100
///     xs = [1, 2, 3]
///     for x in xs:
///         pass
///     return x
///
/// def idx_write() -> int:            # a15: in-place *10        → 50
///     xs = [1, 2, 3, 4]
///     i = 0
///     while i < len(xs):
///         xs[i] = xs[i] * 10
///         i = i + 1
///     return xs[0] + xs[3]
///
/// def alias_ro() -> int:             # pointer-copy sharing     → 10
///     xs = [4, 5, 6]
///     ys = xs
///     return ys[0] + xs[2]
///
/// def lit_iter() -> int:             # literal iterable         → 30
///     t = 0
///     for x in [5, 10, 15]:
///         t = t + x
///     return t
/// ```
fn probe_module() -> Module {
    module(
        "list_local_probes",
        vec![
            func(
                "scan_sum",
                Type::I64,
                vec![],
                vec![
                    let_list("xs", &[3, 7, 11, 2]),
                    let_ty("t", Type::I64, lit_i(0)),
                    for_list(
                        "x",
                        ident("xs"),
                        vec![assign("t", add(ident("t"), ident("x")))],
                    ),
                ],
                ident("t"),
            ),
            func(
                "multi_continue",
                Type::I64,
                vec![],
                vec![
                    let_list("xs", &[1, 2, 3, 4, 5, 6]),
                    let_ty("t", Type::I64, lit_i(0)),
                    for_list(
                        "x",
                        ident("xs"),
                        vec![
                            if_then(
                                binop(BinOp::Eq, binop(BinOp::Mod, ident("x"), lit_i(2)), lit_i(0)),
                                vec![Stmt::Continue],
                            ),
                            if_then(binop(BinOp::Eq, ident("x"), lit_i(5)), vec![Stmt::Continue]),
                            assign("t", add(ident("t"), ident("x"))),
                        ],
                    ),
                ],
                ident("t"),
            ),
            func(
                "break_early",
                Type::I64,
                vec![],
                vec![
                    let_list("xs", &[3, 7, 11, 2]),
                    let_ty("t", Type::I64, lit_i(0)),
                    for_list(
                        "x",
                        ident("xs"),
                        vec![
                            if_then(binop(BinOp::Gt, ident("x"), lit_i(10)), vec![Stmt::Break]),
                            assign("t", add(ident("t"), ident("x"))),
                        ],
                    ),
                ],
                ident("t"),
            ),
            func(
                "nested_loops",
                Type::I64,
                vec![],
                vec![
                    let_list("xs", &[1, 2, 3]),
                    let_list("ys", &[10, 20]),
                    let_ty("t", Type::I64, lit_i(0)),
                    for_list(
                        "x",
                        ident("xs"),
                        vec![for_list(
                            "y",
                            ident("ys"),
                            vec![assign("t", add(ident("t"), mul(ident("x"), ident("y"))))],
                        )],
                    ),
                ],
                ident("t"),
            ),
            func(
                "shadow",
                Type::I64,
                vec![],
                vec![
                    let_ty("x", Type::I64, lit_i(100)),
                    let_list("xs", &[1, 2, 3]),
                    for_list("x", ident("xs"), vec![]),
                ],
                ident("x"),
            ),
            func(
                "idx_write",
                Type::I64,
                vec![],
                vec![
                    let_list("xs", &[1, 2, 3, 4]),
                    let_ty("i", Type::I64, lit_i(0)),
                    Stmt::While {
                        cond: binop(BinOp::Lt, ident("i"), Expr::Len(Box::new(ident("xs")))),
                        body: vec![
                            Stmt::IndexAssign {
                                list_name: "xs".into(),
                                indices: vec![ident("i")],
                                value: mul(index("xs", ident("i")), lit_i(10)),
                            },
                            assign("i", add(ident("i"), lit_i(1))),
                        ],
                    },
                ],
                add(index("xs", lit_i(0)), index("xs", lit_i(3))),
            ),
            func(
                "alias_ro",
                Type::I64,
                vec![],
                vec![
                    let_list("xs", &[4, 5, 6]),
                    let_ty("ys", Type::List(Box::new(Type::I64)), ident("xs")),
                ],
                add(index("ys", lit_i(0)), index("xs", lit_i(2))),
            ),
            func(
                "lit_iter",
                Type::I64,
                vec![],
                vec![
                    let_ty("t", Type::I64, lit_i(0)),
                    for_list(
                        "x",
                        list_lit(&[5, 10, 15]),
                        vec![assign("t", add(ident("t"), ident("x")))],
                    ),
                ],
                ident("t"),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value of the identical program.
/// Verified: python3 on the mirrored source prints 23 4 10 180 3 50 10 30.
const PINS: &[(&str, i64)] = &[
    ("scan_sum", 23),
    ("multi_continue", 4),
    ("break_early", 10),
    ("nested_loops", 180),
    ("shadow", 3),
    ("idx_write", 50),
    ("alias_ro", 10),
    ("lit_iter", 30),
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
        "xpile-wasm-listlocal-{}-{}",
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
        "wat2wasm rejected the emitted module:\n{}\n---WAT---\n{}",
        String::from_utf8_lossy(&assemble.stderr),
        wat
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

// ---- EMIT-shape tests (always run) ------------------------------------------

#[test]
fn list_lit_materialises_on_the_bump_heap_under_the_param_abi() {
    let wat = emit_module(&probe_module()).expect("list-local programs lower");
    // The literal allocates via the bump allocator into the dedicated scratch,
    // writes the i32 count header, and stores elements at base+8 onward.
    assert!(
        wat.contains("(local $__wasm_list_dst i32)") && wat.contains("call $__alloc"),
        "a ListLit bump-allocates through the dedicated scratch:\n{wat}"
    );
    assert!(
        wat.contains("i64.store offset=8"),
        "elements store from base+8 (the PMAT-968 param ABI):\n{wat}"
    );
    // scan_sum's 4-element list: 8-byte header + 4*8 = 40 bytes.
    assert!(
        wat.contains("i32.const 40"),
        "the alloc size is 8 + n*elem_size:\n{wat}"
    );
}

#[test]
fn alias_binding_is_a_bare_pointer_copy() {
    let wat = emit_module(&probe_module()).expect("lowers");
    let f_start = wat.find("(func $alias_ro").expect("fn present");
    let f = &wat[f_start..];
    let f = &f[..f
        .match_indices("(func $")
        .nth(1)
        .map(|(i, _)| i)
        .unwrap_or(f.len())];
    // Exactly ONE $__alloc in alias_ro (the xs literal) — ys copies the
    // pointer instead of cloning the record (Python sharing).
    assert_eq!(
        f.matches("call $__alloc").count(),
        1,
        "ys = xs must not allocate a second record:\n{f}"
    );
    assert!(
        f.contains("local.get $xs"),
        "the alias binding reads the source pointer:\n{f}"
    );
}

// ---- refusal tests (always run) ---------------------------------------------

#[test]
fn append_keeps_the_precise_growth_refusal() {
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![
                let_list("xs", &[1, 2]),
                Stmt::ListAppend {
                    list_name: "xs".into(),
                    elem: lit_i(3),
                },
            ],
            Expr::Len(Box::new(ident("xs"))),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("list growth is outside the WASM subset")
            && err.contains("relocation would break aliases"),
        "append refuses precisely, naming the relocation hazard: {err}"
    );
}

#[test]
fn list_binding_from_a_call_is_refused() {
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![let_ty(
                "xs",
                Type::List(Box::new(Type::I64)),
                Expr::Call {
                    callee: "make".into(),
                    args: vec![],
                },
            )],
            Expr::Len(Box::new(ident("xs"))),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("binding a list local from"),
        "a list-returning call refuses honestly: {err}"
    );
}

#[test]
fn list_of_str_local_is_refused_at_registration() {
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![let_ty(
                "xs",
                Type::List(Box::new(Type::Str)),
                Expr::ListLit(vec![]),
            )],
            lit_i(0),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("list[bool], list[str], and nested lists are refused")
            || err.contains("refused"),
        "unsupported element types refuse at the local's registration: {err}"
    );
}

// ---- EXECUTED witnesses (gated on WABT) ------------------------------------

#[test]
fn list_local_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("list-local programs lower");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1033: skipping EXECUTED list-local witness — WABT absent. The \
             programs lowered through emit_module (shape asserted in the emit \
             tests); a box with WABT runs every export and asserts each == \
             CPython {PINS:?}."
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
        "PMAT-1033: EXECUTED list-local witness PASSED — sum, multi-continue, \
         break, NESTED loops, loop-var shadow, in-place index-write, \
         pointer-copy aliasing, and a literal iterable all executed in WABT \
         value-matching CPython {PINS:?}."
    );
}
