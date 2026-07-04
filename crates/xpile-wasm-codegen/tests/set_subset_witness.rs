//! PMAT-1244 — EXECUTED witness for native-WASM SET subset/superset ordering
//! `s1 <= s2` / `s1 < s2` / `s1 >= s2` / `s1 > s2` (`Expr::BinOp { Lt | LtEq |
//! Gt | GtEq }` over two set locals) on the bump-heap set runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## What this adds
//!
//! Set equality (`==`/`!=`, PMAT-1242) was the first structural set comparison;
//! this extends the set surface to the ORDERING relations. Like equality, they
//! are ORDER-INDEPENDENT (the boolean never depends on the swap-into-hole storage
//! order a removal leaves behind), so they are tractable and CPython-exact while
//! full order-exposing dict/set *iteration* is not yet wired.
//!
//! Before this slice, a set operand under `<`/`<=`/`>`/`>=` was refused (the set
//! branch of `emit_binop` only handled `==`/`!=`). It was NEVER a silent
//! base-pointer miscompile for ordering — the refusal was honest — but the
//! capability was missing.
//!
//! ## The lowering
//!
//! A new membership-only helper `$__wasm_set_subset_<k>(p, q) -> i32` returns
//! `(p ⊆ q)` — walk p, every key must be a member of q (NO size gate, unlike
//! `set_eq`). It reuses the never-trapping `$__wasm_dict_has_<k>` (already forced
//! by a set of that kind → no new helper dependency). `emit_binop` routes:
//!
//! ```text
//!   p <= q  ⇔  subset(p, q)                      (non-strict subset)
//!   p >= q  ⇔  subset(q, p)                       (operands swapped)
//!   p <  q  ⇔  subset(p, q) ∧ |p| < |q|           (PROPER subset)
//!   p >  q  ⇔  subset(q, p) ∧ |q| < |p|
//! ```
//!
//! The strict variants AND on an inline header size compare (a subset of unequal
//! size is a proper subset, since `p ⊆ q ⟹ |p| ≤ |q|` for sets — no duplicate
//! keys). Both operands are set Idents, so re-emitting one for the size reload is
//! a pure `local.get` (no double side effect).
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG `bool` export returning the ordering result:
//!
//!   * non-strict subset `<=`: proper subset → 1, equal sets → 1 (non-strict!),
//!     non-subset → 0, reordered → 1 (order-independence);
//!   * strict subset `<`: proper → 1, equal → 0 (the size gate is LOAD-BEARING —
//!     a naive `subset`-only check would wrongly say True), non-subset → 0;
//!   * superset `>=`/`>`: the operand-swapped mirror, both truth values;
//!   * empty-set edges: `∅ ⊆ {1}`, `∅ ⊊ {1}`, `∅ ⊆ ∅` → 1, `∅ ⊊ ∅` → 0;
//!   * `<=` after a swap-into-hole `discard` (order-independence over a MUTATED
//!     set's storage);
//!   * str-keyed (`$__wasm_str_eq` CONTENT compare via the has helper):
//!     proper subset / strict non-subset / superset / non-proper superset.
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

/// `a <op> b` over two set names.
fn ord(op: BinOp, l: &str, r: &str) -> Expr {
    Expr::BinOp {
        op,
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

/// A `bool` export whose body binds `a` and `b` then returns `a <op> b`.
fn cmp_fn(name: &str, a: Stmt, b: Stmt, op: BinOp) -> Item {
    func(name, vec![a, b], ord(op, "a", "b"))
}

fn probe_module() -> Module {
    use BinOp::{Gt, GtEq, Lt, LtEq};
    module(
        "set_subset_witness",
        vec![
            // ── int-keyed non-strict subset `<=` ─────────────────────────────
            // proper subset → 1
            cmp_fn("le_proper", iset("a", &[1, 2]), iset("b", &[1, 2, 3]), LtEq),
            // equal sets → 1 (NON-strict subset holds for equal sets)
            cmp_fn(
                "le_equal",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                LtEq,
            ),
            // 2 ∉ {1,3} → not a subset → 0
            cmp_fn("le_not", iset("a", &[1, 2]), iset("b", &[1, 3]), LtEq),
            // reordered members → still a subset (order-independence)
            cmp_fn(
                "le_reorder",
                iset("a", &[3, 1]),
                iset("b", &[1, 2, 3]),
                LtEq,
            ),
            // ── int-keyed strict subset `<` ──────────────────────────────────
            cmp_fn("lt_proper", iset("a", &[1, 2]), iset("b", &[1, 2, 3]), Lt),
            // equal sets → 0 (the size gate is LOAD-BEARING: a subset-only check
            // would wrongly report True for a strict `<`)
            cmp_fn("lt_equal", iset("a", &[1, 2, 3]), iset("b", &[1, 2, 3]), Lt),
            cmp_fn("lt_not", iset("a", &[1, 2]), iset("b", &[1, 3]), Lt),
            // ── int-keyed superset `>=` / `>` (operand-swapped mirror) ────────
            cmp_fn("ge_proper", iset("a", &[1, 2, 3]), iset("b", &[1, 2]), GtEq),
            cmp_fn(
                "ge_equal",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                GtEq,
            ),
            cmp_fn("gt_proper", iset("a", &[1, 2, 3]), iset("b", &[1, 2]), Gt),
            cmp_fn("gt_equal", iset("a", &[1, 2, 3]), iset("b", &[1, 2, 3]), Gt),
            // superset that is NOT a superset → 0 ({1,2} ⊉ {1,3})
            cmp_fn("ge_not", iset("a", &[1, 2]), iset("b", &[1, 3]), GtEq),
            // ── empty-set edges ──────────────────────────────────────────────
            cmp_fn("le_empty", iset("a", &[]), iset("b", &[1]), LtEq),
            cmp_fn("lt_empty", iset("a", &[]), iset("b", &[1]), Lt),
            cmp_fn("le_empty_both", iset("a", &[]), iset("b", &[]), LtEq),
            cmp_fn("lt_empty_both", iset("a", &[]), iset("b", &[]), Lt),
            // ── order-independence AFTER a swap-into-hole removal ─────────────
            // build {1,2,3}, discard 2 (reorders storage → {1,3}), then {1,3} ⊆
            // {1,3,5} → still 1.
            func(
                "le_after_discard",
                vec![
                    iset("a", &[1, 2, 3]),
                    discard("a", Expr::LitInt(2)),
                    iset("b", &[1, 3, 5]),
                ],
                ord(LtEq, "a", "b"),
            ),
            // ── str-keyed ordering (CONTENT compare via $__wasm_str_eq) ───────
            cmp_fn(
                "le_str_proper",
                sset("a", &["a"]),
                sset("b", &["a", "bb"]),
                LtEq,
            ),
            // 'bb' ∉ {'a','cc'} → not a strict subset → 0
            cmp_fn(
                "lt_str_not",
                sset("a", &["a", "bb"]),
                sset("b", &["a", "cc"]),
                Lt,
            ),
            cmp_fn(
                "ge_str_proper",
                sset("a", &["a", "bb"]),
                sset("b", &["a"]),
                GtEq,
            ),
            // equal str sets → not a PROPER superset → 0
            cmp_fn(
                "gt_str_equal",
                sset("a", &["a", "bb"]),
                sset("b", &["a", "bb"]),
                Gt,
            ),
        ],
    )
}

/// The CPython-pinned truth for every export (cross-checked in
/// `cpython_pins_are_python`).
const PINS: &[(&str, i64)] = &[
    ("le_proper", 1),
    ("le_equal", 1),
    ("le_not", 0),
    ("le_reorder", 1),
    ("lt_proper", 1),
    ("lt_equal", 0),
    ("lt_not", 0),
    ("ge_proper", 1),
    ("ge_equal", 1),
    ("gt_proper", 1),
    ("gt_equal", 0),
    ("ge_not", 0),
    ("le_empty", 1),
    ("lt_empty", 1),
    ("le_empty_both", 1),
    ("lt_empty_both", 0),
    ("le_after_discard", 1),
    ("le_str_proper", 1),
    ("lt_str_not", 0),
    ("ge_str_proper", 1),
    ("gt_str_equal", 0),
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setsubset-{}", std::process::id()));
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
fn set_subset_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the set `<`/`<=`/`>`/`>=` program must lower through emit_module");
    // Both element kinds are present, so both subset helpers are emitted AND
    // called at the ordering sites — never a raw `i32.lt_s` on pointers.
    for helper in [
        "func $__wasm_set_subset_i",
        "func $__wasm_set_subset_s",
        "call $__wasm_set_subset_i",
        "call $__wasm_set_subset_s",
    ] {
        assert!(wat.contains(helper), "missing {helper}:\n{wat}");
    }
    // The helper reuses the never-trapping membership probe (no bespoke scan).
    for helper in ["call $__wasm_dict_has_i", "call $__wasm_dict_has_s"] {
        assert!(
            wat.contains(helper),
            "set subset must reuse the has helper {helper}:\n{wat}"
        );
    }
    // str-keyed ordering forces the CONTENT-compare helper.
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed set ordering must carry the content-compare helper:\n{wat}"
    );
}

#[test]
fn set_subset_refuses_mixed_and_algebra() {
    // A set compared with a NON-set operand under `<=` is refused (a set only
    // orders against a set).
    let mixed = module(
        "setsub_mixed",
        vec![func(
            "f",
            vec![iset("a", &[1])],
            Expr::BinOp {
                op: BinOp::LtEq,
                lhs: Box::new(ident("a")),
                rhs: Box::new(Expr::LitInt(1)),
            },
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed).expect_err("set <= int must be refused")
    );
    assert!(
        msg.contains("set") || msg.contains("name"),
        "mixed set/non-set `<=` must be refused honestly: {msg}"
    );

    // Two sets of DIFFERENT key kinds can never be ordered — refused honestly
    // rather than routed at a mismatched helper.
    let mixed_kind = module(
        "setsub_mixed_kind",
        vec![func(
            "f",
            vec![iset("a", &[1]), sset("b", &["x"])],
            ord(BinOp::LtEq, "a", "b"),
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed_kind).expect_err("set[int] <= set[str] must be refused")
    );
    assert!(
        msg.contains("key kind"),
        "mixed-kind set `<=` must name the key-kind mismatch: {msg}"
    );

    // Set ALGEBRA (BinOp::BitOr = union `|`) stays refused — only equality and
    // ordering are wired.
    let union = module(
        "setsub_union",
        vec![func(
            "f",
            vec![iset("a", &[1]), iset("b", &[2])],
            ord(BinOp::BitOr, "a", "b"),
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&union).expect_err("set union `|` must be refused")
    );
    assert!(
        msg.contains("set algebra") || msg.contains("algebra"),
        "set union must be refused as unwired algebra: {msg}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_subset_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1244: skipping EXECUTED set-subset witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             the $__wasm_set_subset_<k> helper (asserted in \
             `set_subset_lowers_and_carries_helper`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1244: running EXECUTED set-subset witness via WABT");
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
        "no set-subset probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1244: EXECUTED set-subset witness PASSED — non-strict subset held \
         for a proper subset AND equal sets, strict `<` correctly rejected equal \
         sets (the size gate is load-bearing), superset mirrored via operand swap, \
         empty-set edges held, order-independence survived a swap-into-hole \
         discard, and the str-keyed content-compare path matched. All == CPython \
         {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
v={}\n\
v['le_proper']=int({1,2}<={1,2,3})\n\
v['le_equal']=int({1,2,3}<={1,2,3})\n\
v['le_not']=int({1,2}<={1,3})\n\
v['le_reorder']=int({3,1}<={1,2,3})\n\
v['lt_proper']=int({1,2}<{1,2,3})\n\
v['lt_equal']=int({1,2,3}<{1,2,3})\n\
v['lt_not']=int({1,2}<{1,3})\n\
v['ge_proper']=int({1,2,3}>={1,2})\n\
v['ge_equal']=int({1,2,3}>={1,2,3})\n\
v['gt_proper']=int({1,2,3}>{1,2})\n\
v['gt_equal']=int({1,2,3}>{1,2,3})\n\
v['ge_not']=int({1,2}>={1,3})\n\
v['le_empty']=int(set()<={1})\n\
v['lt_empty']=int(set()<{1})\n\
v['le_empty_both']=int(set()<=set())\n\
v['lt_empty_both']=int(set()<set())\n\
a={1,2,3}\n\
a.discard(2)\n\
v['le_after_discard']=int(a<={1,3,5})\n\
v['le_str_proper']=int({'a'}<={'a','bb'})\n\
v['lt_str_not']=int({'a','bb'}<{'a','cc'})\n\
v['ge_str_proper']=int({'a','bb'}>={'a'})\n\
v['gt_str_equal']=int({'a','bb'}>{'a','bb'})\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1244: python3 absent — pins asserted against the WABT witness only");
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
