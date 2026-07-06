//! PMAT-1309 — EXECUTED witness for dict/set FUNCTION PARAMETERS in the WASM
//! lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): `def f(d: dict[int, int])
//! -> int` lowers with the dict riding the SAME i32 base-pointer ABI a
//! list/str/struct param does (PMAT-966/986/996), passed by reference through
//! intra-module calls — the FIRST dict/set flow across a function boundary.
//!
//! ## What a dict/set param supports (executed here, value-matched vs CPython)
//!
//! * the WHOLE read surface: `d[k]` (guarded), `d.get(k, default)`, `k in d`
//!   / `k not in d`, `len(d)`, bare-key + `.values()` iteration folds, and
//!   `==`/`!=` — including the str-VALUED content-comparing sv-twin
//!   (PMAT-1307) with a PARAM operand;
//! * IN-PLACE mutation with **caller-visible reference semantics** — `d.pop`
//!   / `del d[k]` / `s.discard` / `d.clear()` never relocate the record
//!   (swap-last-into-hole / count-header write), so the caller observes the
//!   mutation through its own pointer, exactly Python. Pinned by reading
//!   `len`/`get` IN THE CALLER after the helper mutates;
//! * param HANDOFF (`outer(d)` passes `d` on to `inner(d)`), the same dict
//!   passed TWICE (`q_pair(d, d)` — both names alias one record), and a
//!   param as the read-only SOURCE of a local's `update`;
//! * str-KEYED and str-VALUED dict params, set params.
//!
//! ## What refuses (the honest-growth boundary, pinned below)
//!
//! A GROWTH op through a param — `d[k] = v`, `d.setdefault` (both value
//! lanes), `d.update(...)` as RECEIVER, `s.add` — can 2x-grow + RELOCATE the
//! record; the write-back updates only the callee's local, so the caller
//! would keep a STALE base-pointer. Each refuses with a precise diagnostic
//! (`refuse_heap_param_growth`). Growth stays supported where the dict is
//! BOUND (the caller): `check_param_clear_visible` re-inserts caller-side
//! after a helper cleared through the param.
//!
//! ## Call-site kind checking (silent-miscompile belt)
//!
//! Every dict/set param is an i32 at the WAT level, so a kind-mismatched
//! argument would be read with the WRONG key encoding / value interpretation
//! — a silent miscompile, not a trap. `check_heap_call_args` kind-checks the
//! triple `(key_kind, value_is_str, is_set)` per argument, pinned here:
//! str-keyed→int-keyed, str-valued→int-valued, set→dict, a dict literal
//! argument (bind it first), a dict passed to a NON-dict param, and dict/set
//! params on struct METHODS (free functions only) all refuse.
//!
//! ## Param-seeded gate walkers (the PMAT-1305/1307 standing lesson)
//!
//! A module whose ONLY dict is a PARAM (no `Let` anywhere) must still emit
//! the `$__wasm_dict_*_<k>` helper family, the `(memory …)`, `$__wasm_str_eq`
//! for a str-valued param's content compares, and the `$__wasm_dict_eq_sv_<k>`
//! twin for a param-hosted str-valued `==` — every name-set-driven gate scan
//! (`module_dict_key_kinds`, `module_needs_str_eq`/`_cmp`,
//! `module_needs_dict_eq_sv`) now seeds from the SIGNATURE before walking the
//! body. Pinned in `param_only_modules_carry_their_helpers`; a miss is a
//! `call` against an undeclared helper — a hard wat2wasm failure the executed
//! lane would catch too.
//!
//! ## Witness shape
//!
//! ONE module of standalone `def`s (helpers with dict/set params + zero-arg
//! `check_*` observables; valid plain `python3` AND wasm-frontend-lowerable
//! through the real CLI profile). `wasm-interp --run-all-exports` runs every
//! export — helpers included, invoked with ZEROED args, so every helper is
//! written TOTAL (get-with-default / membership-guarded del / discard /
//! clear): address 0 holds count=0 in zeroed linear memory and no helper
//! traps. Each `check_*` value is pinned to a hand-derived constant AND
//! cross-checked against live `python3` on the IDENTICAL source (zero
//! reimplementation risk). Gated on `wasm_runtime_available()` — a clean
//! skip (still asserting the EMIT path + helper carriage) without WABT.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

// ---- frontend lowering (the CLI's `--target wasm` path) ---------------------

fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// FULL pipeline: Python source (one or more `def`s) → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the executed corpus ----------------------------------------------------

/// `(observable, hand-derived CPython value)` — the oracle re-derives each at
/// runtime, so a wrong constant here fails against BOTH lanes.
const PINS: &[(&str, i64)] = &[
    ("check_param_get", 1993),
    ("check_param_subscript", 329),
    ("check_param_len_contains", 7),
    ("check_param_iter_fold", 66),
    ("check_param_eq", 10),
    ("check_param_str_key", 72),
    ("check_param_str_val", 103),
    ("check_param_strv_eq", 10),
    ("check_param_pop_visible", 20019),
    ("check_param_del_visible", 101),
    ("check_param_clear_visible", 80),
    ("check_param_set", 30),
    ("check_param_set_discard_visible", 11),
    ("check_param_handoff", 46),
    ("check_param_same_dict_twice", 38),
    ("check_param_update_source", 4200),
    ("check_param_mutate_then_eq", 51),
    ("check_param_strk_del_visible", 109),
    ("check_param_strv_pop_visible", 411),
];

/// The single executed module — helpers take dict/set params; every helper is
/// TOTAL (no trap under `--run-all-exports` zeroed-arg invocation).
fn corpus_source() -> String {
    r#"def p_get(d: dict[int, int]) -> int:
    return d.get(2, -1) * 100 + d.get(9, -7)

def check_param_get() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    return p_get(d)

def p_sub(d: dict[int, int]) -> int:
    if 2 in d:
        return d[2]
    return -1

def check_param_subscript() -> int:
    d: dict[int, int] = {2: 33}
    e: dict[int, int] = {1: 5}
    return p_sub(d) * 10 + p_sub(e)

def p_meminfo(d: dict[int, int]) -> int:
    n: int = len(d)
    if 1 in d:
        n = n * 2
    if 7 not in d:
        n = n + 1
    return n

def check_param_len_contains() -> int:
    d: dict[int, int] = {1: 10, 5: 50, 9: 90}
    return p_meminfo(d)

def p_fold(d: dict[int, int]) -> int:
    acc: int = 0
    for k in d:
        acc = acc + k
    for v in d.values():
        acc = acc + v
    return acc

def check_param_iter_fold() -> int:
    d: dict[int, int] = {1: 10, 2: 20, 3: 30}
    return p_fold(d)

def p_eq(d: dict[int, int]) -> int:
    m: dict[int, int] = {1: 10, 2: 20}
    if d == m:
        return 1
    return 0

def check_param_eq() -> int:
    a: dict[int, int] = {2: 20, 1: 10}
    b: dict[int, int] = {1: 10, 2: 21}
    return p_eq(a) * 10 + p_eq(b)

def p_strk(d: dict[str, int]) -> int:
    return d.get("x", -1) * 10 + len(d)

def check_param_str_key() -> int:
    d: dict[str, int] = {"x": 7, "y": 8}
    return p_strk(d)

def p_strv(d: dict[int, str]) -> int:
    s: str = d.get(1, "?")
    n: int = len(s)
    if d.get(2, "?") == "vv":
        n = n + 100
    return n

def check_param_str_val() -> int:
    d: dict[int, str] = {1: "abc", 2: "vv"}
    return p_strv(d)

def p_strv_eq(d: dict[int, str]) -> int:
    m: dict[int, str] = {1: "abc", 2: "vv"}
    if d == m:
        return 1
    return 0

def check_param_strv_eq() -> int:
    a: dict[int, str] = {2: "vv", 1: "abc"}
    b: dict[int, str] = {1: "abc", 2: "ba"}
    return p_strv_eq(a) * 10 + p_strv_eq(b)

def p_take(d: dict[int, int]) -> int:
    return d.pop(2, -1)

def check_param_pop_visible() -> int:
    d: dict[int, int] = {1: 10, 2: 20, 3: 30}
    got: int = p_take(d)
    return got * 1000 + len(d) * 10 + d.get(2, -1)

def p_drop(d: dict[int, int]) -> int:
    if 1 in d:
        del d[1]
        return 1
    return 0

def check_param_del_visible() -> int:
    d: dict[int, int] = {1: 10, 5: 50}
    a: int = p_drop(d)
    b: int = p_drop(d)
    return a * 100 + b * 10 + len(d)

def p_wipe(d: dict[int, int]) -> int:
    d.clear()
    return len(d)

def check_param_clear_visible() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    z: int = p_wipe(d)
    d[7] = 70
    return z * 100 + len(d) * 10 + d.get(7, -1)

def p_setinfo(s: set[int]) -> int:
    n: int = len(s)
    if 3 in s:
        n = n * 10
    return n

def check_param_set() -> int:
    s: set[int] = {3, 5, 9}
    return p_setinfo(s)

def p_setdrop(s: set[int]) -> int:
    s.discard(5)
    return len(s)

def check_param_set_discard_visible() -> int:
    s: set[int] = {3, 5}
    a: int = p_setdrop(s)
    b: int = p_setdrop(s)
    if 5 in s:
        return -1
    return a * 10 + b

def q_inner(d: dict[int, int]) -> int:
    return d.get(1, -1)

def q_outer(d: dict[int, int]) -> int:
    return q_inner(d) * 10 + d.get(2, -1)

def check_param_handoff() -> int:
    d: dict[int, int] = {1: 4, 2: 6}
    return q_outer(d)

def q_pair(a: dict[int, int], b: dict[int, int]) -> int:
    return a.get(1, -1) * 10 + b.get(2, -1)

def check_param_same_dict_twice() -> int:
    d: dict[int, int] = {1: 3, 2: 8}
    return q_pair(d, d)

def p_merge_from(src: dict[int, int]) -> int:
    a: dict[int, int] = {1: 1, 9: 9}
    a.update(src)
    return len(a) * 1000 + a.get(1, -1) * 10 + a.get(2, -1)

def check_param_update_source() -> int:
    d: dict[int, int] = {1: 100, 2: 200}
    return p_merge_from(d)

def p_norm(d: dict[int, int]) -> int:
    return d.pop(99, 0)

def check_param_mutate_then_eq() -> int:
    a: dict[int, int] = {1: 10, 99: 5}
    m: dict[int, int] = {1: 10}
    x: int = p_norm(a)
    if a == m:
        return x * 10 + 1
    return x * 10

def p_strk_drop(d: dict[str, int]) -> int:
    if "y" in d:
        del d["y"]
        return 1
    return 0

def check_param_strk_del_visible() -> int:
    d: dict[str, int] = {"x": 1, "y": 2}
    a: int = p_strk_drop(d)
    return a * 100 + len(d) * 10 + d.get("y", -1)

def p_strv_take(d: dict[int, str]) -> int:
    s: str = d.pop(1, "?")
    return len(s)

def check_param_strv_pop_visible() -> int:
    d: dict[int, str] = {1: "abcd", 2: "vv"}
    n: int = p_strv_take(d)
    m: int = p_strv_take(d)
    return n * 100 + m * 10 + len(d)
"#
    .to_string()
}

// ---- CPython oracle ----------------------------------------------------------

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `{observable → value}` from `python3` running the IDENTICAL module source.
fn python_oracle() -> Option<BTreeMap<String, i64>> {
    let mut prog = corpus_source();
    prog.push_str("\nv = {}\n");
    for (name, _) in PINS {
        prog.push_str(&format!("v['{name}'] = {name}()\n"));
    }
    prog.push_str("print(';'.join(f'{k}={val}' for k, val in v.items()))\n");

    let out = Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "PMAT-1309: python3 oracle failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut map = BTreeMap::new();
    for kv in text.trim().split(';') {
        let (k, val) = kv.split_once('=').expect("k=v");
        map.insert(k.to_string(), val.parse::<i64>().expect("int observable"));
    }
    Some(map)
}

// ---- WABT harness --------------------------------------------------------------

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-dparam-{}-{}", std::process::id(), tag));
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
        "wat2wasm failed for {tag}:\n{}\n---WAT (first 4k)---\n{}",
        String::from_utf8_lossy(&assemble.stderr),
        &wat[..wat.len().min(4096)]
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

/// Parse a `name() => i64:<value>` line. `wasm-interp` prints i64 UNSIGNED, so
/// a negative renders as its two's-complement `u64` — parse, reinterpret.
fn parse_scalar(stdout: &str, name: &str) -> i64 {
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

// ---- emit-path pins (run everywhere, no WABT needed) ---------------------------

/// The whole corpus lowers through the FULL pipeline, and the emitted WAT
/// carries the param ABI: dict/set params as `(param $… i32)`, the dict
/// helper families, the memory, and the sv-twin for the param-hosted
/// str-valued `==`.
#[test]
fn corpus_emits_with_param_abi_and_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // dict/set params ride i32 base-pointers.
        "(func $p_get (param $d i32)",
        "(func $p_setinfo (param $s i32)",
        "(func $q_pair (param $a i32) (param $b i32)",
        // helper families for both key kinds (int-keyed reads + str-keyed del).
        "$__wasm_dict_get_i",
        "$__wasm_dict_has_i",
        "$__wasm_dict_pop_i",
        "$__wasm_dict_pop_s",
        // dict equality: the int-valued base helper AND the str-valued twin
        // (both hosted through PARAM operands in this corpus).
        "$__wasm_dict_eq_i",
        "$__wasm_dict_eq_sv_i",
        // str content compare for the str-valued param's `== "vv"`.
        "$__wasm_str_eq",
        "(memory (export \"mem\") 1)",
    ] {
        assert!(
            wat.contains(needle),
            "emitted WAT must contain {needle:?}\n---WAT (first 6k)---\n{}",
            &wat[..wat.len().min(6144)]
        );
    }
}

/// The param-seeded gate-walker pin: a module whose ONLY dict/set is a PARAM
/// (no `Let`-bound dict anywhere) still carries its kind's helper family, the
/// memory, `$__wasm_str_eq` for a str-valued param's content compare, and the
/// `$__wasm_dict_eq_sv_<k>` twin for a param-hosted str-valued `==`. A miss is
/// a `call` against an undeclared helper — a hard wat2wasm failure.
#[test]
fn param_only_modules_carry_their_helpers() {
    // int-keyed, int-valued: get/has/len — no Let-bound dict in the module.
    let wat = emit(
        "def only_param(d: dict[int, int]) -> int:\n    if 1 in d:\n        return d[1]\n    return d.get(2, -1) + len(d)\n",
    )
    .expect("param-only int module");
    for needle in ["$__wasm_dict_get_i", "$__wasm_dict_has_i", "(memory"] {
        assert!(wat.contains(needle), "int param-only WAT lacks {needle}");
    }

    // str-keyed param: key compares are content compares → $__wasm_str_eq.
    let wat = emit("def only_sk(d: dict[str, int]) -> int:\n    return d.get(\"k\", -1)\n")
        .expect("param-only str-keyed module");
    for needle in ["$__wasm_dict_get_s", "$__wasm_str_eq"] {
        assert!(
            wat.contains(needle),
            "str-keyed param-only WAT lacks {needle}"
        );
    }

    // str-VALUED param hosting `==` over a value read → $__wasm_str_eq via the
    // param-seeded StrEqScan (no Let-bound dict[int, str] in scope).
    let wat = emit(
        "def only_sv(d: dict[int, str]) -> int:\n    if d.get(1, \"?\") == \"x\":\n        return 1\n    return 0\n",
    )
    .expect("param-only str-valued module");
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-valued param-only WAT lacks $__wasm_str_eq (param-seeded StrEqScan miss)"
    );

    // str-VALUED param as an `==` OPERAND → the sv-twin, via the param-seeded
    // module_needs_dict_eq_sv scan.
    let wat = emit(
        "def only_sv_eq(d: dict[int, str]) -> int:\n    m: dict[int, str] = {1: \"a\"}\n    if d == m:\n        return 1\n    return 0\n",
    )
    .expect("param-only sv-eq module");
    assert!(
        wat.contains("$__wasm_dict_eq_sv_i"),
        "param-hosted str-valued `==` must carry the sv-twin"
    );

    // set param: membership + len, keys-only helpers.
    let wat = emit(
        "def only_set(s: set[int]) -> int:\n    if 3 in s:\n        return len(s)\n    return 0\n",
    )
    .expect("param-only set module");
    assert!(
        wat.contains("$__wasm_dict_has_i"),
        "set param-only WAT lacks has helper"
    );
}

// ---- the honest-growth refusal belt --------------------------------------------

/// Every GROWTH op through a dict/set param refuses with the param named —
/// never a silent stale-pointer miscompile.
#[test]
fn growth_through_a_param_refuses() {
    let cases: &[(&str, &str)] = &[
        (
            "subscript store",
            "def f(d: dict[int, int]) -> int:\n    d[1] = 5\n    return 1\n",
        ),
        (
            "setdefault (int lane)",
            "def f(d: dict[int, int]) -> int:\n    x: int = d.setdefault(1, 2)\n    return x\n",
        ),
        (
            "setdefault (str lane)",
            "def f(d: dict[int, str]) -> int:\n    s: str = d.setdefault(1, \"x\")\n    return len(s)\n",
        ),
        (
            "update receiver",
            "def f(d: dict[int, int]) -> int:\n    o: dict[int, int] = {5: 5}\n    d.update(o)\n    return len(d)\n",
        ),
        (
            "set add",
            "def f(s: set[int]) -> int:\n    s.add(7)\n    return len(s)\n",
        ),
    ];
    for (tag, src) in cases {
        let err = emit(src).expect_err(&format!("{tag} through a param must refuse"));
        assert!(
            err.contains("PARAMETER"),
            "{tag}: refusal must name the param posture, got: {err}"
        );
    }
}

/// The growth boundary is exact: the SAME ops on a `Let`-bound local still
/// lower (growth is supported where the dict is bound), and in-place mutation
/// through a param lowers (executed above).
#[test]
fn growth_on_locals_and_inplace_on_params_still_lower() {
    emit("def f() -> int:\n    d: dict[int, int] = {1: 1}\n    d[2] = 2\n    x: int = d.setdefault(3, 3)\n    o: dict[int, int] = {4: 4}\n    d.update(o)\n    return x + len(d)\n")
        .expect("growth on a Let-bound local must keep lowering");
    emit("def f(d: dict[int, int]) -> int:\n    x: int = d.pop(1, -1)\n    if 2 in d:\n        del d[2]\n    d.clear()\n    return x + len(d)\n")
        .expect("in-place mutation through a param must lower");
    emit("def f(s: set[int]) -> int:\n    s.discard(3)\n    return len(s)\n")
        .expect("set discard through a param must lower");
}

// ---- the call-site kind-check belt ----------------------------------------------

/// Kind-mismatched dict/set arguments refuse at the CALL SITE (each would be
/// a silent miscompile at the i32 WAT level).
#[test]
fn kind_mismatched_arguments_refuse() {
    // str-keyed local into an int-keyed param.
    let err = emit(
        "def f(d: dict[int, int]) -> int:\n    return d.get(1, -1)\n\ndef g() -> int:\n    e: dict[str, int] = {\"k\": 1}\n    return f(e)\n",
    )
    .expect_err("key-kind mismatch must refuse");
    assert!(err.contains("keyed"), "key-kind message, got: {err}");

    // str-valued local into an int-valued param.
    let err = emit(
        "def f(d: dict[int, int]) -> int:\n    return d.get(1, -1)\n\ndef g() -> int:\n    e: dict[int, str] = {1: \"a\"}\n    return f(e)\n",
    )
    .expect_err("value-kind mismatch must refuse");
    assert!(
        err.contains("dict[_, str]"),
        "value-kind message, got: {err}"
    );

    // a set local into a dict param.
    let err = emit(
        "def f(d: dict[int, int]) -> int:\n    return d.get(1, -1)\n\ndef g() -> int:\n    s: set[int] = {1, 2}\n    return f(s)\n",
    )
    .expect_err("set-for-dict must refuse");
    assert!(err.contains("set"), "set-vs-dict message, got: {err}");

    // a dict local passed where the callee declares a plain int.
    let err = emit(
        "def f(n: int) -> int:\n    return n\n\ndef g() -> int:\n    d: dict[int, int] = {1: 1}\n    return f(d)\n",
    )
    .expect_err("dict-for-scalar must refuse");
    assert!(
        err.contains("declares no dict/set parameter"),
        "dict-to-scalar message, got: {err}"
    );

    // a dict LITERAL argument (bind it to a local first).
    let err = emit(
        "def f(d: dict[int, int]) -> int:\n    return d.get(1, -1)\n\ndef g() -> int:\n    return f({1: 10})\n",
    )
    .expect_err("dict-literal argument must refuse");
    assert!(err.contains("dict"), "literal-arg message, got: {err}");
}

/// Dict/set params are FREE-function-only: a struct method declaring one
/// refuses at the method registry (its call path bypasses the free-fn
/// kind check).
#[test]
fn method_dict_param_refuses() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, d: dict[int, int]) -> int:\n        return len(d)\n\ndef g() -> int:\n    c: C = C()\n    d: dict[int, int] = {1: 1}\n    return c.m(d)\n",
    )
    .expect_err("method dict param must refuse");
    assert!(
        err.contains("FREE functions only"),
        "method-param message, got: {err}"
    );
}

/// PMAT-1310 lifted the dict-return refusal this test used to pin: a
/// dict-returning free fn + caller-side binding now LOWERS (the caller-side
/// registration story shipped as the return-values slice; executed coverage
/// lives in `dict_return_witness.rs`).
#[test]
fn dict_return_now_lowers() {
    emit(
        "def f() -> dict[int, int]:\n    d: dict[int, int] = {1: 1}\n    return d\n\ndef g() -> int:\n    d: dict[int, int] = f()\n    return len(d)\n",
    )
    .expect("dict return + caller binding must lower since PMAT-1310");
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. The reference-semantics observables
/// (`*_visible`) are the headline: an in-place mutation through the param
/// must be seen by the CALLER's reads after the call.
#[test]
fn dict_param_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1309: skipping EXECUTED dict-param witness — WABT (wat2wasm / \
             wasm-interp) not on PATH; emit-path + refusal pins still ran"
        );
        return;
    }

    let (stdout, ok) = assemble_and_run("corpus", &wat);
    assert!(ok, "wasm-interp failed:\n{stdout}");
    for (name, expected) in PINS {
        let got = parse_scalar(&stdout, name);
        assert_eq!(
            got, *expected,
            "{name}: WASM diverges from the hand-derived pin"
        );
    }

    // Live-CPython cross-check of every pin (zero reimplementation risk: the
    // IDENTICAL source text runs in both lanes).
    if !python3_available() {
        eprintln!("PMAT-1309: python3 not available — pins stand on the hand-derived values");
        return;
    }
    let oracle = python_oracle().expect("oracle must run");
    for (name, expected) in PINS {
        assert_eq!(
            oracle.get(*name).copied(),
            Some(*expected),
            "{name}: hand-derived pin diverges from live CPython — fix the PIN"
        );
    }
}
