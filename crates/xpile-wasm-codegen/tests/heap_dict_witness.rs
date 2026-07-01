//! PMAT-995 (slice 3b) — EXECUTED dict/set witness for the native WASM EMIT
//! lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! Slices 1–2 shipped `str` params, `len`/`ord`, the bump allocator, string
//! concat/`chr`, string literals, `s[i]`, and content equality. This slice
//! adds the FIRST associative collections — `dict[int|str, int]` and
//! `set[int|str]` — as bump-heap open assoc-arrays:
//!
//!   * header (8 bytes, keeps entries 8-aligned): `i32` live **count** @ base+0
//!     (the same `+0` count header `len` already reads), `i32` **capacity** @
//!     base+4;
//!   * `capacity` fixed 16-byte entries from base+8: `key` @ entry+0 (an `i64`
//!     int key, or the `i32` string base-pointer for a str key), `value` @
//!     entry+8 (an `i64`; a set stores a dummy).
//!
//! Three helpers per key kind do a LINEAR scan of the `count` live entries:
//! `$__wasm_dict_get_<k>` (Python `d[k]`; `unreachable`-TRAPS on an absent key
//! — the KeyError analogue, mirroring the list-index bounds trap), `_has_<k>`
//! (`k in d` / `x in s` → 0/1), `_set_<k>` (`d[k] = v` / `s.add(e)`:
//! update-or-insert). `<k>` is `i` for int keys (`i64.eq`) or `s` for str keys
//! (`$__wasm_str_eq` CONTENT compare over the stored string pointers).
//!
//! ## Witness shape
//!
//! Every probed program is a ZERO-ARG function returning a readable scalar
//! (`i64` for `d[k]`/`len`, `i32` for `k in d`) — it builds its dict/set from a
//! literal on the bump heap, then reads it. So unlike the string witness (which
//! splices a driver to preload inputs and read back bytes), this witness needs
//! NO driver: `wasm-interp --run-all-exports` invokes each export directly and
//! prints the scalar. The test lowers a real program through the production
//! `emit_module`, assembles (`wat2wasm`) + runs (`wasm-interp`) it, and asserts
//! each executed scalar VALUE-MATCHES CPython (`python3`, cross-checked live
//! when present; otherwise the pinned constants that `python3` produces).
//!
//! The absent-key path is witnessed too: a `d[9]` program must TRAP at RUN
//! (`wasm-interp` reports `unreachable executed`) — the executed KeyError.
//!
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path lowers + carries the dict/set helpers) on a host without WABT.

use std::process::Command;

use xpile_meta_hir::{Block, Expr, Function, Item, Module, SourceLang, Stmt, Type};
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- meta-HIR builders (what the Python frontend would produce) ------------

fn ident(n: &str) -> Expr {
    Expr::Ident(n.into())
}

/// `d = {1: 10, 2: 20, 3: 30}` — an int-keyed dict local.
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

/// `s = {5, 6, 7}` — an int set local.
fn int_set_let() -> Stmt {
    Stmt::Let {
        name: "s".into(),
        ty: Type::Set(Box::new(Type::I64)),
        mutable: false,
        value: Expr::SetLit(vec![Expr::LitInt(5), Expr::LitInt(6), Expr::LitInt(7)]),
    }
}

fn dict_get(name: &str, key: Expr) -> Expr {
    Expr::DictGet {
        dict: Box::new(ident(name)),
        key: Box::new(key),
    }
}
fn dict_has(name: &str, key: Expr) -> Expr {
    Expr::DictContains {
        dict: Box::new(ident(name)),
        key: Box::new(key),
    }
}
fn set_has(name: &str, elem: Expr) -> Expr {
    Expr::SetContains {
        set: Box::new(ident(name)),
        elem: Box::new(elem),
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

/// The full non-trapping probe module: one zero-arg export per assertion.
/// `(export_name, expected_scalar)` pairs are pinned below; each is exactly
/// what CPython computes (cross-checked live in `cpython_pins_are_python`).
fn probe_module() -> Module {
    module(
        "dict_witness",
        vec![
            // int dict: get / len / membership
            func(
                "get2",
                Type::I64,
                vec![int_dict_let()],
                dict_get("d", Expr::LitInt(2)),
            ),
            func(
                "get3",
                Type::I64,
                vec![int_dict_let()],
                dict_get("d", Expr::LitInt(3)),
            ),
            func(
                "lend",
                Type::I64,
                vec![int_dict_let()],
                Expr::Len(Box::new(ident("d"))),
            ),
            func(
                "has2",
                Type::Bool,
                vec![int_dict_let()],
                dict_has("d", Expr::LitInt(2)),
            ),
            func(
                "has9",
                Type::Bool,
                vec![int_dict_let()],
                dict_has("d", Expr::LitInt(9)),
            ),
            // int set: membership / len
            func(
                "shas6",
                Type::Bool,
                vec![int_set_let()],
                set_has("s", Expr::LitInt(6)),
            ),
            func(
                "shas8",
                Type::Bool,
                vec![int_set_let()],
                set_has("s", Expr::LitInt(8)),
            ),
            func(
                "slen",
                Type::I64,
                vec![int_set_let()],
                Expr::Len(Box::new(ident("s"))),
            ),
            // str dict: content-keyed get / membership
            func(
                "sgetY",
                Type::I64,
                vec![str_dict_let()],
                dict_get("d", Expr::LitStr("y".into())),
            ),
            func(
                "sgetX",
                Type::I64,
                vec![str_dict_let()],
                dict_get("d", Expr::LitStr("x".into())),
            ),
            func(
                "shasY",
                Type::Bool,
                vec![str_dict_let()],
                dict_has("d", Expr::LitStr("y".into())),
            ),
            func(
                "shasZ",
                Type::Bool,
                vec![str_dict_let()],
                dict_has("d", Expr::LitStr("z".into())),
            ),
        ],
    )
}

/// `(export, expected)` — the CPython value for every probe export.
/// `python3 -c "d={1:10,2:20,3:30}; print(d[2],d[3],len(d),int(2 in d),int(9 in d))"`
/// → `20 30 3 1 0`; the str-dict / set values likewise (see the module above).
const PINS: &[(&str, i64)] = &[
    ("get2", 20),
    ("get3", 30),
    ("lend", 3),
    ("has2", 1),
    ("has9", 0),
    ("shas6", 1),
    ("shas8", 0),
    ("slen", 3),
    ("sgetY", 200),
    ("sgetX", 100),
    ("shasY", 1),
    ("shasZ", 0),
];

// ---- WABT harness -----------------------------------------------------------

/// Parse a `name() => i32:<v>` or `name() => i64:<v>` line for `name`.
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
    val.parse()
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

/// Assemble `wat` and run all exports; returns wasm-interp's (stdout, success).
fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dict-{}-{}", tag, std::process::id()));
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
    // wasm-interp exits 0 and prints the trap inline (`=> error: ...`), so
    // return stdout regardless and let callers inspect it.
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

// ---- CONSTRUCT assertions (hold with or without WABT) -----------------------

#[test]
fn dict_program_lowers_and_carries_helpers() {
    let wat = emit_module(&probe_module())
        .expect("the dict/set program must lower through the production emit_module");
    // The bump allocator underlies every dict/set allocation.
    assert!(
        wat.contains("(func $__alloc") && wat.contains("(global $__heap_ptr (mut i32)"),
        "dict/set needs the bump allocator:\n{wat}"
    );
    // Int-key + str-key helpers are both present (str via content compare).
    for helper in [
        "$__wasm_dict_get_i",
        "$__wasm_dict_has_i",
        "$__wasm_dict_get_s",
        "$__wasm_dict_has_s",
        "$__wasm_str_eq",
    ] {
        assert!(wat.contains(helper), "missing helper {helper}:\n{wat}");
    }
    // The absent-key trap (KeyError analogue) is emitted in the get helper.
    assert!(
        wat.contains("unreachable"),
        "d[k] must trap on an absent key:\n{wat}"
    );
    // str keys are laid out as static (data) literals (the collect fix).
    assert!(
        wat.contains("(data (i32.const 512)"),
        "str keys must be laid out as static data literals:\n{wat}"
    );
}

#[test]
fn missing_key_get_lowers_with_trap() {
    // `d[9]` over `{1,2,3}` — lowers fine; the trap only fires at RUN.
    let m = module(
        "missing",
        vec![func(
            "missing",
            Type::I64,
            vec![int_dict_let()],
            dict_get("d", Expr::LitInt(9)),
        )],
    );
    let wat = emit_module(&m).expect("absent-key get still lowers (traps at run)");
    assert!(
        wat.contains("unreachable"),
        "absent-key trap machinery:\n{wat}"
    );
}

// ---- EXECUTED witness (gated on WABT) --------------------------------------

#[test]
fn dict_set_program_executes_in_wasm_and_matches_cpython() {
    let wat = emit_module(&probe_module()).expect("dict/set program lowers through emit_module");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-995: skipping EXECUTED dict/set witness — WABT (wat2wasm / \
             wasm-interp) absent. The program lowered through emit_module \
             (asserted in `dict_program_lowers_and_carries_helpers`); a box \
             with WABT also runs every export and asserts each == the CPython \
             value {PINS:?}. Free CI skips execution and stays green."
        );
        return;
    }

    eprintln!("PMAT-995: running EXECUTED dict/set witness via WABT");
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

    eprintln!(
        "PMAT-995: EXECUTED dict/set witness PASSED — int + str dict get/len/\
         membership and int-set membership/len all lowered through emit_module, \
         bump-allocated a [count][cap] entry array in linear memory, and executed \
         in WABT value-matching CPython {PINS:?}. PMAT-986 heap runtime: dicts + \
         sets are real."
    );
}

#[test]
fn absent_key_get_traps_at_run() {
    // Python `d[9]` on `{1:10,2:20,3:30}` raises KeyError; the WASM lowering
    // TRAPS (`unreachable`). Prove wasm-interp reports the trap, not a value.
    let m = module(
        "trap",
        vec![func(
            "missing",
            Type::I64,
            vec![int_dict_let()],
            dict_get("d", Expr::LitInt(9)),
        )],
    );
    let wat = emit_module(&m).expect("absent-key get lowers");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-995: skipping EXECUTED absent-key trap witness — WABT absent. \
             `d[9]` lowers with the `unreachable` trap (asserted in \
             `missing_key_get_lowers_with_trap`); a box with WABT also RUNS it \
             and confirms wasm-interp reports `unreachable executed` (the \
             KeyError analogue) instead of returning a value."
        );
        return;
    }

    let (stdout, _ok) = assemble_and_run("trap", &wat);
    assert!(
        stdout.contains("error: unreachable executed"),
        "d[9] must TRAP (KeyError analogue), got:\n{stdout}\n---WAT---\n{wat}"
    );
    // And it must NOT have returned an ordinary scalar for `missing`.
    assert!(
        !stdout.contains("missing() => i64:"),
        "d[9] returned a value instead of trapping:\n{stdout}"
    );
    eprintln!(
        "PMAT-995: EXECUTED absent-key trap witness PASSED — `d[9]` on a \
         3-entry dict TRAPPED (`unreachable executed`) under WABT, matching \
         CPython's KeyError. The dict-get bounds/KeyError posture is real."
    );
}

// ---- CPython differential cross-check (gated on python3) --------------------

#[test]
fn cpython_pins_are_python() {
    // Recompute every pin with the real CPython interpreter when present, so
    // the pinned constants can't silently drift from Python semantics.
    let py = "\
d = {1: 10, 2: 20, 3: 30}\n\
sd = {'x': 100, 'y': 200}\n\
s = {5, 6, 7}\n\
vals = {\n\
 'get2': d[2], 'get3': d[3], 'lend': len(d),\n\
 'has2': int(2 in d), 'has9': int(9 in d),\n\
 'shas6': int(6 in s), 'shas8': int(8 in s), 'slen': len(s),\n\
 'sgetY': sd['y'], 'sgetX': sd['x'],\n\
 'shasY': int('y' in sd), 'shasZ': int('z' in sd),\n\
}\n\
print(';'.join(f'{k}={v}' for k, v in vals.items()))\n";
    let out = match Command::new("python3").arg("-c").arg(py).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("PMAT-995: python3 absent — pins asserted against the WABT witness only");
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
