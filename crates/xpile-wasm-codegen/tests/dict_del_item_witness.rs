//! PMAT-1234 — EXECUTED witness for native-WASM `del d[k]` (`Stmt::DelItem`,
//! `is_dict`) over the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` +
//! `C-WASM-HEAP`).
//!
//! The dict runtime already shipped `d[k]` (`Expr::DictGet`, TRAPS on absent),
//! `k in d` (`Expr::DictContains`), `d[k] = v` (`Stmt::DictSet`), the total read
//! `d.get(k, default)` (PMAT-1223, `Expr::DictGetOr`), the removing-EXPRESSION
//! `d.pop(k[, default])` (PMAT-1225, `Expr::DictPop`), and the get-or-INSERT
//! `d.setdefault(k, default)` (PMAT-1227, `Expr::DictSetDefault`). This slice
//! adds `del d[k]` — dict entry removal in STATEMENT position.
//!
//! `del d[k]` is exactly the bare `d.pop(k)` with the returned value discarded:
//! it reuses the SAME removal helper and drops the i64 value.
//!
//! ```wat
//! ;; del d[k]:
//! ;;   call $__wasm_dict_pop_<k>   ;; swap-last-into-hole + count-- (in place)
//! ;;   drop                        ;; the removed value nobody asked for
//! ;; the helper's not-found tail is `unreachable` == CPython del d[missing] KeyError
//! ```
//!
//! Removal shrinks the region IN PLACE (swap-last-into-hole), so — unlike
//! `d[k] = v` / `d.setdefault` on a miss — the base pointer NEVER moves and there
//! is NO local write-back. Deleting a MIDDLE entry moves the last entry into the
//! hole (a real `memory.copy`); deleting the LAST-indexed entry is a no-op copy
//! (`$ea == $last`); both then drop the entry via count--.
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG export returning an `i64`. `del d[k]` runs as a bare
//! statement, then the post-state is OBSERVED with the known-good total read
//! `d.get(k, -1)` and `len(d)`:
//!
//!   * delete-middle → the key is gone (`get == -1`), a bystander survives, AND
//!     the swapped-in last entry is still readable (proof the swap-into-hole kept
//!     it in the scan range).
//!   * delete-last-indexed → the `$ea == $last` no-op-copy branch; key gone,
//!     bystander survives.
//!   * delete-first → the head entry goes.
//!   * `len(d)` decrements by exactly one per delete; delete-all → 0.
//!   * delete-then-reinsert → `del d[k]; d[k] = v'` reads back the NEW value.
//!   * loop-delete → `while i <= 3: del d[i]` deletes keys 1..3 (exercises the
//!     DelItem arm reached THROUGH a `While` body); survivors + count hold.
//!   * str-keyed (content-compare) delete: gone / bystander / swapped / len /
//!     reinsert.
//!
//! A SEPARATE trap witness runs `del d[absent]` and asserts the export traps
//! (`unreachable executed`) — the CPython `del d[missing]` → KeyError analogue,
//! the one behaviour `d.pop(k, default)` (which never traps) could not show.
//!
//! Every value pin is cross-checked against live `python3` in
//! `cpython_pins_are_python`, each with a FRESH dict (delete mutates). Gated on
//! `wasm_runtime_available()` — a clean skip (still asserting the EMIT path
//! lowers + carries the shape) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `d = {1: 10, 2: 20, 3: 30}` — an int-keyed dict local (cap 3 + slack).
fn int_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![
            (Expr::LitInt(1), Expr::LitInt(10)),
            (Expr::LitInt(2), Expr::LitInt(20)),
            (Expr::LitInt(3), Expr::LitInt(30)),
        ]),
    }
}

/// `d = {1:10, 2:20, 3:30, 4:40, 5:50, 6:60}` — the 6-entry dict for the
/// loop-delete probe.
fn int_dict6_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(
            (1..=6i64)
                .map(|k| (Expr::LitInt(k), Expr::LitInt(k * 10)))
                .collect(),
        ),
    }
}

/// `d = {"x": 100, "y": 200, "z": 300}` — a str-keyed dict local (content-compare
/// removal path).
fn str_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![
            (Expr::LitStr("x".into()), Expr::LitInt(100)),
            (Expr::LitStr("y".into()), Expr::LitInt(200)),
            (Expr::LitStr("z".into()), Expr::LitInt(300)),
        ]),
    }
}

/// `del d[key]` — the entry-removal statement (`Stmt::DelItem`, dict form).
fn del(key: Expr) -> Stmt {
    Stmt::DelItem {
        name: "d".into(),
        key,
        is_dict: true,
    }
}

/// `d[key] = value` — reinsert after a delete.
fn dict_set(key: Expr, value: Expr) -> Stmt {
    Stmt::DictSet {
        dict_name: "d".into(),
        key,
        value,
    }
}

/// `d.get(key, -1)` — the known-good total read, OBSERVES the post-delete state.
fn get_or(key: Expr, default: i64) -> Expr {
    Expr::DictGetOr {
        dict: Box::new(ident("d")),
        key: Box::new(key),
        default: Box::new(Expr::LitInt(default)),
    }
}

/// `len(d)` — the entry count (decrements by one per delete).
fn dlen() -> Expr {
    Expr::Len(Box::new(ident("d")))
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

/// `i = 1; while i <= 3: del d[i]; i = i + 1` — deletes keys 1,2,3 from `d6`
/// through a `While` body (exercises the DelItem gate-walkers' While recursion).
fn loop_delete_stmts() -> Vec<Stmt> {
    vec![
        int_dict6_let(),
        Stmt::Let {
            name: "i".into(),
            ty: Type::I64,
            mutable: true,
            value: Expr::LitInt(1),
        },
        Stmt::While {
            cond: Expr::BinOp {
                op: BinOp::LtEq,
                lhs: Box::new(ident("i")),
                rhs: Box::new(Expr::LitInt(3)),
            },
            body: vec![
                del(ident("i")),
                Stmt::Assign {
                    name: "i".into(),
                    value: Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(ident("i")),
                        rhs: Box::new(Expr::LitInt(1)),
                    },
                },
            ],
        },
    ]
}

fn probe_module() -> Module {
    module(
        "dict_del_item_witness",
        vec![
            // ── delete a MIDDLE entry (key 2): swap-last-into-hole ───────────
            // the deleted key is gone
            func(
                "del_mid_gone",
                vec![int_dict_let(), del(Expr::LitInt(2))],
                get_or(Expr::LitInt(2), -1),
            ),
            // a bystander (key 1) survives
            func(
                "del_mid_bystander",
                vec![int_dict_let(), del(Expr::LitInt(2))],
                get_or(Expr::LitInt(1), -1),
            ),
            // the LAST entry (key 3) was swapped into the hole — still readable
            func(
                "del_mid_swapped",
                vec![int_dict_let(), del(Expr::LitInt(2))],
                get_or(Expr::LitInt(3), -1),
            ),
            // count decrements by one
            func(
                "del_mid_len",
                vec![int_dict_let(), del(Expr::LitInt(2))],
                dlen(),
            ),
            // ── delete the LAST-indexed entry (key 3): no-op copy branch ─────
            func(
                "del_last_gone",
                vec![int_dict_let(), del(Expr::LitInt(3))],
                get_or(Expr::LitInt(3), -1),
            ),
            func(
                "del_last_bystander",
                vec![int_dict_let(), del(Expr::LitInt(3))],
                get_or(Expr::LitInt(1), -1),
            ),
            func(
                "del_last_len",
                vec![int_dict_let(), del(Expr::LitInt(3))],
                dlen(),
            ),
            // ── delete the FIRST entry (key 1) ───────────────────────────────
            func(
                "del_first_gone",
                vec![int_dict_let(), del(Expr::LitInt(1))],
                get_or(Expr::LitInt(1), -1),
            ),
            func(
                "del_first_bystander",
                vec![int_dict_let(), del(Expr::LitInt(1))],
                get_or(Expr::LitInt(2), -1),
            ),
            // ── delete then REINSERT: read back the new value ────────────────
            func(
                "del_reinsert",
                vec![
                    int_dict_let(),
                    del(Expr::LitInt(2)),
                    dict_set(Expr::LitInt(2), Expr::LitInt(99)),
                ],
                get_or(Expr::LitInt(2), -1),
            ),
            // ── delete ALL: count → 0 ────────────────────────────────────────
            func(
                "del_all_len",
                vec![
                    int_dict_let(),
                    del(Expr::LitInt(1)),
                    del(Expr::LitInt(2)),
                    del(Expr::LitInt(3)),
                ],
                dlen(),
            ),
            // ── loop-delete (DelItem reached through a `While` body) ──────────
            func("del_loop_len", loop_delete_stmts(), dlen()), // 6 - 3 == 3
            func(
                "del_loop_survivor",
                loop_delete_stmts(),
                get_or(Expr::LitInt(6), -1),
            ), // 60
            func(
                "del_loop_gone",
                loop_delete_stmts(),
                get_or(Expr::LitInt(2), -1),
            ), // -1
            // ── str-keyed (content-compare) removal ──────────────────────────
            func(
                "del_s_gone",
                vec![str_dict_let(), del(Expr::LitStr("x".into()))],
                get_or(Expr::LitStr("x".into()), -1),
            ),
            func(
                "del_s_bystander",
                vec![str_dict_let(), del(Expr::LitStr("x".into()))],
                get_or(Expr::LitStr("y".into()), -1),
            ),
            func(
                "del_s_swapped",
                vec![str_dict_let(), del(Expr::LitStr("x".into()))],
                get_or(Expr::LitStr("z".into()), -1),
            ),
            func(
                "del_s_len",
                vec![str_dict_let(), del(Expr::LitStr("x".into()))],
                dlen(),
            ),
            func(
                "del_s_reinsert",
                vec![
                    str_dict_let(),
                    del(Expr::LitStr("x".into())),
                    dict_set(Expr::LitStr("x".into()), Expr::LitInt(7)),
                ],
                get_or(Expr::LitStr("x".into()), -1),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every value probe.
const PINS: &[(&str, i64)] = &[
    ("del_mid_gone", -1),
    ("del_mid_bystander", 10),
    ("del_mid_swapped", 30),
    ("del_mid_len", 2),
    ("del_last_gone", -1),
    ("del_last_bystander", 10),
    ("del_last_len", 2),
    ("del_first_gone", -1),
    ("del_first_bystander", 20),
    ("del_reinsert", 99),
    ("del_all_len", 0),
    ("del_loop_len", 3),
    ("del_loop_survivor", 60),
    ("del_loop_gone", -1),
    ("del_s_gone", -1),
    ("del_s_bystander", 200),
    ("del_s_swapped", 300),
    ("del_s_len", 2),
    ("del_s_reinsert", 7),
];

/// The KeyError-trap probes: `del d[absent]` must trap (`unreachable`). Isolated
/// so `--run-all-exports` reports the trap on its own line.
fn trap_module() -> Module {
    module(
        "dict_del_item_trap_witness",
        vec![
            // del d[9] on {1,2,3} → KeyError → unreachable
            func(
                "del_miss",
                vec![int_dict_let(), del(Expr::LitInt(9))],
                get_or(Expr::LitInt(1), -1),
            ),
            // del d["q"] on {"x","y","z"} → KeyError → unreachable
            func(
                "del_s_miss",
                vec![str_dict_let(), del(Expr::LitStr("q".into()))],
                get_or(Expr::LitStr("y".into()), -1),
            ),
        ],
    )
}

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
        std::env::temp_dir().join(format!("xpile-wasm-dictdel-{}-{}", tag, std::process::id()));
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
fn dict_del_item_lowers_and_carries_shape() {
    let wat = emit_module(&probe_module())
        .expect("the `del d[k]` program must lower through emit_module");
    // `del` reuses the SHARED pop helper for both key kinds — NO bespoke
    // `$__wasm_dict_del_*` helper.
    assert!(
        !wat.contains("$__wasm_dict_del"),
        "del must NOT declare a bespoke helper — it reuses the pop helper:\n{wat}"
    );
    // int-keyed AND str-keyed dicts are present, so both pop helpers exist and
    // are called by the del sites.
    for helper in ["call $__wasm_dict_pop_i", "call $__wasm_dict_pop_s"] {
        assert!(wat.contains(helper), "missing pop call {helper}:\n{wat}");
    }
    // The pop helper carries the swap-last-into-hole `memory.copy` + count-- and
    // the KeyError not-found trap.
    assert!(
        wat.contains("memory.copy") && wat.contains("unreachable"),
        "the pop helper must carry the swap-into-hole copy + the KeyError trap:\n{wat}"
    );
    // A `del` statement DROPS the removed value (it is a statement, not an expr).
    assert!(
        wat.contains("drop"),
        "a `del d[k]` statement must drop the pop helper's returned value:\n{wat}"
    );
    // Removal never grows, so no del site writes a base pointer back to the dict
    // local — but the shared set helper (used by the reinsert probes) still may.
    // (No stronger assertion here; the write-back-absence is proven by execution.)
}

#[test]
fn dict_del_list_form_now_lowers() {
    // PMAT-1284: `del xs[i]` (list-element deletion, `is_dict == false`) over a
    // `list[int]`/`list[float]` is now SUPPORTED — the shrink-and-shift mirror of
    // `insert` — so the list form lowers to a `$__wasm_list_delitem` call rather
    // than the old honest refusal. (The dedicated executed differential coverage
    // lives in `list_delitem_witness.rs`.)
    let m = module(
        "del_list",
        vec![func(
            "f",
            vec![
                Stmt::Let {
                    name: "xs".into(),
                    ty: Type::List(Box::new(Type::I64)),
                    mutable: true,
                    value: Expr::ListLit(vec![Expr::LitInt(1), Expr::LitInt(2)]),
                },
                Stmt::DelItem {
                    name: "xs".into(),
                    key: Expr::LitInt(0),
                    is_dict: false,
                },
            ],
            Expr::LitInt(0),
        )],
    );
    let wat = emit_module(&m).expect("`del xs[i]` (list[int]) now lowers through the WASM lane");
    assert!(
        wat.contains("call $__wasm_list_delitem"),
        "the list `del xs[i]` form must call the delete-at-index helper:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_del_item_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1234: skipping EXECUTED `del d[k]` witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module (asserted \
             in `dict_del_item_lowers_and_carries_shape`); a box with WABT also runs \
             every export and asserts each == the CPython value {PINS:?}. Free CI \
             skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1234: running EXECUTED `del d[k]` witness via WABT");
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
    // Every value probe deletes a PRESENT key, so none traps.
    assert!(
        !stdout.contains("unreachable executed"),
        "no value probe should trap (every deleted key is present):\n{stdout}"
    );

    eprintln!(
        "PMAT-1234: EXECUTED `del d[k]` witness PASSED — middle/last/first deletes \
         removed the key, bystanders + the swapped-in last entry survived, len \
         decremented by one per delete (all-delete → 0), delete-then-reinsert read \
         the new value, a loop-delete through a While body removed keys 1..3, and \
         the str-keyed path matched. All value-match CPython {PINS:?}."
    );
}

#[test]
fn dict_del_item_absent_key_traps_keyerror() {
    let wat = emit_module(&trap_module()).expect("trap program lowers through emit_module");
    // The KeyError trap is present in the emitted module either way.
    assert!(
        wat.contains("unreachable"),
        "the pop helper's not-found tail must be `unreachable` (KeyError analogue):\n{wat}"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1234: skipping EXECUTED `del d[absent]` trap witness — WABT absent. \
             The emitted module carries the `unreachable` KeyError tail (asserted \
             above); a box with WABT confirms the export traps at runtime."
        );
        return;
    }

    let (stdout, _ok) = assemble_and_run("trap", &wat);
    for name in ["del_miss", "del_s_miss"] {
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("{name}()")))
            .unwrap_or_else(|| panic!("no `{name}` line in interp output:\n{stdout}"));
        assert!(
            line.contains("unreachable executed"),
            "`del d[absent]` ({name}) must trap (KeyError analogue), got: {line:?}\n\
             full output:\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1234: `del d[absent]` trap witness PASSED — both an int-keyed and a \
         str-keyed missing-key delete trapped (unreachable == CPython KeyError)."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Each probe rebuilds its dict; delete mutates, so mirror that with a fresh
    // dict per line (exactly what each WASM function does).
    let py = "\
def f(): return {1:10,2:20,3:30}\n\
def f6(): return {k:k*10 for k in range(1,7)}\n\
def s(): return {'x':100,'y':200,'z':300}\n\
def loop():\n\
\td=f6()\n\
\ti=1\n\
\twhile i<=3:\n\
\t\tdel d[i]; i+=1\n\
\treturn d\n\
v={}\n\
d=f(); del d[2]; v['del_mid_gone']=d.get(2,-1)\n\
d=f(); del d[2]; v['del_mid_bystander']=d.get(1,-1)\n\
d=f(); del d[2]; v['del_mid_swapped']=d.get(3,-1)\n\
d=f(); del d[2]; v['del_mid_len']=len(d)\n\
d=f(); del d[3]; v['del_last_gone']=d.get(3,-1)\n\
d=f(); del d[3]; v['del_last_bystander']=d.get(1,-1)\n\
d=f(); del d[3]; v['del_last_len']=len(d)\n\
d=f(); del d[1]; v['del_first_gone']=d.get(1,-1)\n\
d=f(); del d[1]; v['del_first_bystander']=d.get(2,-1)\n\
d=f(); del d[2]; d[2]=99; v['del_reinsert']=d.get(2,-1)\n\
d=f(); del d[1]; del d[2]; del d[3]; v['del_all_len']=len(d)\n\
d=loop(); v['del_loop_len']=len(d)\n\
d=loop(); v['del_loop_survivor']=d.get(6,-1)\n\
d=loop(); v['del_loop_gone']=d.get(2,-1)\n\
d=s(); del d['x']; v['del_s_gone']=d.get('x',-1)\n\
d=s(); del d['x']; v['del_s_bystander']=d.get('y',-1)\n\
d=s(); del d['x']; v['del_s_swapped']=d.get('z',-1)\n\
d=s(); del d['x']; v['del_s_len']=len(d)\n\
d=s(); del d['x']; d['x']=7; v['del_s_reinsert']=d.get('x',-1)\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1234: python3 absent — pins asserted against the WABT witness only");
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
