//! PMAT-1247 — EXECUTED witness for native-WASM SET ALGEBRA reached through
//! `Expr::SetOp` — the shape the PYTHON FRONTEND produces for `a | b` (union),
//! `a & b` (intersection), `a - b` (difference), and `a ^ b` (symmetric
//! difference). Each yields a NEW set on the bump-heap set runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! The set-PREDICATE family (equality PMAT-1242, dict equality 1243, subset/
//! superset ordering 1244/1245, disjoint 1246) answered boolean questions ABOUT
//! two sets without allocating. Set ALGEBRA is the first set op that CONSTRUCTS
//! a new set: it `$__alloc`s a fresh keys-only region, sizes it to the worst-case
//! result (`|a|+|b|` for union / symmetric difference, `|a|` for intersection /
//! difference), and populates it via the dedup update-or-insert helper
//! `$__wasm_dict_set_<k>` — the gated ops (`&` / `-` / `^`) additionally probe
//! the other operand with the never-trapping `$__wasm_dict_has_<k>`. So a set of
//! a given key kind forces NO helper beyond the ones its literal already carries.
//!
//! Key correctness properties this pins against live `python3`:
//!   * union collapses the overlap (`|a ∪ b| ≤ |a|+|b|`; dedup keeps it exact).
//!   * intersection / difference keep exactly the gated keys of `a`.
//!   * symmetric difference is `(a − b) ∪ (b − a)` — the keys in exactly one side.
//!   * a NEW set is returned — the operands are never mutated (each op re-binds
//!     `a`/`b` fresh, so a later op sees the untouched literals).
//!   * empty-operand and self-op (`a | a`, `a - a`, `a ^ a`) edges.
//!   * str-keyed algebra goes through `$__wasm_str_eq` (CONTENT, not pointer) —
//!     `{"aa","bb"} & {"bb","cc"}` keeps `"bb"` by value.
//!
//! The result set is OBSERVED two ways: `len(c)` (cardinality → i64) and
//! `x in c` (membership → i32 bool). Gated on `wasm_runtime_available()` — a
//! clean skip (still asserting the EMIT path lowers + carries every helper)
//! without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SetOp, SourceLang, Stmt, Type};
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

/// `<name>: set[int] = <l> <op> <r>` — an int set bound to a set-algebra expr.
fn iop(name: &str, l: &str, op: SetOp, r: &str) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: true,
        value: Expr::SetOp {
            lhs: Box::new(ident(l)),
            op,
            rhs: Box::new(ident(r)),
        },
    }
}

/// `<name>: set[str] = <l> <op> <r>` — a str set bound to a set-algebra expr.
fn sop(name: &str, l: &str, op: SetOp, r: &str) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Set(Box::new(Type::Str)),
        mutable: true,
        value: Expr::SetOp {
            lhs: Box::new(ident(l)),
            op,
            rhs: Box::new(ident(r)),
        },
    }
}

/// `len(<name>)` — the result set's cardinality (→ i64).
fn slen(name: &str) -> Expr {
    Expr::Len(Box::new(ident(name)))
}

/// `<elem> in <name>` — membership in the result set (→ i32 bool).
fn imember(name: &str, elem: i64) -> Expr {
    Expr::SetContains {
        set: Box::new(ident(name)),
        elem: Box::new(Expr::LitInt(elem)),
    }
}

fn smember(name: &str, elem: &str) -> Expr {
    Expr::SetContains {
        set: Box::new(ident(name)),
        elem: Box::new(Expr::LitStr(elem.into())),
    }
}

fn func(name: &str, ret: Type, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
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

/// A `len`-observing int function: bind a, b, c = a <op> b; return len(c).
fn ilen(name: &str, a: &[i64], op: SetOp, b: &[i64]) -> Item {
    func(
        name,
        Type::I64,
        vec![iset("a", a), iset("b", b), iop("c", "a", op, "b")],
        slen("c"),
    )
}

/// A membership-observing int function: bind a, b, c = a <op> b; return x in c.
fn ihas(name: &str, a: &[i64], op: SetOp, b: &[i64], x: i64) -> Item {
    func(
        name,
        Type::Bool,
        vec![iset("a", a), iset("b", b), iop("c", "a", op, "b")],
        imember("c", x),
    )
}

fn probe_module() -> Module {
    use SetOp::{Difference as Sub, Intersection as And, SymmetricDifference as Xor, Union as Or};
    module(
        "set_algebra_witness",
        vec![
            // ── UNION `|` ────────────────────────────────────────────────────
            ilen("un_len", &[1, 2, 3], Or, &[2, 3, 4]), // {1,2,3,4} → 4
            ilen("un_disjoint_len", &[1, 2], Or, &[3, 4]), // no overlap → 4
            ihas("un_has_shared", &[1, 2, 3], Or, &[2, 3, 4], 2), // 1
            ihas("un_has_onlyb", &[1, 2, 3], Or, &[2, 3, 4], 4), // 1
            ihas("un_has_absent", &[1, 2, 3], Or, &[2, 3, 4], 9), // 0
            // ── INTERSECTION `&` ─────────────────────────────────────────────
            ilen("in_len", &[1, 2, 3], And, &[2, 3, 4]), // {2,3} → 2
            ilen("in_disjoint_len", &[1, 2], And, &[3, 4]), // ∅ → 0
            ihas("in_has_shared", &[1, 2, 3], And, &[2, 3, 4], 2), // 1
            ihas("in_has_onlya", &[1, 2, 3], And, &[2, 3, 4], 1), // 0 (1 not in b)
            // ── DIFFERENCE `-` ───────────────────────────────────────────────
            ilen("di_len", &[1, 2, 3], Sub, &[2, 3, 4]), // {1} → 1
            ihas("di_has_onlya", &[1, 2, 3], Sub, &[2, 3, 4], 1), // 1
            ihas("di_has_shared", &[1, 2, 3], Sub, &[2, 3, 4], 2), // 0
            ilen("di_full_len", &[1, 2, 3], Sub, &[1, 2, 3]), // a − a → 0
            // ── SYMMETRIC DIFFERENCE `^` ─────────────────────────────────────
            ilen("sd_len", &[1, 2, 3], Xor, &[2, 3, 4]), // {1,4} → 2
            ihas("sd_has_onlya", &[1, 2, 3], Xor, &[2, 3, 4], 1), // 1
            ihas("sd_has_onlyb", &[1, 2, 3], Xor, &[2, 3, 4], 4), // 1
            ihas("sd_has_shared", &[1, 2, 3], Xor, &[2, 3, 4], 3), // 0
            ilen("sd_disjoint_len", &[1, 2], Xor, &[3, 4]), // {1,2,3,4} → 4
            // ── empty-operand edges ──────────────────────────────────────────
            ilen("un_empty_lhs_len", &[], Or, &[1, 2]), // ∅ ∪ {1,2} → 2
            ilen("un_empty_both_len", &[], Or, &[]),    // ∅ → 0
            ilen("in_empty_lhs_len", &[], And, &[1, 2]), // ∅ ∩ {1,2} → 0
            ilen("di_empty_rhs_len", &[1, 2], Sub, &[]), // {1,2} − ∅ → 2
            ilen("sd_empty_both_len", &[], Xor, &[]),   // ∅ → 0
            // ── self-op edges (a op a) ───────────────────────────────────────
            ilen("un_self_len", &[1, 2, 3], Or, &[1, 2, 3]), // a ∪ a → 3
            ilen("in_self_len", &[1, 2, 3], And, &[1, 2, 3]), // a ∩ a → 3
            ilen("di_self_len", &[1, 2, 3], Sub, &[1, 2, 3]), // a − a → 0
            ilen("sd_self_len", &[1, 2, 3], Xor, &[1, 2, 3]), // a △ a → 0
            // ── str-keyed (CONTENT compare via $__wasm_str_eq) ───────────────
            func(
                "str_un_len",
                Type::I64,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", Or, "b"),
                ],
                slen("c"),
            ), // {"aa","bb","cc"} → 3
            func(
                "str_in_len",
                Type::I64,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", And, "b"),
                ],
                slen("c"),
            ), // {"bb"} → 1
            func(
                "str_di_len",
                Type::I64,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", Sub, "b"),
                ],
                slen("c"),
            ), // {"aa"} → 1
            func(
                "str_sd_len",
                Type::I64,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", Xor, "b"),
                ],
                slen("c"),
            ), // {"aa","cc"} → 2
            func(
                "str_in_has",
                Type::Bool,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", And, "b"),
                ],
                smember("c", "bb"),
            ), // 1 (content match, not pointer)
            func(
                "str_di_has",
                Type::Bool,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", Sub, "b"),
                ],
                smember("c", "aa"),
            ), // 1
            func(
                "str_di_has_absent",
                Type::Bool,
                vec![
                    sset("a", &["aa", "bb"]),
                    sset("b", &["bb", "cc"]),
                    sop("c", "a", Sub, "b"),
                ],
                smember("c", "bb"),
            ), // 0 ("bb" is in b, so dropped from a − b)
        ],
    )
}

/// The CPython-pinned truth for every export (cross-checked in
/// `cpython_pins_are_python`).
const PINS: &[(&str, i64)] = &[
    ("un_len", 4),
    ("un_disjoint_len", 4),
    ("un_has_shared", 1),
    ("un_has_onlyb", 1),
    ("un_has_absent", 0),
    ("in_len", 2),
    ("in_disjoint_len", 0),
    ("in_has_shared", 1),
    ("in_has_onlya", 0),
    ("di_len", 1),
    ("di_has_onlya", 1),
    ("di_has_shared", 0),
    ("di_full_len", 0),
    ("sd_len", 2),
    ("sd_has_onlya", 1),
    ("sd_has_onlyb", 1),
    ("sd_has_shared", 0),
    ("sd_disjoint_len", 4),
    ("un_empty_lhs_len", 2),
    ("un_empty_both_len", 0),
    ("in_empty_lhs_len", 0),
    ("di_empty_rhs_len", 2),
    ("sd_empty_both_len", 0),
    ("un_self_len", 3),
    ("in_self_len", 3),
    ("di_self_len", 0),
    ("sd_self_len", 0),
    ("str_un_len", 3),
    ("str_in_len", 1),
    ("str_di_len", 1),
    ("str_sd_len", 2),
    ("str_in_has", 1),
    ("str_di_has", 1),
    ("str_di_has_absent", 0),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal; every pin here is non-negative, so `u64` → `i64` is exact. The
/// `<ty>` label (`i32` for membership, `i64` for `len`) is ignored.
fn parse_scalar_export(stdout: &str, name: &str) -> i64 {
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
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setalg-{}", std::process::id()));
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
fn set_algebra_lowers_and_carries_helpers() {
    let wat = emit_module(&probe_module())
        .expect("the set-algebra program must lower through emit_module");
    // Both element kinds present → all four ops × both kinds emitted AND called.
    for op in ["union", "intersection", "difference", "symdiff"] {
        for k in ['i', 's'] {
            assert!(
                wat.contains(&format!("func $__wasm_set_{op}_{k}")),
                "missing helper def $__wasm_set_{op}_{k}:\n{wat}"
            );
            assert!(
                wat.contains(&format!("call $__wasm_set_{op}_{k}")),
                "helper $__wasm_set_{op}_{k} defined but never called:\n{wat}"
            );
        }
    }
    // Construction reuses the dedup insert + membership probe — NO bespoke helper.
    for helper in [
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_set_s",
        "call $__wasm_dict_has_i",
        "call $__wasm_dict_has_s",
        "call $__alloc",
    ] {
        assert!(
            wat.contains(helper),
            "set algebra must reuse {helper}:\n{wat}"
        );
    }
    // str-keyed algebra compares elements by CONTENT.
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed set algebra must carry the content-compare helper:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_algebra_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1247: skipping EXECUTED set-algebra witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             all four $__wasm_set_<op>_<k> helpers (asserted in \
             `set_algebra_lowers_and_carries_helpers`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1247: running EXECUTED set-algebra witness via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}\n---WAT---\n{wat}");

    for &(name, expected) in PINS {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n\
             full interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no set-algebra probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1247: EXECUTED set-algebra witness PASSED — `a | b` / `a & b` / \
         `a - b` / `a ^ b` are reachable through the frontend's `Expr::SetOp` shape, \
         each ALLOCATING a new set; all {} exports == CPython {PINS:?}.",
        PINS.len()
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
v={}\n\
v['un_len']=len({1,2,3}|{2,3,4})\n\
v['un_disjoint_len']=len({1,2}|{3,4})\n\
v['un_has_shared']=int(2 in ({1,2,3}|{2,3,4}))\n\
v['un_has_onlyb']=int(4 in ({1,2,3}|{2,3,4}))\n\
v['un_has_absent']=int(9 in ({1,2,3}|{2,3,4}))\n\
v['in_len']=len({1,2,3}&{2,3,4})\n\
v['in_disjoint_len']=len({1,2}&{3,4})\n\
v['in_has_shared']=int(2 in ({1,2,3}&{2,3,4}))\n\
v['in_has_onlya']=int(1 in ({1,2,3}&{2,3,4}))\n\
v['di_len']=len({1,2,3}-{2,3,4})\n\
v['di_has_onlya']=int(1 in ({1,2,3}-{2,3,4}))\n\
v['di_has_shared']=int(2 in ({1,2,3}-{2,3,4}))\n\
v['di_full_len']=len({1,2,3}-{1,2,3})\n\
v['sd_len']=len({1,2,3}^{2,3,4})\n\
v['sd_has_onlya']=int(1 in ({1,2,3}^{2,3,4}))\n\
v['sd_has_onlyb']=int(4 in ({1,2,3}^{2,3,4}))\n\
v['sd_has_shared']=int(3 in ({1,2,3}^{2,3,4}))\n\
v['sd_disjoint_len']=len({1,2}^{3,4})\n\
v['un_empty_lhs_len']=len(set()|{1,2})\n\
v['un_empty_both_len']=len(set()|set())\n\
v['in_empty_lhs_len']=len(set()&{1,2})\n\
v['di_empty_rhs_len']=len({1,2}-set())\n\
v['sd_empty_both_len']=len(set()^set())\n\
v['un_self_len']=len({1,2,3}|{1,2,3})\n\
v['in_self_len']=len({1,2,3}&{1,2,3})\n\
v['di_self_len']=len({1,2,3}-{1,2,3})\n\
v['sd_self_len']=len({1,2,3}^{1,2,3})\n\
v['str_un_len']=len({'aa','bb'}|{'bb','cc'})\n\
v['str_in_len']=len({'aa','bb'}&{'bb','cc'})\n\
v['str_di_len']=len({'aa','bb'}-{'bb','cc'})\n\
v['str_sd_len']=len({'aa','bb'}^{'bb','cc'})\n\
v['str_in_has']=int('bb' in ({'aa','bb'}&{'bb','cc'}))\n\
v['str_di_has']=int('aa' in ({'aa','bb'}-{'bb','cc'}))\n\
v['str_di_has_absent']=int('bb' in ({'aa','bb'}-{'bb','cc'}))\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1247: python3 absent — pins asserted against the WABT witness only");
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
