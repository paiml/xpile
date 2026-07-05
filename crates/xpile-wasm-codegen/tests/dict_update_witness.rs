//! PMAT-1302 — EXECUTED witness for native-WASM `d.update(other)` reached
//! through `Stmt::DictUpdate` — the shape the PYTHON FRONTEND produces for the
//! in-place dict merge `d.update(o)`. It runs on the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! The read-only dict-iteration surface (bare `for k in d`, `.keys()`,
//! `.values()`, `.items()`, PMAT-1297..1301) walks a dict's entries WITHOUT
//! mutating it. `d.update(o)` is the first dict-to-dict MUTATION: it walks every
//! entry of the source dict `o` and threads the receiver through
//! `d = $__wasm_dict_update_<k>(d, o)`, whose helper body is the set-algebra
//! walk (PMAT-1247) with the VALUE read (`entry+DICT_VAL_OFFSET`) swapped in for
//! the `0` sentinel and the RECEIVER threaded in place of a fresh allocation.
//! `$__wasm_dict_set_<k>` (the shared update-or-insert dedup) overwrites an
//! existing key in place and appends a new one — exactly Python `update` — and
//! returns the (possibly 2x-grown + relocated) base-pointer, which is written
//! back to the `d` local.
//!
//! Key correctness properties this pins against live `python3`:
//!   * `o`'s value WINS on a shared key (`{1:10,2:20}.update({2:99}) → d[2]==99`).
//!   * new keys of `o` are appended (`d[3]` after merging `{3:30}`).
//!   * a GROW that RELOCATES the receiver (merge 4 entries into a size-1 dict)
//!     keeps every prior key readable through the written-back base-pointer.
//!   * the self-case `d.update(d)` never grows (all keys present) → a no-op that
//!     leaves every value untouched.
//!   * an EMPTY source (`o == {}`) is a no-op.
//!   * str-keyed merge goes through `$__wasm_str_eq` (CONTENT, not pointer) — a
//!     shared string key is overwritten by value, not appended.
//!
//! The result dict is OBSERVED two ways: `len(d)` (cardinality → i64) and
//! `d[k]` (`Expr::DictGet`, the value → i64). Gated on `wasm_runtime_available()`
//! — a clean skip (still asserting the EMIT path lowers + carries the helper)
//! without WABT.
//!
//! Refusals (a dict-literal argument, a set argument, a mismatched key kind) are
//! asserted to fail `emit_module` with an honest message rather than mis-merge.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `<name>: dict[int, int] = {k0: v0, …}` — an int-keyed dict local.
fn idict(name: &str, pairs: &[(i64, i64)]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(
            pairs
                .iter()
                .map(|(k, v)| (Expr::LitInt(*k), Expr::LitInt(*v)))
                .collect(),
        ),
    }
}

/// `<name>: dict[str, int] = {"k0": v0, …}` — a str-keyed dict local.
fn sdict(name: &str, pairs: &[(&str, i64)]) -> Stmt {
    Stmt::Let {
        name: name.into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(
            pairs
                .iter()
                .map(|(k, v)| (Expr::LitStr((*k).into()), Expr::LitInt(*v)))
                .collect(),
        ),
    }
}

/// `<recv>.update(<src>)` — the in-place merge statement.
fn update(recv: &str, src: &str) -> Stmt {
    Stmt::DictUpdate {
        dict_name: recv.into(),
        other: ident(src),
    }
}

/// `len(<name>)` — the merged dict's cardinality (→ i64).
fn dlen(name: &str) -> Expr {
    Expr::Len(Box::new(ident(name)))
}

/// `<name>[key]` — a value read (→ i64); TRAPS on a missing key, so every probe
/// reads a key known to be present post-merge.
fn iget(name: &str, key: i64) -> Expr {
    Expr::DictGet {
        dict: Box::new(ident(name)),
        key: Box::new(Expr::LitInt(key)),
    }
}

fn sget(name: &str, key: &str) -> Expr {
    Expr::DictGet {
        dict: Box::new(ident(name)),
        key: Box::new(Expr::LitStr(key.into())),
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

fn probe_module() -> Module {
    module(
        "dict_update_witness",
        vec![
            // ── int: overwrite + append semantics ────────────────────────────
            // d={1:10,2:20}; o={2:99,3:30}; d.update(o) → {1:10,2:99,3:30}
            func(
                "over_len",
                Type::I64,
                vec![
                    idict("d", &[(1, 10), (2, 20)]),
                    idict("o", &[(2, 99), (3, 30)]),
                    update("d", "o"),
                ],
                dlen("d"),
            ), // 3
            func(
                "over_shared_wins",
                Type::I64,
                vec![
                    idict("d", &[(1, 10), (2, 20)]),
                    idict("o", &[(2, 99), (3, 30)]),
                    update("d", "o"),
                ],
                iget("d", 2),
            ), // 99 — o's value wins on the shared key 2
            func(
                "over_kept",
                Type::I64,
                vec![
                    idict("d", &[(1, 10), (2, 20)]),
                    idict("o", &[(2, 99), (3, 30)]),
                    update("d", "o"),
                ],
                iget("d", 1),
            ), // 10 — d's own key 1 (not in o) is untouched
            func(
                "over_appended",
                Type::I64,
                vec![
                    idict("d", &[(1, 10), (2, 20)]),
                    idict("o", &[(2, 99), (3, 30)]),
                    update("d", "o"),
                ],
                iget("d", 3),
            ), // 30 — o's new key 3 is appended
            // ── int: GROW that RELOCATES the receiver ────────────────────────
            // d starts size 1; merging 4 entries overflows its capacity → the
            // 2x-grow relocates it. Every prior key must survive the write-back.
            func(
                "grow_len",
                Type::I64,
                vec![
                    idict("d", &[(1, 1)]),
                    idict("o", &[(2, 2), (3, 3), (4, 4), (5, 5)]),
                    update("d", "o"),
                ],
                dlen("d"),
            ), // 5
            func(
                "grow_new_key",
                Type::I64,
                vec![
                    idict("d", &[(1, 1)]),
                    idict("o", &[(2, 2), (3, 3), (4, 4), (5, 5)]),
                    update("d", "o"),
                ],
                iget("d", 5),
            ), // 5 — the last-appended key of the grown region
            func(
                "grow_orig_key",
                Type::I64,
                vec![
                    idict("d", &[(1, 1)]),
                    idict("o", &[(2, 2), (3, 3), (4, 4), (5, 5)]),
                    update("d", "o"),
                ],
                iget("d", 1),
            ), // 1 — d's original key survives the relocation copy
            // ── int: self-update (a no-op — all keys already present) ────────
            func(
                "self_len",
                Type::I64,
                vec![idict("d", &[(1, 5), (2, 6), (3, 7)]), update("d", "d")],
                dlen("d"),
            ), // 3
            func(
                "self_val",
                Type::I64,
                vec![idict("d", &[(1, 5), (2, 6), (3, 7)]), update("d", "d")],
                iget("d", 2),
            ), // 6 — d.update(d) leaves every value untouched
            // ── int: empty source (a no-op) ──────────────────────────────────
            func(
                "empty_src_len",
                Type::I64,
                vec![
                    idict("d", &[(1, 10), (2, 20)]),
                    idict("o", &[]),
                    update("d", "o"),
                ],
                dlen("d"),
            ), // 2
            func(
                "empty_src_kept",
                Type::I64,
                vec![
                    idict("d", &[(1, 10), (2, 20)]),
                    idict("o", &[]),
                    update("d", "o"),
                ],
                iget("d", 1),
            ), // 10
            // ── str: CONTENT-compare overwrite ───────────────────────────────
            // d={"a":1,"b":2}; o={"b":9,"c":3}; d.update(o) → {"a":1,"b":9,"c":3}
            func(
                "str_over_len",
                Type::I64,
                vec![
                    sdict("d", &[("a", 1), ("b", 2)]),
                    sdict("o", &[("b", 9), ("c", 3)]),
                    update("d", "o"),
                ],
                dlen("d"),
            ), // 3 — "b" overwritten by content, not appended
            func(
                "str_over_shared_wins",
                Type::I64,
                vec![
                    sdict("d", &[("a", 1), ("b", 2)]),
                    sdict("o", &[("b", 9), ("c", 3)]),
                    update("d", "o"),
                ],
                sget("d", "b"),
            ), // 9
            func(
                "str_over_kept",
                Type::I64,
                vec![
                    sdict("d", &[("a", 1), ("b", 2)]),
                    sdict("o", &[("b", 9), ("c", 3)]),
                    update("d", "o"),
                ],
                sget("d", "a"),
            ), // 1
            func(
                "str_over_appended",
                Type::I64,
                vec![
                    sdict("d", &[("a", 1), ("b", 2)]),
                    sdict("o", &[("b", 9), ("c", 3)]),
                    update("d", "o"),
                ],
                sget("d", "c"),
            ), // 3
        ],
    )
}

/// The CPython-pinned truth for every export (cross-checked in
/// `cpython_pins_are_python`).
const PINS: &[(&str, i64)] = &[
    ("over_len", 3),
    ("over_shared_wins", 99),
    ("over_kept", 10),
    ("over_appended", 30),
    ("grow_len", 5),
    ("grow_new_key", 5),
    ("grow_orig_key", 1),
    ("self_len", 3),
    ("self_val", 6),
    ("empty_src_len", 2),
    ("empty_src_kept", 10),
    ("str_over_len", 3),
    ("str_over_shared_wins", 9),
    ("str_over_kept", 1),
    ("str_over_appended", 3),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal; every pin here is non-negative, so `u64` → `i64` is exact.
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictupdate-{}", std::process::id()));
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
fn dict_update_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the dict-update program must lower through emit_module");
    // Both key kinds present → both update helpers emitted AND called.
    for k in ['i', 's'] {
        assert!(
            wat.contains(&format!("func $__wasm_dict_update_{k}")),
            "missing helper def $__wasm_dict_update_{k}:\n{wat}"
        );
        assert!(
            wat.contains(&format!("call $__wasm_dict_update_{k}")),
            "helper $__wasm_dict_update_{k} defined but never called:\n{wat}"
        );
    }
    // The merge reuses the shared update-or-insert dedup — NO bespoke insert.
    for helper in ["call $__wasm_dict_set_i", "call $__wasm_dict_set_s"] {
        assert!(
            wat.contains(helper),
            "dict update must reuse {helper}:\n{wat}"
        );
    }
    // The helper reads the VALUE slot (a real merge, not a set's 0 sentinel).
    assert!(
        wat.contains(&format!("i64.load offset={}", 8)),
        "the update helper must read the entry value at offset 8:\n{wat}"
    );
    // str-keyed merge compares keys by CONTENT.
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed dict update must carry the content-compare helper:\n{wat}"
    );
}

// ---- honest refusals --------------------------------------------------------

#[test]
fn dict_update_refuses_dict_literal_argument() {
    // d.update({9: 9}) — the argument is a literal, not a dict NAME.
    let m = module(
        "reject_lit",
        vec![func(
            "f",
            Type::I64,
            vec![
                idict("d", &[(1, 1)]),
                Stmt::DictUpdate {
                    dict_name: "d".into(),
                    other: Expr::DictLit(vec![(Expr::LitInt(9), Expr::LitInt(9))]),
                },
            ],
            dlen("d"),
        )],
    );
    let err = emit_module(&m).expect_err("a dict-literal argument must be refused");
    let msg = format!("{err}");
    assert!(
        msg.contains("must be a `dict` NAME"),
        "refusal should name the dict-NAME requirement, got: {msg}"
    );
}

#[test]
fn dict_update_refuses_set_argument() {
    // d.update(s) — a set is not a mapping.
    let m = module(
        "reject_set",
        vec![func(
            "f",
            Type::I64,
            vec![
                idict("d", &[(1, 1)]),
                Stmt::Let {
                    name: "s".into(),
                    ty: Type::Set(Box::new(Type::I64)),
                    mutable: true,
                    value: Expr::SetLit(vec![Expr::LitInt(2), Expr::LitInt(3)]),
                },
                update("d", "s"),
            ],
            dlen("d"),
        )],
    );
    let err = emit_module(&m).expect_err("a set argument must be refused");
    let msg = format!("{err}");
    assert!(
        msg.contains("not a mapping"),
        "refusal should name the set-is-not-a-mapping gap, got: {msg}"
    );
}

#[test]
fn dict_update_refuses_mismatched_key_kind() {
    // int-keyed d, str-keyed o — the WASM subset merges only same-kind dicts.
    let m = module(
        "reject_kind",
        vec![func(
            "f",
            Type::I64,
            vec![
                idict("d", &[(1, 1)]),
                sdict("o", &[("x", 2)]),
                update("d", "o"),
            ],
            dlen("d"),
        )],
    );
    let err = emit_module(&m).expect_err("a mismatched key kind must be refused");
    let msg = format!("{err}");
    assert!(
        msg.contains("key kinds differ"),
        "refusal should name the key-kind mismatch, got: {msg}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_update_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1302: skipping EXECUTED dict-update witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module and carries \
             the $__wasm_dict_update_<k> helper (asserted in \
             `dict_update_lowers_and_carries_helper`); a box with WABT also runs every \
             export and asserts each == the CPython value {PINS:?}. Free CI skips \
             execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1302: running EXECUTED dict-update witness via WABT");
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
        "no dict-update probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1302: EXECUTED dict-update witness PASSED — `d.update(o)` is reachable \
         through the frontend's `Stmt::DictUpdate` shape, merging o into d in place \
         (o's value winning on a shared key, a grow relocating the receiver, the \
         self/empty no-ops, str CONTENT-compare); all {} exports == CPython {PINS:?}.",
        PINS.len()
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    let py = "\
v={}\n\
d={1:10,2:20}; o={2:99,3:30}; d.update(o); v['over_len']=len(d)\n\
d={1:10,2:20}; o={2:99,3:30}; d.update(o); v['over_shared_wins']=d[2]\n\
d={1:10,2:20}; o={2:99,3:30}; d.update(o); v['over_kept']=d[1]\n\
d={1:10,2:20}; o={2:99,3:30}; d.update(o); v['over_appended']=d[3]\n\
d={1:1}; o={2:2,3:3,4:4,5:5}; d.update(o); v['grow_len']=len(d)\n\
d={1:1}; o={2:2,3:3,4:4,5:5}; d.update(o); v['grow_new_key']=d[5]\n\
d={1:1}; o={2:2,3:3,4:4,5:5}; d.update(o); v['grow_orig_key']=d[1]\n\
d={1:5,2:6,3:7}; d.update(d); v['self_len']=len(d)\n\
d={1:5,2:6,3:7}; d.update(d); v['self_val']=d[2]\n\
d={1:10,2:20}; o={}; d.update(o); v['empty_src_len']=len(d)\n\
d={1:10,2:20}; o={}; d.update(o); v['empty_src_kept']=d[1]\n\
d={'a':1,'b':2}; o={'b':9,'c':3}; d.update(o); v['str_over_len']=len(d)\n\
d={'a':1,'b':2}; o={'b':9,'c':3}; d.update(o); v['str_over_shared_wins']=d['b']\n\
d={'a':1,'b':2}; o={'b':9,'c':3}; d.update(o); v['str_over_kept']=d['a']\n\
d={'a':1,'b':2}; o={'b':9,'c':3}; d.update(o); v['str_over_appended']=d['c']\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1302: python3 absent — pins asserted against the WABT witness only");
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
