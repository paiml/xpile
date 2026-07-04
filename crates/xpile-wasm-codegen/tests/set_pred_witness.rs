//! PMAT-1245 — EXECUTED witness for native-WASM SET ordering reached through
//! `Expr::SetPred` — the shape the PYTHON FRONTEND actually produces for `a <= b`
//! / `a < b` / `a >= b` / `a > b` over two sets (subset / proper-subset /
//! superset / proper-superset). Runs on the bump-heap set runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists (the PMAT-1244 reachability gap)
//!
//! PMAT-1244 wired set ordering into `emit_binop` and shipped a witness built on
//! `Expr::BinOp { LtEq, .. }`. That emit path is correct — but the Python
//! frontend does NOT lower `s1 <= s2` to a `BinOp`; it lowers set comparison
//! operators to `Expr::SetPred { op: SetPredOp }`. So the PMAT-1244 capability,
//! though witness-green at the meta-HIR level, was UNREACHABLE from Python
//! source: a live `xpile transpile set_le.py --target wasm` still errored with
//! "expression <container/aggregate/builtin expression>" (the `SetPred` fell
//! through `emit_expr`'s catch-all before `emit_binop` ever saw it). An
//! adversarial-verify pass caught this.
//!
//! PMAT-1245 adds the `Expr::SetPred` arm to `emit_expr`, routing it to the SAME
//! `$__wasm_set_subset_<k>` helper PMAT-1244 already emits — so set ordering is
//! now reachable end-to-end. This witness builds `Expr::SetPred` (the frontend
//! shape) so a future regression that only kept the BinOp arm would fail here.
//!
//! ## The lowering
//!
//! ```text
//!   a <= b  ⇔  subset(a, b)                 (SetPredOp::Subset)
//!   a >= b  ⇔  subset(b, a)                 (SetPredOp::Superset — swapped)
//!   a <  b  ⇔  subset(a, b) ∧ |a| < |b|     (SetPredOp::ProperSubset)
//!   a >  b  ⇔  subset(b, a) ∧ |b| < |a|     (SetPredOp::ProperSuperset)
//! ```
//!
//! `a.isdisjoint(b)` (`SetPredOp::Disjoint`) needs a distinct no-common-element
//! walk and is refused honestly.
//!
//! Every value pin is cross-checked against live `python3`. Gated on
//! `wasm_runtime_available()` — a clean skip (still asserting the EMIT path
//! lowers + carries the helper) without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SetPredOp, SourceLang, Stmt, Type};
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

/// `a <pred> b` over two set names — the FRONTEND shape (`Expr::SetPred`).
fn pred(op: SetPredOp, l: &str, r: &str) -> Expr {
    Expr::SetPred {
        lhs: Box::new(ident(l)),
        op,
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

/// A `bool` export whose body binds `a` and `b` then returns `a <pred> b`.
fn cmp_fn(name: &str, a: Stmt, b: Stmt, op: SetPredOp) -> Item {
    func(name, vec![a, b], pred(op, "a", "b"))
}

fn probe_module() -> Module {
    use SetPredOp::{ProperSubset, ProperSuperset, Subset, Superset};
    module(
        "set_pred_witness",
        vec![
            // ── non-strict subset `<=` (SetPredOp::Subset) ────────────────────
            cmp_fn(
                "le_proper",
                iset("a", &[1, 2]),
                iset("b", &[1, 2, 3]),
                Subset,
            ),
            // equal sets → 1 (non-strict subset holds)
            cmp_fn(
                "le_equal",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                Subset,
            ),
            // 2 ∉ {1,3} → 0
            cmp_fn("le_not", iset("a", &[1, 2]), iset("b", &[1, 3]), Subset),
            // reordered → still a subset (order-independence)
            cmp_fn(
                "le_reorder",
                iset("a", &[3, 1]),
                iset("b", &[1, 2, 3]),
                Subset,
            ),
            // ── strict subset `<` (SetPredOp::ProperSubset) ───────────────────
            cmp_fn(
                "lt_proper",
                iset("a", &[1, 2]),
                iset("b", &[1, 2, 3]),
                ProperSubset,
            ),
            // equal sets → 0 (the size gate is LOAD-BEARING for strict `<`)
            cmp_fn(
                "lt_equal",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                ProperSubset,
            ),
            cmp_fn(
                "lt_not",
                iset("a", &[1, 2]),
                iset("b", &[1, 3]),
                ProperSubset,
            ),
            // ── superset `>=` / `>` (operand-swapped mirror) ──────────────────
            cmp_fn(
                "ge_proper",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2]),
                Superset,
            ),
            cmp_fn(
                "ge_equal",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                Superset,
            ),
            cmp_fn(
                "gt_proper",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2]),
                ProperSuperset,
            ),
            cmp_fn(
                "gt_equal",
                iset("a", &[1, 2, 3]),
                iset("b", &[1, 2, 3]),
                ProperSuperset,
            ),
            cmp_fn("ge_not", iset("a", &[1, 2]), iset("b", &[1, 3]), Superset),
            // ── empty-set edges ──────────────────────────────────────────────
            cmp_fn("le_empty", iset("a", &[]), iset("b", &[1]), Subset),
            cmp_fn("lt_empty", iset("a", &[]), iset("b", &[1]), ProperSubset),
            cmp_fn("le_empty_both", iset("a", &[]), iset("b", &[]), Subset),
            cmp_fn(
                "lt_empty_both",
                iset("a", &[]),
                iset("b", &[]),
                ProperSubset,
            ),
            // ── order-independence AFTER a swap-into-hole removal ─────────────
            func(
                "le_after_discard",
                vec![
                    iset("a", &[1, 2, 3]),
                    discard("a", Expr::LitInt(2)),
                    iset("b", &[1, 3, 5]),
                ],
                pred(Subset, "a", "b"),
            ),
            // ── str-keyed ordering (CONTENT compare via $__wasm_str_eq) ───────
            cmp_fn(
                "le_str_proper",
                sset("a", &["a"]),
                sset("b", &["a", "bb"]),
                Subset,
            ),
            cmp_fn(
                "lt_str_not",
                sset("a", &["a", "bb"]),
                sset("b", &["a", "cc"]),
                ProperSubset,
            ),
            cmp_fn(
                "ge_str_proper",
                sset("a", &["a", "bb"]),
                sset("b", &["a"]),
                Superset,
            ),
            cmp_fn(
                "gt_str_equal",
                sset("a", &["a", "bb"]),
                sset("b", &["a", "bb"]),
                ProperSuperset,
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setpred-{}", std::process::id()));
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
fn set_pred_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the SetPred set-ordering program must lower through emit_module");
    // The SetPred path routes to the SAME subset helper the BinOp path does —
    // both element kinds present → both helpers emitted AND called.
    for helper in [
        "func $__wasm_set_subset_i",
        "func $__wasm_set_subset_s",
        "call $__wasm_set_subset_i",
        "call $__wasm_set_subset_s",
    ] {
        assert!(wat.contains(helper), "missing {helper}:\n{wat}");
    }
    for helper in ["call $__wasm_dict_has_i", "call $__wasm_dict_has_s"] {
        assert!(
            wat.contains(helper),
            "set subset must reuse the has helper {helper}:\n{wat}"
        );
    }
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed set ordering must carry the content-compare helper:\n{wat}"
    );
}

#[test]
fn set_pred_refuses_mixed_and_disjoint() {
    // A set predicate against a NON-set operand is refused.
    let mixed = module(
        "setpred_mixed",
        vec![func(
            "f",
            vec![iset("a", &[1])],
            Expr::SetPred {
                lhs: Box::new(ident("a")),
                op: SetPredOp::Subset,
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
        "mixed set/non-set predicate must be refused honestly: {msg}"
    );

    // Mixed key kinds have no subset relation → refused.
    let mixed_kind = module(
        "setpred_mixed_kind",
        vec![func(
            "f",
            vec![iset("a", &[1]), sset("b", &["x"])],
            pred(SetPredOp::Subset, "a", "b"),
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed_kind).expect_err("set[int] <= set[str] must be refused")
    );
    assert!(
        msg.contains("key kind"),
        "mixed-kind set predicate must name the key-kind mismatch: {msg}"
    );

    // `a.isdisjoint(b)` (Disjoint) is NOT subset — refused honestly for now.
    let disjoint = module(
        "setpred_disjoint",
        vec![func(
            "f",
            vec![iset("a", &[1, 2]), iset("b", &[3, 4])],
            pred(SetPredOp::Disjoint, "a", "b"),
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&disjoint).expect_err("isdisjoint must be refused")
    );
    assert!(
        msg.contains("isdisjoint") || msg.contains("disjoint"),
        "isdisjoint must be refused as an unwired predicate: {msg}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_pred_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1245: skipping EXECUTED SetPred set-ordering witness — WABT \
             (wat2wasm / wasm-interp) absent. The program lowered through emit_module \
             and carries the $__wasm_set_subset_<k> helper (asserted in \
             `set_pred_lowers_and_carries_helper`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1245: running EXECUTED SetPred set-ordering witness via WABT");
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
        "no set-ordering probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1245: EXECUTED SetPred set-ordering witness PASSED — set ordering is \
         now reachable through the frontend's `Expr::SetPred` shape (not just the \
         PMAT-1244 BinOp path), all 21 exports == CPython {PINS:?}."
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
            eprintln!("PMAT-1245: python3 absent — pins asserted against the WABT witness only");
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
