//! PMAT-1246 — EXECUTED witness for native-WASM `a.isdisjoint(b)` reached
//! through `Expr::SetPred { op: SetPredOp::Disjoint }` — the shape the PYTHON
//! FRONTEND produces for `a.isdisjoint(b)`. Runs on the bump-heap set runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! PMAT-1244/1245 wired set ORDERING (`<=` / `<` / `>=` / `>`) via the
//! `$__wasm_set_subset_<k>` helper but refused `isdisjoint` honestly — disjoint
//! is not subset. PMAT-1246 adds a distinct `$__wasm_set_disjoint_<k>` helper:
//! the DUAL walk. Subset returns 0 on the FIRST key of `p` ABSENT from `q`
//! (`∀ key∈p: key∈q`); disjoint returns 0 on the FIRST key of `p` PRESENT in `q`
//! (`∀ key∈p: key∉q`). Two sets are disjoint iff they share no element.
//!
//! Key correctness properties this pins against live `python3`:
//!   * NO cardinality relation — a size-1 set can be disjoint from a size-100 one
//!     (unlike subset, which forces `|p| ≤ |q|`), so there is no size gate.
//!   * SYMMETRIC — `a.isdisjoint(b) == b.isdisjoint(a)` (walking either side is
//!     CPython-exact); the witness builds both directions.
//!   * two EMPTY sets are disjoint (the loop never runs → falls through to 1).
//!   * order-INDEPENDENT — survives a swap-into-hole `discard`.
//!   * str-keyed compare goes through `$__wasm_str_eq` (content, not pointer).
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper) without WABT.

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

/// `a.isdisjoint(b)` over two set names — the FRONTEND shape (`Expr::SetPred`).
fn disjoint(l: &str, r: &str) -> Expr {
    Expr::SetPred {
        lhs: Box::new(ident(l)),
        op: SetPredOp::Disjoint,
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

/// A `bool` export whose body binds `a` and `b` then returns `a.isdisjoint(b)`.
fn dj_fn(name: &str, a: Stmt, b: Stmt) -> Item {
    func(name, vec![a, b], disjoint("a", "b"))
}

fn probe_module() -> Module {
    module(
        "set_disjoint_witness",
        vec![
            // ── genuinely disjoint (no shared key) → 1 ────────────────────────
            dj_fn("dj_true", iset("a", &[1, 2]), iset("b", &[3, 4])),
            // ── one shared key → 0 (NOT disjoint) ─────────────────────────────
            dj_fn("dj_overlap_one", iset("a", &[1, 2]), iset("b", &[2, 3])),
            // ── fully overlapping / equal → 0 ─────────────────────────────────
            dj_fn("dj_equal", iset("a", &[1, 2, 3]), iset("b", &[1, 2, 3])),
            // ── subset relation still shares keys → 0 ─────────────────────────
            dj_fn("dj_subset", iset("a", &[1]), iset("b", &[1, 2, 3])),
            // ── NO cardinality relation: size-1 disjoint from a big set → 1 ───
            dj_fn(
                "dj_size_asymmetric",
                iset("a", &[99]),
                iset("b", &[1, 2, 3, 4, 5]),
            ),
            // ── symmetry: b.isdisjoint(a) of the overlap case → also 0 ────────
            func(
                "dj_symmetric_overlap",
                vec![iset("a", &[1, 2]), iset("b", &[2, 3])],
                disjoint("b", "a"),
            ),
            // symmetry on the disjoint case → also 1
            func(
                "dj_symmetric_true",
                vec![iset("a", &[1, 2]), iset("b", &[3, 4])],
                disjoint("b", "a"),
            ),
            // ── empty-set edges ───────────────────────────────────────────────
            // an empty set is disjoint from everything (loop never runs → 1)
            dj_fn("dj_empty_lhs", iset("a", &[]), iset("b", &[1, 2])),
            // a non-empty vs empty → walk finds nothing in the empty q → 1
            dj_fn("dj_empty_rhs", iset("a", &[1, 2]), iset("b", &[])),
            // both empty → disjoint (1)
            dj_fn("dj_empty_both", iset("a", &[]), iset("b", &[])),
            // ── order-independence AFTER a swap-into-hole removal ─────────────
            // remove the ONLY shared element (2) → now disjoint → 1
            func(
                "dj_after_discard",
                vec![
                    iset("a", &[1, 2, 3]),
                    discard("a", Expr::LitInt(2)),
                    iset("b", &[2, 5]),
                ],
                disjoint("a", "b"),
            ),
            // ── str-keyed (CONTENT compare via $__wasm_str_eq) ────────────────
            dj_fn(
                "dj_str_true",
                sset("a", &["aa", "bb"]),
                sset("b", &["cc", "dd"]),
            ),
            dj_fn(
                "dj_str_overlap",
                sset("a", &["aa", "bb"]),
                sset("b", &["bb", "cc"]),
            ),
            dj_fn("dj_str_empty", sset("a", &[]), sset("b", &["zz"])),
        ],
    )
}

/// The CPython-pinned truth for every export (cross-checked in
/// `cpython_pins_are_python`).
const PINS: &[(&str, i64)] = &[
    ("dj_true", 1),
    ("dj_overlap_one", 0),
    ("dj_equal", 0),
    ("dj_subset", 0),
    ("dj_size_asymmetric", 1),
    ("dj_symmetric_overlap", 0),
    ("dj_symmetric_true", 1),
    ("dj_empty_lhs", 1),
    ("dj_empty_rhs", 1),
    ("dj_empty_both", 1),
    ("dj_after_discard", 1),
    ("dj_str_true", 1),
    ("dj_str_overlap", 0),
    ("dj_str_empty", 1),
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setdisjoint-{}", std::process::id()));
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
fn set_disjoint_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the isdisjoint program must lower through emit_module");
    // Both element kinds present → both disjoint helpers emitted AND called.
    for helper in [
        "func $__wasm_set_disjoint_i",
        "func $__wasm_set_disjoint_s",
        "call $__wasm_set_disjoint_i",
        "call $__wasm_set_disjoint_s",
    ] {
        assert!(wat.contains(helper), "missing {helper}:\n{wat}");
    }
    // The disjoint helper reuses the never-trapping membership probe, so it
    // introduces NO helper a set of that kind does not already force.
    for helper in ["call $__wasm_dict_has_i", "call $__wasm_dict_has_s"] {
        assert!(
            wat.contains(helper),
            "set disjoint must reuse the has helper {helper}:\n{wat}"
        );
    }
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed isdisjoint must carry the content-compare helper:\n{wat}"
    );
    // isdisjoint is NOT subset — the disjoint program must NOT also call the
    // subset helper (it would be a wrong-relation miscompile).
    assert!(
        !wat.contains("call $__wasm_set_subset_"),
        "isdisjoint must route to the disjoint helper, not subset:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_disjoint_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1246: skipping EXECUTED isdisjoint witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             the $__wasm_set_disjoint_<k> helper (asserted in \
             `set_disjoint_lowers_and_carries_helper`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1246: running EXECUTED isdisjoint witness via WABT");
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
        "no isdisjoint probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1246: EXECUTED isdisjoint witness PASSED — `a.isdisjoint(b)` is \
         reachable through the frontend's `Expr::SetPred` shape via a distinct \
         DUAL helper, all {} exports == CPython {PINS:?}.",
        PINS.len()
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
v={}\n\
v['dj_true']=int({1,2}.isdisjoint({3,4}))\n\
v['dj_overlap_one']=int({1,2}.isdisjoint({2,3}))\n\
v['dj_equal']=int({1,2,3}.isdisjoint({1,2,3}))\n\
v['dj_subset']=int({1}.isdisjoint({1,2,3}))\n\
v['dj_size_asymmetric']=int({99}.isdisjoint({1,2,3,4,5}))\n\
v['dj_symmetric_overlap']=int({2,3}.isdisjoint({1,2}))\n\
v['dj_symmetric_true']=int({3,4}.isdisjoint({1,2}))\n\
v['dj_empty_lhs']=int(set().isdisjoint({1,2}))\n\
v['dj_empty_rhs']=int({1,2}.isdisjoint(set()))\n\
v['dj_empty_both']=int(set().isdisjoint(set()))\n\
a={1,2,3}\n\
a.discard(2)\n\
v['dj_after_discard']=int(a.isdisjoint({2,5}))\n\
v['dj_str_true']=int({'aa','bb'}.isdisjoint({'cc','dd'}))\n\
v['dj_str_overlap']=int({'aa','bb'}.isdisjoint({'bb','cc'}))\n\
v['dj_str_empty']=int(set().isdisjoint({'zz'}))\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1246: python3 absent — pins asserted against the WABT witness only");
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
