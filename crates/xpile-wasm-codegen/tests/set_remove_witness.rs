//! PMAT-1240 — EXECUTED witness for native-WASM `s.remove(e)` / `s.discard(e)`
//! (`Stmt::SetRemove`) over the bump-heap set runtime (`C-COMPILE-RUST-TO-WASM`
//! + `C-WASM-HEAP`).
//!
//! A set is a keys-only dict (16-byte entries, `key` @ entry+0, a dummy value @
//! entry+8), so element removal reuses the SAME swap-last-into-hole helper
//! `del d[k]` (PMAT-1234, `Stmt::DelItem`) and `d.pop(k)` (PMAT-1225) use —
//! `$__wasm_dict_pop_<k>` (count--, base pointer never moves) — with the popped
//! dummy value dropped. NO bespoke set-removal helper is minted.
//!
//! The two Python builtins differ ONLY on an absent element:
//!
//! ```wat
//! ;; s.remove(e):                        ;; s.discard(e):
//! ;;   call $__wasm_dict_pop_<k>         ;;   call $__wasm_dict_has_<k>
//! ;;   drop                             ;;   if
//! ;; not-found tail = `unreachable`     ;;     call $__wasm_dict_pop_<k>
//! ;;   == CPython KeyError              ;;     drop
//! ;;                                    ;;   end   ;; absent = silent no-op
//! ```
//!
//! `remove` lets the pop helper's not-found tail TRAP (`unreachable`) — CPython
//! `set.remove(missing)` raises `KeyError`. `discard` GATES the pop behind the
//! never-trapping `$__wasm_dict_has_<k>`, so an absent element is a no-op —
//! CPython `set.discard(missing)` returns `None`. Both mutate IN PLACE (the
//! region only shrinks, so no local write-back — unlike `s.add`, which can grow).
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG export. The post-removal state is OBSERVED with the
//! two total set reads `x in s` (`Expr::SetContains` → i32 0/1) and `len(s)`
//! (`Expr::Len` → i64):
//!
//!   * remove-middle → the element is gone (`in` == 0), a bystander survives, AND
//!     the swapped-in last element is still a member (proof the swap-into-hole
//!     kept it in the scan range);
//!   * remove-last / remove-first cover the `$ea == $last` no-op-copy and head
//!     branches;
//!   * `len(s)` decrements by exactly one per remove; remove-all → 0;
//!   * discard-present behaves like remove; discard-ABSENT is a no-op (len holds,
//!     bystander survives) — the behaviour `remove` cannot show without trapping;
//!   * discard-then-re-add reads the element back as a member;
//!   * loop-discard → `while i <= 3: s.discard(i)` through a `While` body;
//!   * str-keyed (content-compare via `$__wasm_str_eq`): gone / bystander /
//!     swapped / len, plus a nested-HEAP discard (`s.discard(a + "b")`) that
//!     exercises the SetRemove gate-walkers recursing into an ALLOCATING element.
//!
//! A SEPARATE trap witness runs `s.remove(absent)` (int + str) and asserts the
//! export traps (`unreachable executed`) — the CPython `KeyError` analogue that
//! `discard` never shows.
//!
//! Every value pin is cross-checked against live `python3` in
//! `cpython_pins_are_python`, each with a FRESH set (removal mutates). Gated on
//! `wasm_runtime_available()` — a clean skip (still asserting the EMIT path
//! lowers + carries the shape) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{BinOp, Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `s = {1, 2, 3}` — an int-elem set local (cap 3 + slack).
fn int_set_let() -> Stmt {
    Stmt::Let {
        name: "s".into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: true,
        value: Expr::SetLit(vec![Expr::LitInt(1), Expr::LitInt(2), Expr::LitInt(3)]),
    }
}

/// `s = {1, 2, 3, 4, 5, 6}` — the 6-elem set for the loop-discard probe.
fn int_set6_let() -> Stmt {
    Stmt::Let {
        name: "s".into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: true,
        value: Expr::SetLit((1..=6i64).map(Expr::LitInt).collect()),
    }
}

/// `s = {"a", "bb", "ccc"}` — a str-elem set local (content-compare path).
fn str_set_let() -> Stmt {
    Stmt::Let {
        name: "s".into(),
        ty: Type::Set(Box::new(Type::Str)),
        mutable: true,
        value: Expr::SetLit(vec![
            Expr::LitStr("a".into()),
            Expr::LitStr("bb".into()),
            Expr::LitStr("ccc".into()),
        ]),
    }
}

/// `s.remove(e)` — element removal that TRAPS on absent (KeyError).
fn remove(elem: Expr) -> Stmt {
    Stmt::SetRemove {
        set_name: "s".into(),
        elem,
        error_if_absent: true,
    }
}

/// `s.discard(e)` — element removal that is a silent no-op on absent.
fn discard(elem: Expr) -> Stmt {
    Stmt::SetRemove {
        set_name: "s".into(),
        elem,
        error_if_absent: false,
    }
}

/// `s.add(e)` — re-add after a removal.
fn add(elem: Expr) -> Stmt {
    Stmt::SetAdd {
        set_name: "s".into(),
        elem,
    }
}

/// `e in s` — a total membership read (`Expr::SetContains` → i32 0/1). OBSERVES
/// the post-removal state.
fn member(elem: Expr) -> Expr {
    Expr::SetContains {
        set: Box::new(ident("s")),
        elem: Box::new(elem),
    }
}

/// `len(s)` — the element count (decrements by one per removal).
fn slen() -> Expr {
    Expr::Len(Box::new(ident("s")))
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

/// `i = 1; while i <= 3: s.discard(i); i = i + 1` — discards 1,2,3 from `s6`
/// through a `While` body (exercises the SetRemove gate-walkers' While recursion).
fn loop_discard_stmts() -> Vec<Stmt> {
    vec![
        int_set6_let(),
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
                discard(ident("i")),
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

/// `s = {"ab", "yo"}; a = "a"; s.discard(a + "b")` — the nested-HEAP discard.
/// `a + "b"` is an ALLOCATING `Expr::Concat`, so every SetRemove gate-walker must
/// recurse into `elem`; a miss would call `$__wasm_str_eq` / the concat path with
/// an undeclared helper and wat2wasm would fail to assemble.
fn nested_heap_discard_stmts() -> Vec<Stmt> {
    vec![
        Stmt::Let {
            name: "s".into(),
            ty: Type::Set(Box::new(Type::Str)),
            mutable: true,
            value: Expr::SetLit(vec![Expr::LitStr("ab".into()), Expr::LitStr("yo".into())]),
        },
        Stmt::Let {
            name: "a".into(),
            ty: Type::Str,
            mutable: false,
            value: Expr::LitStr("a".into()),
        },
        discard(Expr::Concat {
            lhs: Box::new(ident("a")),
            rhs: Box::new(Expr::LitStr("b".into())),
        }),
    ]
}

fn probe_module() -> Module {
    module(
        "set_remove_witness",
        vec![
            // ── remove a MIDDLE element (2): swap-last-into-hole ──────────────
            func(
                "rm_mid_gone",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(2))],
                member(Expr::LitInt(2)),
            ),
            func(
                "rm_mid_bystander",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(2))],
                member(Expr::LitInt(1)),
            ),
            // the LAST element (3) was swapped into the hole — still a member
            func(
                "rm_mid_swapped",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(2))],
                member(Expr::LitInt(3)),
            ),
            func(
                "rm_mid_len",
                Type::I64,
                vec![int_set_let(), remove(Expr::LitInt(2))],
                slen(),
            ),
            // ── remove the LAST-indexed element (3): no-op copy branch ────────
            func(
                "rm_last_gone",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(3))],
                member(Expr::LitInt(3)),
            ),
            func(
                "rm_last_bystander",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(3))],
                member(Expr::LitInt(1)),
            ),
            func(
                "rm_last_len",
                Type::I64,
                vec![int_set_let(), remove(Expr::LitInt(3))],
                slen(),
            ),
            // ── remove the FIRST element (1) ──────────────────────────────────
            func(
                "rm_first_gone",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(1))],
                member(Expr::LitInt(1)),
            ),
            func(
                "rm_first_bystander",
                Type::Bool,
                vec![int_set_let(), remove(Expr::LitInt(1))],
                member(Expr::LitInt(2)),
            ),
            // ── remove ALL: count → 0 ─────────────────────────────────────────
            func(
                "rm_all_len",
                Type::I64,
                vec![
                    int_set_let(),
                    remove(Expr::LitInt(1)),
                    remove(Expr::LitInt(2)),
                    remove(Expr::LitInt(3)),
                ],
                slen(),
            ),
            // ── discard PRESENT (2): behaves like remove ──────────────────────
            func(
                "dc_present_gone",
                Type::Bool,
                vec![int_set_let(), discard(Expr::LitInt(2))],
                member(Expr::LitInt(2)),
            ),
            func(
                "dc_present_len",
                Type::I64,
                vec![int_set_let(), discard(Expr::LitInt(2))],
                slen(),
            ),
            // ── discard ABSENT (99): a silent no-op (len holds, bystander lives)
            func(
                "dc_absent_len",
                Type::I64,
                vec![int_set_let(), discard(Expr::LitInt(99))],
                slen(),
            ),
            func(
                "dc_absent_bystander",
                Type::Bool,
                vec![int_set_let(), discard(Expr::LitInt(99))],
                member(Expr::LitInt(1)),
            ),
            // ── discard then RE-ADD: the element is a member again ────────────
            func(
                "dc_readd",
                Type::Bool,
                vec![
                    int_set_let(),
                    discard(Expr::LitInt(2)),
                    add(Expr::LitInt(2)),
                ],
                member(Expr::LitInt(2)),
            ),
            // ── loop-discard (SetRemove reached through a `While` body) ────────
            func("dc_loop_len", Type::I64, loop_discard_stmts(), slen()), // 6 - 3 == 3
            func(
                "dc_loop_survivor",
                Type::Bool,
                loop_discard_stmts(),
                member(Expr::LitInt(6)),
            ), // 1
            func(
                "dc_loop_gone",
                Type::Bool,
                loop_discard_stmts(),
                member(Expr::LitInt(2)),
            ), // 0
            // ── str-keyed (content-compare) removal ───────────────────────────
            func(
                "rm_s_gone",
                Type::Bool,
                vec![str_set_let(), remove(Expr::LitStr("a".into()))],
                member(Expr::LitStr("a".into())),
            ),
            func(
                "rm_s_bystander",
                Type::Bool,
                vec![str_set_let(), remove(Expr::LitStr("a".into()))],
                member(Expr::LitStr("bb".into())),
            ),
            func(
                "rm_s_swapped",
                Type::Bool,
                vec![str_set_let(), remove(Expr::LitStr("a".into()))],
                member(Expr::LitStr("ccc".into())),
            ),
            func(
                "rm_s_len",
                Type::I64,
                vec![str_set_let(), remove(Expr::LitStr("a".into()))],
                slen(),
            ),
            func(
                "dc_s_gone",
                Type::Bool,
                vec![str_set_let(), discard(Expr::LitStr("bb".into()))],
                member(Expr::LitStr("bb".into())),
            ),
            func(
                "dc_s_len",
                Type::I64,
                vec![str_set_let(), discard(Expr::LitStr("bb".into()))],
                slen(),
            ),
            func(
                "dc_s_absent_len",
                Type::I64,
                vec![str_set_let(), discard(Expr::LitStr("zz".into()))],
                slen(),
            ),
            // ── nested-HEAP discard (walker-recursion gate-hole guard) ────────
            // s.discard(a + "b") removes "ab" from {"ab","yo"} → len 1.
            func(
                "dc_nested_heap_len",
                Type::I64,
                nested_heap_discard_stmts(),
                slen(),
            ),
            func(
                "dc_nested_heap_gone",
                Type::Bool,
                nested_heap_discard_stmts(),
                member(Expr::LitStr("ab".into())),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every value probe (`in` → 0/1,
/// `len` → count).
const PINS: &[(&str, i64)] = &[
    ("rm_mid_gone", 0),
    ("rm_mid_bystander", 1),
    ("rm_mid_swapped", 1),
    ("rm_mid_len", 2),
    ("rm_last_gone", 0),
    ("rm_last_bystander", 1),
    ("rm_last_len", 2),
    ("rm_first_gone", 0),
    ("rm_first_bystander", 1),
    ("rm_all_len", 0),
    ("dc_present_gone", 0),
    ("dc_present_len", 2),
    ("dc_absent_len", 3),
    ("dc_absent_bystander", 1),
    ("dc_readd", 1),
    ("dc_loop_len", 3),
    ("dc_loop_survivor", 1),
    ("dc_loop_gone", 0),
    ("rm_s_gone", 0),
    ("rm_s_bystander", 1),
    ("rm_s_swapped", 1),
    ("rm_s_len", 2),
    ("dc_s_gone", 0),
    ("dc_s_len", 2),
    ("dc_s_absent_len", 3),
    ("dc_nested_heap_len", 1),
    ("dc_nested_heap_gone", 0),
];

/// The KeyError-trap probes: `s.remove(absent)` must trap (`unreachable`).
/// Isolated so `--run-all-exports` reports the trap on its own line.
fn trap_module() -> Module {
    module(
        "set_remove_trap_witness",
        vec![
            // s.remove(9) on {1,2,3} → KeyError → unreachable
            func(
                "rm_miss",
                Type::I64,
                vec![int_set_let(), remove(Expr::LitInt(9))],
                slen(),
            ),
            // s.remove("q") on {"a","bb","ccc"} → KeyError → unreachable
            func(
                "rm_s_miss",
                Type::I64,
                vec![str_set_let(), remove(Expr::LitStr("q".into()))],
                slen(),
            ),
        ],
    )
}

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal, so a negative `i64` renders as its `u64` two's-complement value —
/// parse as `u64` and reinterpret. The `<ty>` label (`i32` for `in`, `i64` for
/// `len`) is ignored — only the value after `:` matters.
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-setrm-{}-{}", tag, std::process::id()));
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
fn set_remove_lowers_and_carries_shape() {
    let wat = emit_module(&probe_module())
        .expect("the `s.remove`/`s.discard` program must lower through emit_module");
    // Removal reuses the SHARED pop helper for both element kinds — NO bespoke
    // set-removal helper.
    assert!(
        !wat.contains("$__wasm_set_remove") && !wat.contains("$__wasm_set_discard"),
        "removal must NOT declare a bespoke helper — it reuses the pop helper:\n{wat}"
    );
    // int-elem AND str-elem sets are present, so both pop helpers exist and are
    // called by the removal sites.
    for helper in ["call $__wasm_dict_pop_i", "call $__wasm_dict_pop_s"] {
        assert!(wat.contains(helper), "missing pop call {helper}:\n{wat}");
    }
    // `discard` GATES the pop behind the never-trapping `has` — so the has helper
    // is called for both element kinds too.
    for helper in ["call $__wasm_dict_has_i", "call $__wasm_dict_has_s"] {
        assert!(
            wat.contains(helper),
            "discard must gate on {helper}:\n{wat}"
        );
    }
    // The pop helper carries the swap-last-into-hole `memory.copy` + count-- and
    // the KeyError not-found trap.
    assert!(
        wat.contains("memory.copy") && wat.contains("unreachable"),
        "the pop helper must carry the swap-into-hole copy + the KeyError trap:\n{wat}"
    );
    // A removal statement DROPS the popped dummy value.
    assert!(
        wat.contains("drop"),
        "a set removal must drop the pop helper's returned dummy value:\n{wat}"
    );
    // The nested-heap discard forces the str-eq content helper to be declared
    // (the walker-recursion gate-hole guard proven at ASSEMBLE time below).
    assert!(
        wat.contains("$__wasm_str_eq"),
        "the str-keyed removal path must carry the content-compare helper:\n{wat}"
    );
}

#[test]
fn set_remove_over_non_set_local_is_refused() {
    // `s.remove(e)` where `s` is NOT a set local (here a plain `i64`) has no
    // element-kind — an HONEST refusal, not a silent miscompile.
    let m = module(
        "setrm_non_set",
        vec![func(
            "f",
            Type::I64,
            vec![
                Stmt::Let {
                    name: "s".into(),
                    ty: Type::I64,
                    mutable: true,
                    value: Expr::LitInt(0),
                },
                remove(Expr::LitInt(1)),
            ],
            Expr::LitInt(0),
        )],
    );
    let err =
        emit_module(&m).expect_err("`s.remove` over a non-set local must be refused by WASM lane");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("set") && msg.contains('s'),
        "the refusal must name the non-`set` receiver: {msg}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn set_remove_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1240: skipping EXECUTED `s.remove`/`s.discard` witness — WABT \
             (wat2wasm / wasm-interp) absent. The program lowered through emit_module \
             (asserted in `set_remove_lowers_and_carries_shape`); a box with WABT also \
             runs every export and asserts each == the CPython value {PINS:?}. Free CI \
             skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1240: running EXECUTED `s.remove`/`s.discard` witness via WABT");
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
    // Every value probe removes a PRESENT element or discards (never traps).
    assert!(
        !stdout.contains("unreachable executed"),
        "no value probe should trap (discard never traps; every remove hits a present element):\n{stdout}"
    );

    eprintln!(
        "PMAT-1240: EXECUTED `s.remove`/`s.discard` witness PASSED — middle/last/first \
         removes dropped the element, bystanders + the swapped-in last element survived, \
         len decremented by one per remove (remove-all → 0), discard-absent was a no-op, \
         discard-then-re-add restored membership, a loop-discard through a While body \
         removed 1..3, and the str-keyed + nested-heap paths matched. All == CPython {PINS:?}."
    );
}

#[test]
fn set_remove_absent_element_traps_keyerror() {
    let wat = emit_module(&trap_module()).expect("trap program lowers through emit_module");
    assert!(
        wat.contains("unreachable"),
        "the pop helper's not-found tail must be `unreachable` (KeyError analogue):\n{wat}"
    );

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1240: skipping EXECUTED `s.remove(absent)` trap witness — WABT absent. \
             The emitted module carries the `unreachable` KeyError tail (asserted above); \
             a box with WABT confirms the export traps at runtime."
        );
        return;
    }

    let (stdout, _ok) = assemble_and_run("trap", &wat);
    for name in ["rm_miss", "rm_s_miss"] {
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("{name}()")))
            .unwrap_or_else(|| panic!("no `{name}` line in interp output:\n{stdout}"));
        assert!(
            line.contains("unreachable executed"),
            "`s.remove(absent)` ({name}) must trap (KeyError analogue), got: {line:?}\n\
             full output:\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1240: `s.remove(absent)` trap witness PASSED — both an int-elem and a \
         str-elem missing-element remove trapped (unreachable == CPython KeyError). \
         (discard never traps — covered by the value witness above.)"
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Each probe rebuilds its set; removal mutates, so mirror that with a fresh
    // set per line (exactly what each WASM function does).
    let py = "\
def si(): return {1,2,3}\n\
def si6(): return set(range(1,7))\n\
def ss(): return {'a','bb','ccc'}\n\
def loop():\n\
\ts=si6()\n\
\ti=1\n\
\twhile i<=3:\n\
\t\ts.discard(i); i+=1\n\
\treturn s\n\
v={}\n\
s=si(); s.remove(2); v['rm_mid_gone']=int(2 in s)\n\
s=si(); s.remove(2); v['rm_mid_bystander']=int(1 in s)\n\
s=si(); s.remove(2); v['rm_mid_swapped']=int(3 in s)\n\
s=si(); s.remove(2); v['rm_mid_len']=len(s)\n\
s=si(); s.remove(3); v['rm_last_gone']=int(3 in s)\n\
s=si(); s.remove(3); v['rm_last_bystander']=int(1 in s)\n\
s=si(); s.remove(3); v['rm_last_len']=len(s)\n\
s=si(); s.remove(1); v['rm_first_gone']=int(1 in s)\n\
s=si(); s.remove(1); v['rm_first_bystander']=int(2 in s)\n\
s=si(); s.remove(1); s.remove(2); s.remove(3); v['rm_all_len']=len(s)\n\
s=si(); s.discard(2); v['dc_present_gone']=int(2 in s)\n\
s=si(); s.discard(2); v['dc_present_len']=len(s)\n\
s=si(); s.discard(99); v['dc_absent_len']=len(s)\n\
s=si(); s.discard(99); v['dc_absent_bystander']=int(1 in s)\n\
s=si(); s.discard(2); s.add(2); v['dc_readd']=int(2 in s)\n\
s=loop(); v['dc_loop_len']=len(s)\n\
s=loop(); v['dc_loop_survivor']=int(6 in s)\n\
s=loop(); v['dc_loop_gone']=int(2 in s)\n\
s=ss(); s.remove('a'); v['rm_s_gone']=int('a' in s)\n\
s=ss(); s.remove('a'); v['rm_s_bystander']=int('bb' in s)\n\
s=ss(); s.remove('a'); v['rm_s_swapped']=int('ccc' in s)\n\
s=ss(); s.remove('a'); v['rm_s_len']=len(s)\n\
s=ss(); s.discard('bb'); v['dc_s_gone']=int('bb' in s)\n\
s=ss(); s.discard('bb'); v['dc_s_len']=len(s)\n\
s=ss(); s.discard('zz'); v['dc_s_absent_len']=len(s)\n\
s={'ab','yo'}; a='a'; s.discard(a+'b'); v['dc_nested_heap_len']=len(s)\n\
s={'ab','yo'}; a='a'; s.discard(a+'b'); v['dc_nested_heap_gone']=int('ab' in s)\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1240: python3 absent — pins asserted against the WABT witness only");
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
