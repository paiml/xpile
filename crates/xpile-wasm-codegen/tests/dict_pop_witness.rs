//! PMAT-1225 — EXECUTED witness for native-WASM `d.pop(k)` / `d.pop(k, default)`
//! (`Expr::DictPop`) over the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The dict runtime (PMAT-995) shipped `d[k]` (`Expr::DictGet`, TRAPS on an
//! absent key), `k in d` (`Expr::DictContains`), `d[k] = v` (`Stmt::DictSet`),
//! and the total read `d.get(k, default)` (PMAT-1223, `Expr::DictGetOr`). This
//! slice adds `d.pop(k[, default])` — a dict read that ALSO REMOVES the entry.
//! The keyed helper scans for the key, captures its value, swaps the LAST entry
//! into the hole, decrements the count, and returns the value:
//!
//! ```wat
//! ;; $__wasm_dict_pop_<k>(p, key) -> value; removes the entry; traps if absent
//! ;;   found: v = entry.val; memory.copy(entry, last_entry, 16); count--; return v
//! ;;   not found: unreachable   ;; the bare d.pop(k) KeyError analogue
//! ```
//!
//! Removal shrinks in place, so the dict's base pointer NEVER moves (unlike
//! `d[k] = v`, which may 2x-realloc) — no local write-back. The bare `d.pop(k)`
//! traps on an absent key; the 2-arg `d.pop(k, default)` is gated by `has` so it
//! never traps (an absent key falls to `default` WITHOUT mutating) — the same
//! `if has then pop else default` shape as `emit_dict_get_or`, but the present
//! branch REMOVES rather than reads.
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG export returning an `i64`. Value probes pop in tail
//! position (the returned value). Removal-proof probes pop as a BARE STATEMENT
//! (`Stmt::SideEffectCall`, its value dropped — the in-place mutation IS the
//! point) then read the post-pop dict with the known-good `d.get(k, -1)` so the
//! removal + swap-last-into-hole is observable as a single scalar:
//!
//!   * pop-FIRST (index 0): the LAST entry is swapped into the hole — assert the
//!     moved entry AND a bystander both still read back.
//!   * pop-MIDDLE (index 1): same swap, from a middle hole.
//!   * pop-LAST (index n-1): the `memory.copy` is a self-copy (no-op) — assert
//!     the removal still lands and the bystanders survive.
//!   * absent-key-with-default: NO mutation — a bystander is undisturbed.
//!   * double-pop: two removals in a row (the second swaps the survivor down).
//!
//! plus the str-keyed (content-compare) path. Every pin is cross-checked against
//! live `python3` in `cpython_pins_are_python`, each with a FRESH dict (pop
//! mutates), exactly mirroring how each WASM function rebuilds its dict.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the helper) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `d = {1: 10, 2: 20, 3: 30}` — an int-keyed dict local (entries at indices
/// 0/1/2 in insertion order).
fn int_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: false,
        value: Expr::DictLit(vec![
            (Expr::LitInt(1), Expr::LitInt(10)),
            (Expr::LitInt(2), Expr::LitInt(20)),
            (Expr::LitInt(3), Expr::LitInt(30)),
        ]),
    }
}

/// `d = {"x": 100, "y": 200}` — a str-keyed dict local (content-compare path).
fn str_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: false,
        value: Expr::DictLit(vec![
            (Expr::LitStr("x".into()), Expr::LitInt(100)),
            (Expr::LitStr("y".into()), Expr::LitInt(200)),
        ]),
    }
}

/// `d.pop(key)` — the bare, TRAPPING (on absent) form.
fn pop(key: Expr) -> Expr {
    Expr::DictPop {
        dict: Box::new(ident("d")),
        key: Box::new(key),
        default: None,
    }
}

/// `d.pop(key, default)` — the TOTAL form.
fn pop_or(key: Expr, default: Expr) -> Expr {
    Expr::DictPop {
        dict: Box::new(ident("d")),
        key: Box::new(key),
        default: Some(Box::new(default)),
    }
}

/// `d.pop(key)` as a BARE STATEMENT — value dropped, mutation kept
/// (`Stmt::SideEffectCall`, exercises the statement-position pop arm).
fn pop_stmt(key: Expr) -> Stmt {
    Stmt::SideEffectCall { call: pop(key) }
}

/// `d.pop(key, default)` as a BARE STATEMENT.
fn pop_or_stmt(key: Expr, default: Expr) -> Stmt {
    Stmt::SideEffectCall {
        call: pop_or(key, default),
    }
}

/// `d.get(key, -1)` — the known-good total read, used to OBSERVE the post-pop
/// dict state as a single scalar.
fn get_or(key: Expr, default: i64) -> Expr {
    Expr::DictGetOr {
        dict: Box::new(ident("d")),
        key: Box::new(key),
        default: Box::new(Expr::LitInt(default)),
    }
}

fn func(name: &str, stmts: Vec<Stmt>, tail: Expr) -> Item {
    Item::Function(Function {
        name: name.into(),
        params: vec![],
        return_type: Type::I64,
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
        "dict_pop_witness",
        vec![
            // ── value probes: pop in tail (return) position ──────────────────
            // present key → the stored value (and it is removed)
            func("popv", vec![int_dict_let()], pop(Expr::LitInt(2))),
            // present key WITH a default → the stored value (default ignored)
            func(
                "popv_def",
                vec![int_dict_let()],
                pop_or(Expr::LitInt(2), Expr::LitInt(-1)),
            ),
            // absent key WITH a default → the default; must NOT trap
            func(
                "popabs",
                vec![int_dict_let()],
                pop_or(Expr::LitInt(9), Expr::LitInt(-1)),
            ),
            // ── removal-proof probes: pop as a statement, then read ──────────
            // pop present key 2 → re-read 2 == default (removed)
            func(
                "pop_removed",
                vec![int_dict_let(), pop_stmt(Expr::LitInt(2))],
                get_or(Expr::LitInt(2), -1),
            ),
            // pop FIRST key 1 (index 0): last entry (3,30) swaps into the hole
            // → key 3 still reads back (the moved entry survives)
            func(
                "pop_first_moved",
                vec![int_dict_let(), pop_stmt(Expr::LitInt(1))],
                get_or(Expr::LitInt(3), -1),
            ),
            // …and a bystander (key 2) is undisturbed
            func(
                "pop_first_other",
                vec![int_dict_let(), pop_stmt(Expr::LitInt(1))],
                get_or(Expr::LitInt(2), -1),
            ),
            // pop MIDDLE key 2 (index 1): last (3,30) swaps into the hole
            // → key 3 still reads back
            func(
                "pop_mid_moved",
                vec![int_dict_let(), pop_stmt(Expr::LitInt(2))],
                get_or(Expr::LitInt(3), -1),
            ),
            // pop LAST key 3 (index 2): the memory.copy is a self-copy (no-op)
            // → a bystander (key 1) survives
            func(
                "pop_last_self",
                vec![int_dict_let(), pop_stmt(Expr::LitInt(3))],
                get_or(Expr::LitInt(1), -1),
            ),
            // …and the popped last key is gone
            func(
                "pop_last_removed",
                vec![int_dict_let(), pop_stmt(Expr::LitInt(3))],
                get_or(Expr::LitInt(3), -1),
            ),
            // absent-key pop with default → NO mutation: a bystander survives
            func(
                "pop_absent_nomut",
                vec![
                    int_dict_let(),
                    pop_or_stmt(Expr::LitInt(9), Expr::LitInt(-1)),
                ],
                get_or(Expr::LitInt(2), -1),
            ),
            // double pop: pop 1 then 3, then read the survivor 2
            func(
                "pop_two",
                vec![
                    int_dict_let(),
                    pop_stmt(Expr::LitInt(1)),
                    pop_stmt(Expr::LitInt(3)),
                ],
                get_or(Expr::LitInt(2), -1),
            ),
            // …and both popped keys are gone
            func(
                "pop_two_removed",
                vec![
                    int_dict_let(),
                    pop_stmt(Expr::LitInt(1)),
                    pop_stmt(Expr::LitInt(3)),
                ],
                get_or(Expr::LitInt(1), -1),
            ),
            // ── str-keyed (content-compare) path ─────────────────────────────
            func(
                "spopv",
                vec![str_dict_let()],
                pop_or(Expr::LitStr("x".into()), Expr::LitInt(-1)),
            ),
            func(
                "spop_removed",
                vec![str_dict_let(), pop_stmt(Expr::LitStr("x".into()))],
                get_or(Expr::LitStr("x".into()), -1),
            ),
            func(
                "spop_other",
                vec![str_dict_let(), pop_stmt(Expr::LitStr("x".into()))],
                get_or(Expr::LitStr("y".into()), -1),
            ),
            func(
                "spopabs",
                vec![str_dict_let()],
                pop_or(Expr::LitStr("z".into()), Expr::LitInt(-1)),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every probe export.
const PINS: &[(&str, i64)] = &[
    ("popv", 20),
    ("popv_def", 20),
    ("popabs", -1),
    ("pop_removed", -1),
    ("pop_first_moved", 30),
    ("pop_first_other", 20),
    ("pop_mid_moved", 30),
    ("pop_last_self", 10),
    ("pop_last_removed", -1),
    ("pop_absent_nomut", 20),
    ("pop_two", 20),
    ("pop_two_removed", -1),
    ("spopv", 100),
    ("spop_removed", -1),
    ("spop_other", 200),
    ("spopabs", -1),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => i64:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal, so a negative `i64` renders as its `u64` two's-complement value —
/// parse as `u64` and reinterpret.
fn parse_scalar_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    let val = line
        .rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim();
    val.parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-dictpop-{}-{}", tag, std::process::id()));
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
fn dict_pop_lowers_and_carries_helper() {
    let wat = emit_module(&probe_module())
        .expect("the `d.pop(k[, default])` program must lower through emit_module");
    // The removing helper is emitted for BOTH key kinds.
    for helper in ["$__wasm_dict_pop_i", "$__wasm_dict_pop_s"] {
        assert!(wat.contains(helper), "missing helper {helper}:\n{wat}");
    }
    // Removal is swap-last-into-hole: the helper uses memory.copy.
    assert!(
        wat.contains("memory.copy"),
        "the pop helper must move the last entry into the hole via memory.copy:\n{wat}"
    );
    // The 2-arg total form gates the mutating pop with `has` under `if (result
    // i64)` — NOT a bare unconditional pop.
    assert!(
        wat.contains("call $__wasm_dict_has_i")
            && wat.contains("if (result i64)")
            && wat.contains("call $__wasm_dict_pop_i"),
        "the total `.pop(k, default)` must be `if has(...) then pop(...) else default`:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_pop_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1225: skipping EXECUTED `d.pop(k[, default])` witness — WABT \
             (wat2wasm / wasm-interp) absent. The program lowered through \
             emit_module (asserted in `dict_pop_lowers_and_carries_helper`); a box \
             with WABT also runs every export and asserts each == the CPython \
             value {PINS:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1225: running EXECUTED `d.pop(k[, default])` witness via WABT");
    let (stdout, ok) = assemble_and_run("ok", &wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}\n---WAT---\n{wat}");

    for &(name, expected) in PINS {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, expected,
            "executed WASM {name}() = {got} but CPython = {expected}\n\
             full interp output:\n{stdout}"
        );
    }
    // No probe uses the bare `d.pop(absent)` form, so nothing must trap: every
    // absent key is popped WITH a default (falls through, never `unreachable`).
    assert!(
        !stdout.contains("unreachable executed"),
        "no pop probe should trap (absent keys all carry a default):\n{stdout}"
    );

    eprintln!(
        "PMAT-1225: EXECUTED `d.pop(k[, default])` witness PASSED — pop returned \
         the stored value and REMOVED the entry (re-read == default), \
         swap-last-into-hole kept every bystander + moved entry readable across \
         first/middle/last/double pops, absent-with-default did not mutate, and \
         the str-keyed path matched. All value-match CPython {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Each probe rebuilds its dict; pop mutates, so mirror that with a fresh
    // dict per line (exactly what each WASM function does).
    let py = "\
def f(): return {1:10,2:20,3:30}\n\
def s(): return {'x':100,'y':200}\n\
v={}\n\
d=f(); v['popv']=d.pop(2)\n\
d=f(); v['popv_def']=d.pop(2,-1)\n\
d=f(); v['popabs']=d.pop(9,-1)\n\
d=f(); d.pop(2); v['pop_removed']=d.get(2,-1)\n\
d=f(); d.pop(1); v['pop_first_moved']=d.get(3,-1)\n\
d=f(); d.pop(1); v['pop_first_other']=d.get(2,-1)\n\
d=f(); d.pop(2); v['pop_mid_moved']=d.get(3,-1)\n\
d=f(); d.pop(3); v['pop_last_self']=d.get(1,-1)\n\
d=f(); d.pop(3); v['pop_last_removed']=d.get(3,-1)\n\
d=f(); d.pop(9,-1); v['pop_absent_nomut']=d.get(2,-1)\n\
d=f(); d.pop(1); d.pop(3); v['pop_two']=d.get(2,-1)\n\
d=f(); d.pop(1); d.pop(3); v['pop_two_removed']=d.get(1,-1)\n\
d=s(); v['spopv']=d.pop('x',-1)\n\
d=s(); d.pop('x'); v['spop_removed']=d.get('x',-1)\n\
d=s(); d.pop('x'); v['spop_other']=d.get('y',-1)\n\
d=s(); v['spopabs']=d.pop('z',-1)\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1225: python3 absent — pins asserted against the WABT witness only");
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
