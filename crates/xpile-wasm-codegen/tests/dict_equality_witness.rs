//! PMAT-1243 — EXECUTED witness for native-WASM DICT equality `d1 == d2` /
//! `d1 != d2` (`Expr::BinOp { Eq | NotEq }` over two dict locals) on the
//! bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## The bug this fixes
//!
//! A dict rides an i32 base-pointer, INDISTINGUISHABLE from a bool/int i32 in
//! the opcode table — so before this slice `d1 == d2` was REFUSED honestly (a
//! prior slice, PMAT-1242, wired set equality but left the dict `==` unwired
//! rather than let it fall through to a BASE-POINTER compare that says
//! `{1:2} == {1:2}` is False). This slice makes it CORRECT.
//!
//! ## The fix
//!
//! `emit_binop` now intercepts a dict-valued operand and routes `==`/`!=` to a
//! real `$__wasm_dict_eq_<k>(p, q) -> i32`:
//!
//! ```wat
//! ;; |p| != |q|                        → 0   (cheap header compare, first)
//! ;; else walk p; any key ∉ q          → 0   (reuses the never-trapping has)
//! ;;          or p[k] != q[k] (i64.ne) → 0   (get is safe — has just said k∈q)
//! ;; else                              → 1
//! ```
//!
//! Dict keys are unique, so `|p| == |q|` AND `∀k∈p: k∈q ∧ p[k]==q[k]` ⟺
//! `p == q` — no need to also walk q. It reuses `$__wasm_dict_has_<k>` (probe)
//! and `$__wasm_dict_get_<k>` (value fetch, only after `has` confirmed the key),
//! so NO new helper dependency is introduced. `!=` is the `i32.eqz` inversion.
//!
//! ## The load-bearing distinction from set equality
//!
//! `eq_same_key_diff_val` — `{1:10, 2:20} == {1:10, 2:99}` — is the witness that
//! dict equality is NOT set equality: the KEY sets are identical, so a
//! membership-only `$__wasm_set_eq_<k>` would wrongly say **equal**. Dict `==`
//! must also compare VALUES → **not equal**, matching CPython. `eq_str_diff_val`
//! and `ne_diff_val` pin the same property on str keys and `!=`.
//!
//! **Order-INDEPENDENCE**: `eq_after_del` builds `{1:10, 2:20, 3:30}`, `del`s
//! key 2 (a swap-into-hole removal that reorders storage), and compares to
//! `{3:30, 1:10}` → still equal. The boolean result never depends on entry
//! order, exactly as for set equality.
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG `bool` export returning the `==`/`!=` result. Every
//! value pin is cross-checked against live `python3` in `cpython_pins_are_python`.
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper) without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `<name>: dict[int, int] = {k0: v0, …}` — an int-keyed, int-valued dict local.
fn idict(name: &str, entries: &[(i64, i64)]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(
            entries
                .iter()
                .map(|&(k, v)| (Expr::LitInt(k), Expr::LitInt(v)))
                .collect(),
        ),
    }
}

/// `<name>: dict[str, int] = {"k0": v0, …}` — str keys (CONTENT compare).
fn sdict(name: &str, entries: &[(&str, i64)]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(
            entries
                .iter()
                .map(|&(k, v)| (Expr::LitStr(k.into()), Expr::LitInt(v)))
                .collect(),
        ),
    }
}

/// `del <name>[key]` — a removal that reorders the entry array (swap-into-hole).
fn del(name: &str, key: Expr) -> Stmt {
    Stmt::DelItem {
        name: name.into(),
        key,
        is_dict: true,
    }
}

/// `a == b` over two dict names.
fn eq(l: &str, r: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(ident(l)),
        rhs: Box::new(ident(r)),
    }
}

/// `a != b` over two dict names.
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
        "dict_equality_witness",
        vec![
            // ── int-keyed equality ────────────────────────────────────────────
            // permuted key/value pairs → equal
            cmp_fn(
                "eq_reorder",
                idict("a", &[(1, 10), (2, 20), (3, 30)]),
                idict("b", &[(3, 30), (2, 20), (1, 10)]),
                eq("a", "b"),
            ),
            cmp_fn(
                "eq_identical",
                idict("a", &[(1, 10), (2, 20)]),
                idict("b", &[(1, 10), (2, 20)]),
                eq("a", "b"),
            ),
            // |p| > |q| → not equal
            cmp_fn(
                "eq_size_smaller",
                idict("a", &[(1, 10), (2, 20), (3, 30)]),
                idict("b", &[(1, 10), (2, 20)]),
                eq("a", "b"),
            ),
            // |p| < |q| → not equal (size check is load-bearing)
            cmp_fn(
                "eq_size_larger",
                idict("a", &[(1, 10), (2, 20)]),
                idict("b", &[(1, 10), (2, 20), (3, 30)]),
                eq("a", "b"),
            ),
            // SAME keys, one differing VALUE → not equal. THE dict-vs-set witness:
            // a membership-only set_eq would wrongly say equal here.
            cmp_fn(
                "eq_same_key_diff_val",
                idict("a", &[(1, 10), (2, 20)]),
                idict("b", &[(1, 10), (2, 99)]),
                eq("a", "b"),
            ),
            // same size + same values, one differing KEY → not equal
            cmp_fn(
                "eq_diff_key_same_val",
                idict("a", &[(1, 10), (2, 20)]),
                idict("b", &[(1, 10), (3, 20)]),
                eq("a", "b"),
            ),
            // `!=` inversion, both truth values
            cmp_fn(
                "ne_reorder",
                idict("a", &[(1, 10), (2, 20)]),
                idict("b", &[(2, 20), (1, 10)]),
                ne("a", "b"),
            ),
            cmp_fn(
                "ne_diff_val",
                idict("a", &[(1, 10)]),
                idict("b", &[(1, 99)]),
                ne("a", "b"),
            ),
            // empty-dict edges
            cmp_fn("eq_empty", idict("a", &[]), idict("b", &[]), eq("a", "b")),
            cmp_fn(
                "eq_empty_vs_one",
                idict("a", &[]),
                idict("b", &[(1, 10)]),
                eq("a", "b"),
            ),
            // order-independence AFTER a swap-into-hole removal: build
            // {1:10,2:20,3:30}, del key 2 (reorders storage), compare to
            // {3:30,1:10} → still equal.
            func(
                "eq_after_del",
                vec![
                    idict("a", &[(1, 10), (2, 20), (3, 30)]),
                    del("a", Expr::LitInt(2)),
                    idict("b", &[(3, 30), (1, 10)]),
                ],
                eq("a", "b"),
            ),
            // ── str-keyed equality (CONTENT compare via $__wasm_str_eq) ───────
            cmp_fn(
                "eq_str_reorder",
                sdict("a", &[("a", 1), ("bb", 2)]),
                sdict("b", &[("bb", 2), ("a", 1)]),
                eq("a", "b"),
            ),
            // same str keys, differing value → not equal (dict-vs-set on str keys)
            cmp_fn(
                "eq_str_diff_val",
                sdict("a", &[("a", 1)]),
                sdict("b", &[("a", 2)]),
                eq("a", "b"),
            ),
            cmp_fn(
                "eq_str_diff_key",
                sdict("a", &[("a", 1)]),
                sdict("b", &[("b", 1)]),
                eq("a", "b"),
            ),
            cmp_fn(
                "ne_str_diff",
                sdict("a", &[("a", 1), ("bb", 2)]),
                sdict("b", &[("a", 1), ("bb", 9)]),
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
    ("eq_same_key_diff_val", 0),
    ("eq_diff_key_same_val", 0),
    ("ne_reorder", 0),
    ("ne_diff_val", 1),
    ("eq_empty", 1),
    ("eq_empty_vs_one", 0),
    ("eq_after_del", 1),
    ("eq_str_reorder", 1),
    ("eq_str_diff_val", 0),
    ("eq_str_diff_key", 0),
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dicteq-{}", std::process::id()));
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
fn dict_equality_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the dict `==`/`!=` program must lower through emit_module");
    // Both key kinds are present, so both dict-equality helpers are emitted AND
    // called at the comparison sites — never a raw `i32.eq` on pointers.
    for helper in [
        "func $__wasm_dict_eq_i",
        "func $__wasm_dict_eq_s",
        "call $__wasm_dict_eq_i",
        "call $__wasm_dict_eq_s",
    ] {
        assert!(wat.contains(helper), "missing {helper}:\n{wat}");
    }
    // The helper reuses the never-trapping membership probe AND the value fetch —
    // the value fetch is what makes it a DICT equality, not a set equality.
    for helper in [
        "call $__wasm_dict_has_i",
        "call $__wasm_dict_get_i",
        "call $__wasm_dict_has_s",
        "call $__wasm_dict_get_s",
    ] {
        assert!(
            wat.contains(helper),
            "dict equality must reuse the has+get helpers {helper}:\n{wat}"
        );
    }
    // str-keyed equality forces the CONTENT-compare helper.
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed dict equality must carry the content-compare helper:\n{wat}"
    );
}

#[test]
fn dict_equality_refuses_ordering_and_mixed_operands() {
    // Dict ORDERING (`<`) has no meaning and must be REFUSED honestly (not a
    // silent pointer compare) — only `==`/`!=` are wired.
    let dict_lt = module(
        "dicteq_lt",
        vec![func(
            "f",
            vec![idict("a", &[(1, 10)]), idict("b", &[(1, 10)])],
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
        "dict `<` refusal must name the equality-only boundary: {msg}"
    );

    // A dict compared with a NON-dict operand is refused (a dict only equals a
    // dict).
    let mixed = module(
        "dicteq_mixed",
        vec![func(
            "f",
            vec![idict("a", &[(1, 10)])],
            Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(ident("a")),
                rhs: Box::new(Expr::LitInt(1)),
            },
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed).expect_err("dict == int must be refused")
    );
    assert!(
        msg.contains("dict") || msg.contains("name"),
        "mixed dict/non-dict `==` must be refused honestly: {msg}"
    );

    // Two dicts of DIFFERENT key kinds can never be equal — refused honestly
    // rather than routed at a mismatched helper.
    let mixed_kind = module(
        "dicteq_mixed_kind",
        vec![func(
            "f",
            vec![idict("a", &[(1, 10)]), sdict("b", &[("x", 1)])],
            eq("a", "b"),
        )],
    );
    let msg = format!(
        "{:?}",
        emit_module(&mixed_kind).expect_err("dict[int,_] == dict[str,_] must be refused")
    );
    assert!(
        msg.contains("key kind"),
        "mixed-kind dict `==` must name the key-kind mismatch: {msg}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_equality_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1243: skipping EXECUTED dict-equality witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             the $__wasm_dict_eq_<k> helper (asserted in \
             `dict_equality_lowers_and_carries_helper`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1243: running EXECUTED dict-equality witness via WABT");
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
        "no dict-equality probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1243: EXECUTED dict-equality witness PASSED — permuted dicts compared \
         equal, the size check rejected differing sizes, a same-keys/differing-VALUE \
         pair compared UNEQUAL (the case a membership-only set_eq gets wrong), a \
         differing key compared unequal, `!=` inverted, empty-dict edges held, \
         order-independence survived a swap-into-hole `del`, and the str-keyed \
         content-compare path matched. All == CPython {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
v={}\n\
v['eq_reorder']=int({1:10,2:20,3:30}=={3:30,2:20,1:10})\n\
v['eq_identical']=int({1:10,2:20}=={1:10,2:20})\n\
v['eq_size_smaller']=int({1:10,2:20,3:30}=={1:10,2:20})\n\
v['eq_size_larger']=int({1:10,2:20}=={1:10,2:20,3:30})\n\
v['eq_same_key_diff_val']=int({1:10,2:20}=={1:10,2:99})\n\
v['eq_diff_key_same_val']=int({1:10,2:20}=={1:10,3:20})\n\
v['ne_reorder']=int({1:10,2:20}!={2:20,1:10})\n\
v['ne_diff_val']=int({1:10}!={1:99})\n\
v['eq_empty']=int({}=={})\n\
v['eq_empty_vs_one']=int({}=={1:10})\n\
a={1:10,2:20,3:30}\n\
del a[2]\n\
v['eq_after_del']=int(a=={3:30,1:10})\n\
v['eq_str_reorder']=int({'a':1,'bb':2}=={'bb':2,'a':1})\n\
v['eq_str_diff_val']=int({'a':1}=={'a':2})\n\
v['eq_str_diff_key']=int({'a':1}=={'b':1})\n\
v['ne_str_diff']=int({'a':1,'bb':2}!={'a':1,'bb':9})\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1243: python3 absent — pins asserted against the WABT witness only");
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
