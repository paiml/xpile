//! PMAT-1030 — EXECUTED for-loop witness for the native WASM EMIT lane
//! (`C-COMPILE-RUST-TO-WASM` + the PMAT-986 string/list runtime).
//!
//! Before this slice the WASM backend had NO `Stmt::ForEach` arm at all:
//! `for x in xs` over a `list[scalar]` and `for ch in s` over a string both
//! refused as "container/aggregate statement" — while `for i in range(n)`
//! worked (the frontend desugars range-loops to Let+While upstream). This
//! slice desugars `ForEach` INSIDE the backend, before every scan/emit pass,
//! into the Let+While+`Index`/`StrCharAt` subset the backend already lowers:
//!
//! ```text
//! let __wasm_fe_i_<k>: int = 0
//! while __wasm_fe_i_<k> < len(src):
//!     let var = src[__wasm_fe_i_<k>]
//!     __wasm_fe_i_<k> = __wasm_fe_i_<k> + 1     ;; BEFORE the body
//!     <body>
//! ```
//!
//! The increment sits BEFORE the body: `continue` lowers to `br $cont`
//! (straight back to the while condition), so an increment-last desugar
//! would re-test the SAME index forever — the `skip_continue` probe below
//! executes that exact shape and value-matches CPython. `break` simply
//! exits. `ord(ch)` over a bare 1-char str name (the loop var) is also
//! unblocked here with a runtime `byte_count != 1 → unreachable` guard
//! (Python's `ord` TypeError analogue) — never a silent first-byte read.
//!
//! ## Witness shape
//!
//! Zero-arg str probes pinned to CPython (`PINS`, verified by executing the
//! identical Python through python3), plus a list-param kernel driven by a
//! spliced `(data …)` fixture, executed under WABT (`wat2wasm` +
//! `wasm-interp`) when available — a clean skip still asserts the EMIT half.

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

fn let_ty(name: &str, ty: Type, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty,
        mutable: true,
        value,
    }
}

fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: name.into(),
        value,
    }
}

fn incr(name: &str) -> Stmt {
    assign(name, add(ident(name), lit_i(1)))
}

fn if_then(cond: Expr, then_body: Vec<Stmt>) -> Stmt {
    Stmt::If {
        cond,
        then_body,
        else_body: vec![],
    }
}

/// `for var in <str iterable>: body` — the shape depyler-frontend produces
/// (a str iterable is wrapped in `Expr::StrChars`, `elem_ty` = `Str`).
fn for_str(var: &str, iterable: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::ForEach {
        var: var.into(),
        iter: Expr::StrChars {
            string: Box::new(iterable),
        },
        elem_ty: Type::Str,
        body,
        over_keys: false,
        dict_guard: None,
        mutate_elems: false,
    }
}

/// `for var in xs: body` over a named `list[int]`.
fn for_list(var: &str, list: &str, body: Vec<Stmt>) -> Stmt {
    Stmt::ForEach {
        var: var.into(),
        iter: ident(list),
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

/// Every zero-arg export mirrors this Python (executed via python3 to pin):
///
/// ```python
/// def count_hits() -> int:           # "banana tree" → 3 a's  → 3
///     n = 0
///     for ch in "banana tree":
///         if ch == "a":
///             n = n + 1
///     return n
///
/// def accum_len() -> int:            # "aabbcc"               → 6
///     out = ""
///     for ch in "abc":
///         out = out + ch + ch
///     return len(out)
///
/// def skip_continue() -> int:        # non-'a' chars of "banana" → 3
///     n = 0
///     for ch in "banana":
///         if ch == "a":
///             continue
///         n = n + 1
///     return n
///
/// def break_at() -> int:             # index of 'x' in "abxcd" → 2
///     i = 0
///     for ch in "abxcd":
///         if ch == "x":
///             break
///         i = i + 1
///     return i
///
/// def nested_pairs() -> int:         # equal (a,b) pairs of "aba" → 5
///     n = 0
///     for a in "aba":
///         for b in "aba":
///             if a == b:
///                 n = n + 1
///     return n
///
/// def ord_sum() -> int:              # ord('A') + ord('B')    → 131
///     t = 0
///     for ch in "AB":
///         t = t + ord(ch)
///     return t
///
/// def empty_iter() -> int:           # loop body never runs   → 100
///     t = 100
///     for ch in "":
///         t = t + 1
///     return t
/// ```
fn probe_module() -> Module {
    module(
        "foreach_probes",
        vec![
            func(
                "count_hits",
                Type::I64,
                vec![],
                vec![
                    let_ty("n", Type::I64, lit_i(0)),
                    for_str(
                        "ch",
                        lit_s("banana tree"),
                        vec![if_then(eq(ident("ch"), lit_s("a")), vec![incr("n")])],
                    ),
                ],
                ident("n"),
            ),
            func(
                "accum_len",
                Type::I64,
                vec![],
                vec![
                    let_ty("out", Type::Str, lit_s("")),
                    for_str(
                        "ch",
                        lit_s("abc"),
                        vec![assign(
                            "out",
                            concat(concat(ident("out"), ident("ch")), ident("ch")),
                        )],
                    ),
                ],
                Expr::Len(Box::new(ident("out"))),
            ),
            func(
                "skip_continue",
                Type::I64,
                vec![],
                vec![
                    let_ty("n", Type::I64, lit_i(0)),
                    for_str(
                        "ch",
                        lit_s("banana"),
                        vec![
                            if_then(eq(ident("ch"), lit_s("a")), vec![Stmt::Continue]),
                            incr("n"),
                        ],
                    ),
                ],
                ident("n"),
            ),
            func(
                "break_at",
                Type::I64,
                vec![],
                vec![
                    let_ty("i", Type::I64, lit_i(0)),
                    for_str(
                        "ch",
                        lit_s("abxcd"),
                        vec![
                            if_then(eq(ident("ch"), lit_s("x")), vec![Stmt::Break]),
                            incr("i"),
                        ],
                    ),
                ],
                ident("i"),
            ),
            func(
                "nested_pairs",
                Type::I64,
                vec![],
                vec![
                    let_ty("n", Type::I64, lit_i(0)),
                    for_str(
                        "a",
                        lit_s("aba"),
                        vec![for_str(
                            "b",
                            lit_s("aba"),
                            vec![if_then(eq(ident("a"), ident("b")), vec![incr("n")])],
                        )],
                    ),
                ],
                ident("n"),
            ),
            func(
                "ord_sum",
                Type::I64,
                vec![],
                vec![
                    let_ty("t", Type::I64, lit_i(0)),
                    for_str(
                        "ch",
                        lit_s("AB"),
                        vec![assign(
                            "t",
                            add(
                                ident("t"),
                                Expr::Ord {
                                    value: Box::new(ident("ch")),
                                },
                            ),
                        )],
                    ),
                ],
                ident("t"),
            ),
            func(
                "empty_iter",
                Type::I64,
                vec![],
                vec![
                    let_ty("t", Type::I64, lit_i(100)),
                    for_str("ch", lit_s(""), vec![incr("t")]),
                ],
                ident("t"),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value of the identical program.
/// Verified: python3 on the mirrored source prints 3 6 3 2 5 131 100.
const PINS: &[(&str, i64)] = &[
    ("count_hits", 3),
    ("accum_len", 6),
    ("skip_continue", 3),
    ("break_at", 2),
    ("nested_pairs", 5),
    ("ord_sum", 131),
    ("empty_iter", 100),
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
        std::env::temp_dir().join(format!("xpile-wasm-foreach-{}-{}", tag, std::process::id()));
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
fn foreach_desugars_to_synthetic_index_locals() {
    let wat = emit_module(&probe_module()).expect("for-loop programs lower");
    assert!(
        wat.contains("(local $__wasm_fe_i_0 i64)"),
        "the desugar binds a synthetic i64 index local:\n{wat}"
    );
    // nested_pairs: the inner loop takes a DISTINCT index slot — a shared
    // slot would make the inner loop advance the outer index.
    assert!(
        wat.contains("(local $__wasm_fe_i_1 i64)"),
        "nested loops number their index locals apart:\n{wat}"
    );
    // A literal iterable is bound ONCE into a synthetic PMAT-1028 str local.
    assert!(
        wat.contains("(local $__wasm_fe_s_0 i32)"),
        "a str-literal iterable binds a synthetic str source local:\n{wat}"
    );
}

#[test]
fn increment_precedes_body_so_continue_advances() {
    // In skip_continue's loop, the index increment must appear BEFORE the
    // body's `br $cont` (the `continue`): an increment-last desugar would
    // jump back to the condition without advancing and loop forever.
    let wat = emit_module(&probe_module()).expect("lowers");
    let f_start = wat.find("(func $skip_continue").expect("fn present");
    let f = &wat[f_start..];
    let f_end = f.find("(func $").map(|i| i + 1).unwrap_or(0);
    let f = &f[..f
        .match_indices("(func $")
        .nth(1)
        .map(|(i, _)| i)
        .unwrap_or(f.len())];
    let _ = f_end;
    let incr_pos = f
        .find("local.set $__wasm_fe_i_0")
        .and_then(|first| {
            f[first + 1..]
                .find("local.set $__wasm_fe_i_0")
                .map(|second| first + 1 + second)
        })
        .expect("loop increment present (init is the first set)");
    let cont_pos = f.find("br $cont").expect("continue/loop-tail br present");
    assert!(
        incr_pos < cont_pos,
        "the index increment must precede the first `br $cont`:\n{f}"
    );
}

// ---- refusal tests (always run) ---------------------------------------------

#[test]
fn dict_iteration_is_refused_precisely() {
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![Stmt::ForEach {
                var: "k".into(),
                iter: ident("d"),
                elem_ty: Type::I64,
                body: vec![],
                over_keys: true,
                dict_guard: None,
                mutate_elems: false,
            }],
            lit_i(0),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("dict iteration"),
        "dict for-loops refuse by name, not a generic statement error: {err}"
    );
}

#[test]
fn elementwise_mutation_loop_is_refused() {
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![Stmt::ForEach {
                var: "row".into(),
                iter: ident("grid"),
                elem_ty: Type::List(Box::new(Type::I64)),
                body: vec![],
                over_keys: false,
                dict_guard: None,
                mutate_elems: true,
            }],
            lit_i(0),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("in place"),
        "mutate_elems loops refuse honestly: {err}"
    );
}

#[test]
fn list_literal_iterable_desugars_to_synthetic_list_local() {
    // PMAT-1033: `for x in [1, 2]` binds the literal ONCE into a synthetic
    // list local (`__wasm_fe_l_<k>`) and iterates the name — no longer the
    // "bind the iterable to a name" refusal.
    let m = module(
        "lit_iter",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![
                let_ty("t", Type::I64, lit_i(0)),
                Stmt::ForEach {
                    var: "x".into(),
                    iter: Expr::ListLit(vec![lit_i(1), lit_i(2)]),
                    elem_ty: Type::I64,
                    body: vec![assign("t", add(ident("t"), ident("x")))],
                    over_keys: false,
                    dict_guard: None,
                    mutate_elems: false,
                },
            ],
            ident("t"),
        )],
    );
    let wat = emit_module(&m).expect("a list-literal iterable lowers");
    assert!(
        wat.contains("(local $__wasm_fe_l_0 i32)") && wat.contains("call $__alloc"),
        "the literal binds a synthetic heap-allocated list source local:\n{wat}"
    );
}

#[test]
fn non_name_call_iterable_is_refused() {
    // A list-RETURNING call iterable stays refused with the how-to-fix
    // message (no list-valued call results in the WASM subset).
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![Stmt::ForEach {
                var: "x".into(),
                iter: Expr::Call {
                    callee: "make".into(),
                    args: vec![],
                },
                elem_ty: Type::I64,
                body: vec![],
                over_keys: false,
                dict_guard: None,
                mutate_elems: false,
            }],
            lit_i(0),
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("bind the iterable to a name"),
        "a call iterable refuses with the how-to-fix message: {err}"
    );
}

#[test]
fn ord_of_non_str_name_is_refused() {
    let m = module(
        "bad",
        vec![func(
            "f",
            Type::I64,
            vec![],
            vec![let_ty("flag", Type::Bool, Expr::LitBool(true))],
            Expr::Ord {
                value: Box::new(ident("flag")),
            },
        )],
    );
    let err = emit_module(&m).unwrap_err().to_string();
    assert!(
        err.contains("not a `str` param or local"),
        "ord() of a non-str name refuses by classification: {err}"
    );
}

// ---- EXECUTED witnesses (gated on WABT) ------------------------------------

#[test]
fn foreach_str_programs_execute_and_match_cpython() {
    let wat = emit_module(&probe_module()).expect("for-loop programs lower");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1030: skipping EXECUTED for-loop witness — WABT absent. The \
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
        "PMAT-1030: EXECUTED for-loop witness PASSED — count, accumulator, \
         continue (increment-before-body), break, NESTED loops, ord(ch) \
         checksum, and the empty-string loop all executed in WABT \
         value-matching CPython {PINS:?}."
    );
}

/// The list[int]-param kernel, driven like `list_neg_index_witness`: splice
/// `(data …)` segments preloading `FIXTURE` at address 0 (i32 count @ 0,
/// i64 elems @ 8) plus a zero-arg `run` export calling `kernel(0)`. The
/// kernel is intentionally literal-free so address 0 stays outside the
/// static string-literal region.
const FIXTURE: &[i64] = &[3, 7, 11, -2, 100];

#[test]
fn foreach_list_kernel_executes_and_matches_cpython() {
    // def sum_until_neg(xs):        # [3,7,11,-2,100] → 21
    //     t = 0
    //     for x in xs:
    //         if x < 0:
    //             break
    //         t = t + x
    //     return t
    let m = module(
        "list_kernel",
        vec![func(
            "kernel",
            Type::I64,
            vec![Param {
                name: "xs".into(),
                ty: Type::List(Box::new(Type::I64)),
                mutable: false,
            }],
            vec![
                let_ty("t", Type::I64, lit_i(0)),
                for_list(
                    "x",
                    "xs",
                    vec![
                        if_then(lt(ident("x"), lit_i(0)), vec![Stmt::Break]),
                        assign("t", add(ident("t"), ident("x"))),
                    ],
                ),
            ],
            ident("t"),
        )],
    );
    let wat = emit_module(&m).expect("list for-loop lowers");
    assert!(
        wat.contains("(local $__wasm_fe_i_0 i64)") && wat.contains("i64.load"),
        "the list loop indexes through the typed element load:\n{wat}"
    );
    if !wasm_runtime_available() {
        eprintln!("PMAT-1030: skipping EXECUTED list kernel — WABT absent (emit asserted).");
        return;
    }
    // Splice the driver.
    let close = wat.rfind(')').expect("closing paren");
    let mut driven = wat[..close].to_string();
    let le_bytes = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>()
    };
    driven.push_str(&format!(
        "  (data (i32.const 0) \"{}\")\n",
        le_bytes(&(FIXTURE.len() as i32).to_le_bytes())
    ));
    for (k, v) in FIXTURE.iter().enumerate() {
        driven.push_str(&format!(
            "  (data (i32.const {}) \"{}\")\n",
            8 + k * 8,
            le_bytes(&v.to_le_bytes())
        ));
    }
    driven
        .push_str("  (func (export \"run\") (result i64)\n    i32.const 0\n    call $kernel)\n)\n");
    let (stdout, ok) = assemble_and_run("list", &driven);
    assert!(ok, "wasm-interp failed:\n{stdout}\n---WAT---\n{driven}");
    let got = parse_i64_export(&stdout, "run");
    assert_eq!(
        got, 21,
        "executed sum-until-negative over [3,7,11,-2,100] must == CPython 21\n{stdout}"
    );
    eprintln!(
        "PMAT-1030: EXECUTED list for-loop witness PASSED — `for x in xs` \
         with break over a driven list[int] == CPython 21."
    );
}
