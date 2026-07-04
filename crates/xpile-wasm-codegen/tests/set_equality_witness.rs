//! PMAT-1242 — EXECUTED witness for native-WASM SET equality `s1 == s2` /
//! `s1 != s2` (`Expr::BinOp { Eq | NotEq }` over two set locals) on the
//! bump-heap set runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## The bug this fixes
//!
//! A set rides an i32 base-pointer, INDISTINGUISHABLE from a bool/int i32 in the
//! opcode table — so before this slice `s1 == s2` fell through to the scalar
//! path and emitted `local.get $a; local.get $b; i32.eq`, a BASE-POINTER
//! compare. Two structurally-equal sets built from different literals land at
//! different heap addresses, so `{1,2,3} == {3,2,1}` compiled to **False**
//! while CPython says **True** — a SILENT MISCOMPILE, worse than a refusal.
//!
//! ## The fix
//!
//! `emit_binop` now intercepts a set-valued operand and routes `==`/`!=` to a
//! real membership helper `$__wasm_set_eq_<k>(p, q) -> i32`:
//!
//! ```wat
//! ;; |p| != |q|            → 0   (cheap header compare, first)
//! ;; else walk p; any key ∉ q → 0   (reuses the never-trapping has helper)
//! ;; else                   → 1
//! ```
//!
//! A set has no duplicate keys, so `|p| == |q|` AND `p ⊆ q` ⟺ `p == q` — no
//! need to also walk q. It reuses `$__wasm_dict_has_<k>` (which a set of that
//! kind already forces), so NO new helper dependency is introduced. `!=` is the
//! `i32.eqz` inversion of `==`.
//!
//! **Order-INDEPENDENCE** is the key correctness property: the boolean result
//! never depends on the swap-into-hole storage order a removal leaves behind, so
//! `eq_after_discard` (build `{1,2,3}`, `discard(2)`, compare to `{3,1}`) is
//! CPython-exact even though `discard` reorders the entry array. This is why set
//! equality is tractable and correct while full dict *iteration* (order-exposing)
//! is not yet wired.
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG `bool` export returning the `==`/`!=` result:
//!
//!   * `eq_reorder` / `eq_identical` — permuted + identical members → equal (the
//!     exact case that previously miscompiled to False);
//!   * `eq_size_smaller` (|p|>|q|) and `eq_size_larger` (|p|<|q|, p ⊆ q) — the
//!     size check is LOAD-BEARING; `{1,2} == {1,2,3}` must be False even though
//!     `{1,2}` is a subset (a subset-only check would wrongly say equal);
//!   * `eq_same_size_diff` — same size, one differing member → not equal;
//!   * `ne_reorder` / `ne_diff` — the `!=` inversion, both truth values;
//!   * `eq_empty` / `eq_empty_vs_one` — the empty-set edges;
//!   * `eq_after_discard` — order-independence after a swap-into-hole removal;
//!   * str-keyed (`$__wasm_str_eq` CONTENT compare via the has helper):
//!     reorder-equal / differing-member / differing-size / `!=`.
//!
//! Every value pin is cross-checked against live `python3` in
//! `cpython_pins_are_python`. Gated on `wasm_runtime_available()` — a clean skip
//! (still asserting the EMIT path lowers + carries the helper) without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `<name>: set[int] = {v0, v1, …}` — an int-elem set local.
fn iset(name: &str, vals: &[i64]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: true,
        value: Expr::SetLit(vals.iter().copied().map(Expr::LitInt).collect()),
    }
}

/// `<name>: set[str] = {"v0", "v1", …}` — a str-elem set local (content compare).
fn sset(name: &str, vals: &[&str]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Set(Box::new(Type::Str)),
        mutable: true,
        value: Expr::SetLit(vals.iter().map(|s| Expr::LitStr((*s).into())).collect()),
    }
}

/// `<name>.discard(e)` — a removal that reorders the entry array (swap-into-hole).
fn discard(name: &str, elem: Expr) -> Stmt {
    Stmt::SetRemove {
        set_name: name.into(),
        elem,
        error_if_absent: false,
    }
}

/// `a == b` over two set names.
fn eq(l: &str, r: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(ident(l)),
        rhs: Box::new(ident(r)),
    }
}

/// `a != b` over two set names.
fn ne(l: &str, r: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::NotEq,
        lhs: Box::new(ident(l)),
        rhs: Box::new(ident(r)),
    }
}

fn func(name: &str, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
        return_type: Type::Bool,
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

/// A `bool` export whose body binds `a` and `b` then returns `cmp(a, b)`.
fn cmp_fn(name: &str, a: Stmt, b: Stmt, cmp: Expr) -> Item {
    func(name, vec![a, b], cmp)
}

fn probe_module() -> Module {
    module(
        "set_equality_witness",
        vec![
            // ── int-keyed equality ────────────────────────────────────────────
            // permuted members → equal (the exact case that miscompiled to False)
            cmp_fn(
                "eq_reorder",
                iset("a", &[1, 2, 3]),
                iset("b", &[3, 2, 1]),
                eq("a", "b"),
            ),
            cmp_fn(
                "eq_identical",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                eq("a", "b"),
            ),
            // |p| > |q| → not equal
            cmp_fn(
                "eq_size_smaller",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2]),
                eq("a", "b"),
            ),
            // |p| < |q| AND p ⊆ q → STILL not equal (size check is load-bearing)
            cmp_fn(
                "eq_size_larger",
                iset("a", &[1, 2]),
                iset("b", &[1, 2, 3]),
                eq("a", "b"),
            ),
            // same size, one differing member → not equal
            cmp_fn(
                "eq_same_size_diff",
                iset("a", &[1, 2]),
                iset("b", &[1, 3]),
                eq("a", "b"),
            ),
            // `!=` inversion, both truth values
            cmp_fn(
                "ne_reorder",
                iset("a", &[1, 2, 3]),
                iset("b", &[3, 2, 1]),
                ne("a", "b"),
            ),
            cmp_fn(
                "ne_diff",
                iset("a", &[1, 2]),
                iset("b", &[1, 3]),
                ne("a", "b"),
            ),
            // empty-set edges
            cmp_fn("eq_empty", iset("a", &[]), iset("b", &[]), eq("a", "b")),
            cmp_fn(
                "eq_empty_vs_one",
                iset("a", &[]),
                iset("b", &[1]),
                eq("a", "b"),
            ),
            // order-independence AFTER a swap-into-hole removal: build {1,2,3},
            // discard 2 (reorders storage), compare to {3,1} → still equal.
            func(
                "eq_after_discard",
                vec![
                    iset("a", &[1, 2, 3]),
                    discard("a", Expr::LitInt(2)),
                    iset("b", &[3, 1]),
                ],
                eq("a", "b"),
            ),
            // ── str-keyed equality (CONTENT compare via $__wasm_str_eq) ───────
            cmp_fn(
                "eq_str_reorder",
                sset("a", &["a", "bb"]),
                sset("b", &["bb", "a"]),
                eq("a", "b"),
            ),
            cmp_fn(
                "eq_str_diff_member",
                sset("a", &["a", "bb"]),
                sset("b", &["a", "cc"]),
                eq("a", "b"),
            ),
            cmp_fn(
                "eq_str_size",
                sset("a", &["a"]),
                sset("b", &["a", "bb"]),
                eq("a", "b"),
            ),
            cmp_fn(
                "ne_str_diff",
                sset("a", &["a", "bb"]),
                sset("b", &["a", "cc"]),
                ne("a", "b"),
            ),
        ],
    )
}

/// The CPython-pinned truth for every export (cross-checked in
/// `cpython_pins_are_python`).
const PINS: &[(&str, i64)] = &[
    ("eq_reorder", 1),
    ("eq_identical", 1),
    ("eq_size_smaller", 0),
    ("eq_size_larger", 0),
    ("eq_same_size_diff", 0),
    ("ne_reorder", 0),
    ("ne_diff", 1),
    ("eq_empty", 1),
    ("eq_empty_vs_one", 0),
    ("eq_after_discard", 1),
    ("eq_str_reorder", 1),
    ("eq_str_diff_member", 0),
    ("eq_str_size", 0),
    ("ne_str_diff", 1),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => i32:<v>` line (`wasm-interp` prints unsigned decimal; the
/// results here are all 0/1 booleans).
fn parse_bool_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    line.rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim()
        .parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse bool for {name} from {line:?}"))
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-seteq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create work dir");
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
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

// ---- CONSTRUCT assertions (hold with or without WABT) -----------------------

#[test]
fn set_equality_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the set `==`/`!=` program must lower through emit_module");
    // Both element kinds are present, so both set-equality helpers are emitted
    // AND called at the comparison sites — never a raw `i32.eq` on pointers.
    for helper in [
        "func $__wasm_set_eq_i",
        "func $__wasm_set_eq_s",
        "call $__wasm_set_eq_i",
        "call $__wasm_set_eq_s",
    ] {
        assert!(wat.contains(helper), "missing {helper}:\n{wat}");
    }
    // The helper reuses the never-trapping membership probe (no bespoke scan).
    for helper in ["call $__wasm_dict_has_i", "call $__wasm_dict_has_s"] {
        assert!(
            wat.contains(helper),
            "set equality must reuse the has helper {helper}:\n{wat}"
        );
    }
    // str-keyed equality forces the CONTENT-compare helper.
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed set equality must carry the content-compare helper:\n{wat}"
    );
}

#[test]
fn set_equality_refuses_dict_and_mixed_operands() {
    // Dict `==`/`!=` is now WIRED (PMAT-1243); what must still be REFUSED is dict
    // ORDERING (`<`) — dicts have no order relation, and the fall-through would
    // compare base-pointers. (The dict `==` path is exercised in
    // `dict_equality_witness.rs`.)
    let dict_lt = module(
        "seteq_dict_lt",
        vec![func(
            "f",
            vec![
                Stmt::Let {
                    name: "a".into(),
                    ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
                    mutable: true,
                    value: Expr::DictLit(vec![(Expr::LitInt(1), Expr::LitInt(10))]),
                },
                Stmt::Let {
                    name: "b".into(),
                    ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
                    mutable: true,
                    value: Expr::DictLit(vec![(Expr::LitInt(1), Expr::LitInt(10))]),
                },
            ],
            Expr::BinOp {
                op: BinOp::Lt,
                lhs: Box::new(ident("a")),
                rhs: Box::new(ident("b")),
            },
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&dict_lt).expect_err("dict `<` must be refused")
    );
    assert!(
        msg.contains("dict") && msg.contains("only structural equality"),
        "dict `<` refusal must name the dict equality-only boundary: {msg}"
    );

    // A set compared with a NON-set operand is refused (a set only equals a set).
    let mixed = module(
        "seteq_mixed",
        vec![func(
            "f",
            vec![iset("a", &[1])],
            Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(ident("a")),
                rhs: Box::new(Expr::LitInt(1)),
            },
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed).expect_err("set == int must be refused")
    );
    assert!(
        msg.contains("set") || msg.contains("name"),
        "mixed set/non-set `==` must be refused honestly: {msg}"
    );

    // Two sets of DIFFERENT key kinds can never be equal — refused honestly
    // rather than routed at a mismatched helper.
    let mixed_kind = module(
        "seteq_mixed_kind",
        vec![func(
            "f",
            vec![iset("a", &[1]), sset("b", &["x"])],
            eq("a", "b"),
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed_kind).expect_err("set[int] == set[str] must be refused")
    );
    assert!(
        msg.contains("key kind"),
        "mixed-kind set `==` must name the key-kind mismatch: {msg}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_equality_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1242: skipping EXECUTED set-equality witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             the $__wasm_set_eq_<k> helper (asserted in \
             `set_equality_lowers_and_carries_helper`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1242: running EXECUTED set-equality witness via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}\n---WAT---\n{wat}");

    for &(name, expected) in PINS {
        let got = parse_bool_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n\
             full interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no set-equality probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1242: EXECUTED set-equality witness PASSED — permuted/identical sets \
         compared equal (the case that previously miscompiled to False), the size \
         check rejected a subset of different size, differing members compared \
         unequal, `!=` inverted correctly, empty-set edges held, order-independence \
         survived a swap-into-hole discard, and the str-keyed content-compare path \
         matched. All == CPython {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
v={}\n\
v['eq_reorder']=int({1,2,3}=={3,2,1})\n\
v['eq_identical']=int({1,2,3}=={1,2,3})\n\
v['eq_size_smaller']=int({1,2,3}=={1,2})\n\
v['eq_size_larger']=int({1,2}=={1,2,3})\n\
v['eq_same_size_diff']=int({1,2}=={1,3})\n\
v['ne_reorder']=int({1,2,3}!={3,2,1})\n\
v['ne_diff']=int({1,2}!={1,3})\n\
v['eq_empty']=int(set()==set())\n\
v['eq_empty_vs_one']=int(set()=={1})\n\
a={1,2,3}\n\
a.discard(2)\n\
v['eq_after_discard']=int(a=={3,1})\n\
v['eq_str_reorder']=int({'a','bb'}=={'bb','a'})\n\
v['eq_str_diff_member']=int({'a','bb'}=={'a','cc'})\n\
v['eq_str_size']=int({'a'}=={'a','bb'})\n\
v['ne_str_diff']=int({'a','bb'}!={'a','cc'})\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1242: python3 absent — pins asserted against the WABT witness only");
            return;
        }
    };
    let mut seen = 0;
    for kv in out.trim().split(';') {
        let (k, v) = kv.split_once('=').expect("k=v");
        let expected: i64 = v.parse().expect("int");
        let pinned = PINS
            .iter()
            .find(|(n, _)| *n == k)
            .unwrap_or_else(|| panic!("python produced an unpinned key {k}"))
            .1;
        assert_eq!(pinned, expected, "pin {k} drifted from CPython");
        seen += 1;
    }
    assert_eq!(seen, PINS.len(), "python3 must cover every pin");
}
