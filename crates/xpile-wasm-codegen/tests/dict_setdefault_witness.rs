//! PMAT-1227 — EXECUTED witness for native-WASM `d.setdefault(k, default)`
//! (`Expr::DictSetDefault`) over the bump-heap dict runtime
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! The dict runtime shipped `d[k]` (`Expr::DictGet`, TRAPS on absent), `k in d`
//! (`Expr::DictContains`), `d[k] = v` (`Stmt::DictSet`), the total read
//! `d.get(k, default)` (PMAT-1223, `Expr::DictGetOr`), and the removing
//! `d.pop(k[, default])` (PMAT-1225, `Expr::DictPop`). This slice adds
//! `d.setdefault(k, default)` — a get-or-INSERT: on a HIT read the existing
//! value (NO overwrite); on a MISS insert `default` under `k` and return it.
//!
//! ```wat
//! ;; setdefault(d, k, default):
//! ;;   if not has(d, k): d = set(d, k, default)   ;; insert-if-absent (never overwrites)
//! ;;   return get(d, k)                           ;; now guaranteed present, i64
//! ```
//!
//! Unlike `d.get`/`d.pop` (which never grow — `get` reads, `pop` shrinks in
//! place), the MISS path calls the update-or-insert helper `$__wasm_dict_set_<k>`,
//! which 2x-REALLOCS + copies when the region is at capacity and returns the
//! (possibly relocated) base pointer — WRITTEN BACK into the dict local, exactly
//! like `d[k] = v`. This is the one behaviour a get/pop witness never exercised:
//! a mutation that can MOVE the dict. The membership helper gates the insert so a
//! HIT never overwrites (CPython keeps the existing value).
//!
//! ## Witness shape
//!
//! Every probe is a ZERO-ARG export returning an `i64`. Value probes put
//! `setdefault` in tail (return) position. Mutation-proof probes run
//! `setdefault` as a BARE STATEMENT (`Stmt::SideEffectCall`, value dropped — the
//! insert-if-absent side effect is the point) then read the post-state with the
//! known-good `d.get(k, -1)`:
//!
//!   * HIT → returns the pre-existing value, and a re-read shows NO overwrite.
//!   * MISS → returns `default`, and a re-read shows the key is now present.
//!   * MISS bystander → an untouched key survives the insert.
//!   * double setdefault on a missing key → the SECOND call is a HIT (returns the
//!     first-inserted value, does not overwrite with its own default).
//!   * GROWTH → start from a 1-entry dict (capacity `1 + DICT_GROWTH_SLACK` = 17),
//!     setdefault 19 fresh keys (20 entries forces the 2x-realloc + copy). Both
//!     the ORIGINAL key and a LATE-inserted key must still read back — proof the
//!     grown base-pointer was written back to the local (a broken write-back would
//!     leave the local dangling at the pre-grow region → corrupt reads).
//!   * str-keyed (content-compare) HIT + MISS + inserted paths.
//!
//! Every pin is cross-checked against live `python3` in `cpython_pins_are_python`,
//! each with a FRESH dict (setdefault mutates), mirroring how each WASM function
//! rebuilds its dict. Gated on `wasm_runtime_available()` — a clean skip (still
//! asserting the EMIT path lowers + carries the shape) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
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

/// `d = {1: 10}` — a 1-entry int dict (capacity `1 + DICT_GROWTH_SLACK` = 17):
/// the seed for the GROWTH probe.
fn one_int_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::I64), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![(Expr::LitInt(1), Expr::LitInt(10))]),
    }
}

/// `d = {"x": 100, "y": 200}` — a str-keyed dict local (content-compare path).
fn str_dict_let() -> Stmt {
    Stmt::Let {
        name: "d".into(),
        ty: Type::Dict(Box::new(Type::Str), Box::new(Type::I64)),
        mutable: true,
        value: Expr::DictLit(vec![
            (Expr::LitStr("x".into()), Expr::LitInt(100)),
            (Expr::LitStr("y".into()), Expr::LitInt(200)),
        ]),
    }
}

/// `d.setdefault(key, default)` — the get-or-insert expression.
fn setdefault(key: Expr, default: Expr) -> Expr {
    Expr::DictSetDefault {
        dict: Box::new(ident("d")),
        key: Box::new(key),
        default: Box::new(default),
    }
}

/// `d.setdefault(key, default)` as a BARE STATEMENT — value dropped, insert kept
/// (`Stmt::SideEffectCall`, exercises the statement-position arm).
fn sd_stmt(key: Expr, default: Expr) -> Stmt {
    Stmt::SideEffectCall {
        call: setdefault(key, default),
    }
}

/// `d.get(key, -1)` — the known-good total read, used to OBSERVE the post-state
/// as a single scalar.
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

/// The seed dict + 19 `setdefault` inserts of keys 2..=20 (values k*10). Starting
/// from a 1-entry dict (cap 17), the 20th entry forces the 2x-realloc + copy — so
/// these statements EXERCISE the grow + write-back path via `setdefault`.
fn grow_stmts() -> Vec<Stmt> {
    let mut s = vec![one_int_dict_let()];
    for k in 2..=20i64 {
        s.push(sd_stmt(Expr::LitInt(k), Expr::LitInt(k * 10)));
    }
    s
}

fn probe_module() -> Module {
    module(
        "dict_setdefault_witness",
        vec![
            // ── value probes: setdefault in tail (return) position ───────────
            // present key → the EXISTING value (default 999 ignored, no overwrite)
            func(
                "sdv_hit",
                vec![int_dict_let()],
                setdefault(Expr::LitInt(2), Expr::LitInt(999)),
            ),
            // absent key → the default (inserted and returned)
            func(
                "sdv_miss",
                vec![int_dict_let()],
                setdefault(Expr::LitInt(9), Expr::LitInt(99)),
            ),
            // ── mutation-proof probes: setdefault as a statement, then read ──
            // HIT does NOT overwrite: re-read key 2 == 20 (not 999)
            func(
                "sd_hit_nomut",
                vec![int_dict_let(), sd_stmt(Expr::LitInt(2), Expr::LitInt(999))],
                get_or(Expr::LitInt(2), -1),
            ),
            // MISS inserts: re-read key 9 == 99 (now present)
            func(
                "sd_miss_inserted",
                vec![int_dict_let(), sd_stmt(Expr::LitInt(9), Expr::LitInt(99))],
                get_or(Expr::LitInt(9), -1),
            ),
            // MISS leaves bystanders: re-read key 2 == 20 after inserting 9
            func(
                "sd_miss_bystander",
                vec![int_dict_let(), sd_stmt(Expr::LitInt(9), Expr::LitInt(99))],
                get_or(Expr::LitInt(2), -1),
            ),
            // second setdefault on the now-present key 9 is a HIT → returns 99
            // (the FIRST insert), NOT its own default 777
            func(
                "sd_twice_hit",
                vec![int_dict_let(), sd_stmt(Expr::LitInt(9), Expr::LitInt(99))],
                setdefault(Expr::LitInt(9), Expr::LitInt(777)),
            ),
            // ── GROWTH: insert past capacity forces realloc + write-back ─────
            // the ORIGINAL key survives the relocation → key 1 == 10
            func("sd_grow_orig", grow_stmts(), get_or(Expr::LitInt(1), -1)),
            // a LATE-inserted key survives → key 20 == 200
            func("sd_grow_late", grow_stmts(), get_or(Expr::LitInt(20), -1)),
            // and an early-inserted key (before the grow) survives → key 5 == 50
            func("sd_grow_early", grow_stmts(), get_or(Expr::LitInt(5), -1)),
            // ── str-keyed (content-compare) path ─────────────────────────────
            // present str key → existing value (default 5 ignored)
            func(
                "sd_s_hit",
                vec![str_dict_let()],
                setdefault(Expr::LitStr("x".into()), Expr::LitInt(5)),
            ),
            // absent str key → default (inserted)
            func(
                "sd_s_miss",
                vec![str_dict_let()],
                setdefault(Expr::LitStr("z".into()), Expr::LitInt(5)),
            ),
            // absent str key inserted: re-read "z" == 5
            func(
                "sd_s_miss_inserted",
                vec![
                    str_dict_let(),
                    sd_stmt(Expr::LitStr("z".into()), Expr::LitInt(5)),
                ],
                get_or(Expr::LitStr("z".into()), -1),
            ),
            // …and a str bystander survives: re-read "y" == 200
            func(
                "sd_s_bystander",
                vec![
                    str_dict_let(),
                    sd_stmt(Expr::LitStr("z".into()), Expr::LitInt(5)),
                ],
                get_or(Expr::LitStr("y".into()), -1),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every probe export.
const PINS: &[(&str, i64)] = &[
    ("sdv_hit", 20),
    ("sdv_miss", 99),
    ("sd_hit_nomut", 20),
    ("sd_miss_inserted", 99),
    ("sd_miss_bystander", 20),
    ("sd_twice_hit", 99),
    ("sd_grow_orig", 10),
    ("sd_grow_late", 200),
    ("sd_grow_early", 50),
    ("sd_s_hit", 100),
    ("sd_s_miss", 5),
    ("sd_s_miss_inserted", 5),
    ("sd_s_bystander", 200),
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
        std::env::temp_dir().join(format!("xpile-wasm-dictsd-{}-{}", tag, std::process::id()));
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
fn dict_setdefault_lowers_and_carries_shape() {
    let wat = emit_module(&probe_module())
        .expect("the `d.setdefault(k, default)` program must lower through emit_module");
    // setdefault reuses the shared has/set/get helpers for BOTH key kinds — it
    // declares NO new helper (no `$__wasm_dict_setdefault_*`).
    assert!(
        !wat.contains("$__wasm_dict_setdefault"),
        "setdefault must NOT declare a bespoke helper — it composes has/set/get:\n{wat}"
    );
    for helper in [
        "call $__wasm_dict_has_i",
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_get_i",
        "call $__wasm_dict_has_s",
        "call $__wasm_dict_set_s",
        "call $__wasm_dict_get_s",
    ] {
        assert!(
            wat.contains(helper),
            "missing composed call {helper}:\n{wat}"
        );
    }
    // The shape is `has ; i32.eqz ; if <insert> end ; get` — an insert-if-absent
    // guarded by membership (a value-carrying `if (result i64)` would be the
    // get_or/pop shape, NOT setdefault's insert-then-read).
    assert!(
        wat.contains("i32.eqz"),
        "setdefault gates the insert on `not has(...)` (i32.eqz):\n{wat}"
    );
    // The insert writes the (possibly grown) base pointer back into the local.
    assert!(
        wat.contains("local.set $d"),
        "the miss-path insert must write the (grown) pointer back to the dict local:\n{wat}"
    );
    // The grow path is present (the shared set helper 2x-reallocs via $__alloc).
    assert!(
        wat.contains("call $__alloc") && wat.contains("memory.copy"),
        "the shared set helper must carry the grow (realloc + copy) path:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_setdefault_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1227: skipping EXECUTED `d.setdefault(k, default)` witness — WABT \
             (wat2wasm / wasm-interp) absent. The program lowered through \
             emit_module (asserted in `dict_setdefault_lowers_and_carries_shape`); a \
             box with WABT also runs every export and asserts each == the CPython \
             value {PINS:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-1227: running EXECUTED `d.setdefault(k, default)` witness via WABT");
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
    // setdefault never traps (a MISS inserts rather than raising), so no export
    // should execute `unreachable`.
    assert!(
        !stdout.contains("unreachable executed"),
        "no setdefault probe should trap (a miss inserts, never KeyError):\n{stdout}"
    );

    eprintln!(
        "PMAT-1227: EXECUTED `d.setdefault(k, default)` witness PASSED — HIT returned \
         the existing value with NO overwrite, MISS inserted + returned the default, \
         bystanders survived, a repeat setdefault on a now-present key was a HIT, the \
         GROWTH probe (20 entries past a cap-17 seed) kept the original + early + late \
         keys readable across the realloc + write-back, and the str-keyed path \
         matched. All value-match CPython {PINS:?}."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Each probe rebuilds its dict; setdefault mutates, so mirror that with a
    // fresh dict per line (exactly what each WASM function does). The grow probes
    // insert keys 2..=20 via setdefault, matching `grow_stmts()`.
    let py = "\
def f(): return {1:10,2:20,3:30}\n\
def one(): return {1:10}\n\
def s(): return {'x':100,'y':200}\n\
def grow():\n\
\td=one()\n\
\tfor k in range(2,21): d.setdefault(k, k*10)\n\
\treturn d\n\
v={}\n\
d=f(); v['sdv_hit']=d.setdefault(2,999)\n\
d=f(); v['sdv_miss']=d.setdefault(9,99)\n\
d=f(); d.setdefault(2,999); v['sd_hit_nomut']=d.get(2,-1)\n\
d=f(); d.setdefault(9,99); v['sd_miss_inserted']=d.get(9,-1)\n\
d=f(); d.setdefault(9,99); v['sd_miss_bystander']=d.get(2,-1)\n\
d=f(); d.setdefault(9,99); v['sd_twice_hit']=d.setdefault(9,777)\n\
d=grow(); v['sd_grow_orig']=d.get(1,-1)\n\
d=grow(); v['sd_grow_late']=d.get(20,-1)\n\
d=grow(); v['sd_grow_early']=d.get(5,-1)\n\
d=s(); v['sd_s_hit']=d.setdefault('x',5)\n\
d=s(); v['sd_s_miss']=d.setdefault('z',5)\n\
d=s(); d.setdefault('z',5); v['sd_s_miss_inserted']=d.get('z',-1)\n\
d=s(); d.setdefault('z',5); v['sd_s_bystander']=d.get('y',-1)\n\
print(';'.join(f'{k}={val}' for k,val in v.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-1227: python3 absent — pins asserted against the WABT witness only");
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
