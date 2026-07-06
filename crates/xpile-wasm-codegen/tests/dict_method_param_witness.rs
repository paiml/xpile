//! PMAT-1311 — EXECUTED witness for dict/set INSTANCE-METHOD PARAMETERS in
//! the WASM lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): `def m(self, d:
//! dict[int, int]) -> int` lowers with the dict riding the SAME i32
//! base-pointer ABI a free-function dict param does (PMAT-1309) — the first
//! dict/set flow across a METHOD boundary.
//!
//! ## What a method dict/set param supports (executed here, vs CPython)
//!
//! * the read surface through a method: `d[k]` (guarded), `d.get(k,
//!   default)`, `k in d`, `len(d)`, bare-key iteration folds — composed with
//!   `self` field reads in the same body;
//! * IN-PLACE mutation with **caller-visible reference semantics** — `d.pop`
//!   / `s.discard` through the method param never relocate the record, so
//!   the caller observes the mutation through its own pointer (pinned by
//!   reading `len`/`get` IN THE CALLER after the method mutates);
//! * str-KEYED and str-VALUED dict params, set params, a method HANDING its
//!   dict param on to a free function, and the same dict passed to a method
//!   TWICE (both names alias one record).
//!
//! ## What refuses (pinned below)
//!
//! * GROWTH through a method param (`d[k] = v`) — same
//!   `refuse_heap_param_growth` posture as free fns (a grown record
//!   relocates; the caller's pointer would go stale);
//! * call-site KIND mismatches (str-keyed→int-keyed, set→dict, a dict
//!   literal argument, a dict passed where the method declares no dict) —
//!   each would be a silent i32-pointer miscompile, checked against the
//!   method's `<Struct>.<method>`-keyed heap-sig entry;
//! * dict/set params on ASSOCIATED fns (the desugared explicit `__init__`
//!   ctor) — their `Expr::Call` path carries no heap sigs;
//! * dict/set RETURNS from methods (unchanged from PMAT-1310, pinned in
//!   `dict_return_witness.rs`).
//!
//! ## Witness shape
//!
//! ONE module (a class whose methods take dict/set params + zero-arg
//! `check_*` observables; valid plain `python3` AND wasm-frontend-lowerable
//! through the real CLI profile). `wasm-interp --run-all-exports` runs every
//! export — methods included, invoked with ZEROED args (`self = 0`, `d = 0`),
//! so every method is written TOTAL (get-with-default / membership-guarded
//! ops / discard): address 0 holds count=0 in zeroed linear memory and no
//! method traps. Each `check_*` value is pinned to a hand-derived constant
//! AND cross-checked against live `python3` on the IDENTICAL source. Gated
//! on `wasm_runtime_available()` — a clean skip (still asserting the EMIT
//! path + helper carriage) without WABT.

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

/// FULL pipeline: Python source (class + `def`s) → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the executed corpus ----------------------------------------------------

/// `(observable, hand-derived CPython value)` — the oracle re-derives each at
/// runtime, so a wrong constant here fails against BOTH lanes.
const PINS: &[(&str, i64)] = &[
    ("check_method_get", 1007),
    ("check_method_subscript", 329),
    ("check_method_len_contains", 103),
    ("check_method_iter_fold", 10),
    ("check_method_str_key", 88),
    ("check_method_str_val", 5),
    ("check_method_pop_visible", 5001),
    ("check_method_set", 52),
    ("check_method_set_discard_visible", 22),
    ("check_method_handoff", 21),
    ("check_method_same_dict_twice", 64),
    ("check_method_self_state", 1110),
];

/// The single executed module — one class whose methods take dict/set
/// params; every method is TOTAL (no trap under `--run-all-exports`
/// zeroed-arg invocation: `self = 0` reads a zeroed field, `d = 0` reads
/// count = 0).
fn corpus_source() -> String {
    r#"class Acc:
    def __init__(self) -> None:
        self.base: int = 7

    def read(self, d: dict[int, int]) -> int:
        return d.get(10, -3) + self.base

    def sub(self, d: dict[int, int]) -> int:
        if 2 in d:
            return d[2]
        return -1

    def probe(self, d: dict[int, int]) -> int:
        n: int = len(d)
        if 20 in d:
            n = n + 100
        return n

    def drain(self, d: dict[int, int]) -> int:
        return d.pop(30, -1)

    def fold(self, d: dict[int, int]) -> int:
        acc: int = 0
        for k in d:
            acc = acc + k
        for v in d.values():
            acc = acc + v
        return acc

    def sread(self, d: dict[str, int]) -> int:
        return d.get("k", -9)

    def svread(self, d: dict[int, str]) -> int:
        s: str = d.get(5, "")
        return len(s)

    def scard(self, s: set[int]) -> int:
        n: int = len(s)
        if 3 in s:
            n = n + 50
        return n

    def sdrop(self, s: set[int]) -> int:
        s.discard(4)
        return len(s)

    def via(self, d: dict[int, int]) -> int:
        return free_read(d)

    def pair(self, a: dict[int, int], b: dict[int, int]) -> int:
        return a.get(1, -1) * 10 + b.get(2, -1)

    def stateful(self, d: dict[int, int]) -> int:
        self.base = self.base + len(d)
        return d.get(1, -5) + self.base

def free_read(d: dict[int, int]) -> int:
    return d.get(2, -1)

def check_method_get() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {10: 1000, 20: 2}
    return a.read(d)

def check_method_subscript() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {2: 33}
    e: dict[int, int] = {1: 5}
    return a.sub(d) * 10 + a.sub(e)

def check_method_len_contains() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {20: 1, 21: 2, 22: 3}
    return a.probe(d)

def check_method_iter_fold() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {1: 2, 3: 4}
    return a.fold(d)

def check_method_str_key() -> int:
    a: Acc = Acc()
    d: dict[str, int] = {"k": 88, "z": 1}
    return a.sread(d)

def check_method_str_val() -> int:
    a: Acc = Acc()
    d: dict[int, str] = {5: "hello"}
    return a.svread(d)

def check_method_pop_visible() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {30: 500, 31: 6}
    got: int = a.drain(d)
    return got * 10 + len(d)

def check_method_set() -> int:
    a: Acc = Acc()
    s: set[int] = {3, 9}
    return a.scard(s)

def check_method_set_discard_visible() -> int:
    a: Acc = Acc()
    s: set[int] = {4, 8, 12}
    n: int = a.sdrop(s)
    return n * 10 + len(s)

def check_method_handoff() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {2: 21}
    return a.via(d)

def check_method_same_dict_twice() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {1: 6, 2: 4}
    return a.pair(d, d)

def check_method_self_state() -> int:
    a: Acc = Acc()
    d: dict[int, int] = {1: 1000, 2: 8, 3: 9}
    x: int = a.stateful(d)
    return x + a.base * 10
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
            "PMAT-1311: python3 oracle failed:\n{}",
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
        std::env::temp_dir().join(format!("xpile-wasm-dmparam-{}-{}", std::process::id(), tag));
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
/// carries the method-param ABI: dict/set params as `(param $… i32)` on the
/// mangled `$Acc.<method>` functions, both key kinds' helper families, and
/// the memory.
#[test]
fn corpus_emits_with_method_param_abi_and_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // method dict/set params ride i32 base-pointers on mangled fns.
        "(func $Acc.read (param $self i32) (param $d i32)",
        "(func $Acc.scard (param $self i32) (param $s i32)",
        "(func $Acc.pair (param $self i32) (param $a i32) (param $b i32)",
        // helper families for both key kinds (int-keyed reads + str-keyed get).
        "$__wasm_dict_get_i",
        "$__wasm_dict_has_i",
        "$__wasm_dict_pop_i",
        "$__wasm_dict_get_s",
        // str content compare for the str-keyed param's key compares.
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

/// The param-seeded gate-walker pin, method edition: a module whose ONLY
/// dict/set is a METHOD param (no `Let`-bound dict anywhere) still carries
/// its kind's helper family and the memory — `module_dict_key_kinds` seeds
/// from every signature `module_functions` yields, methods included. A miss
/// is a `call` against an undeclared helper — a hard wat2wasm failure.
#[test]
fn method_param_only_modules_carry_their_helpers() {
    let wat = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, d: dict[int, int]) -> int:\n        if 1 in d:\n            return d[1]\n        return d.get(2, -1) + len(d)\n",
    )
    .expect("method-param-only int module");
    for needle in ["$__wasm_dict_get_i", "$__wasm_dict_has_i", "(memory"] {
        assert!(wat.contains(needle), "method-param-only WAT lacks {needle}");
    }
}

// ---- refusal pins ---------------------------------------------------------------

/// GROWTH through a method dict param refuses — a grown record relocates and
/// the caller's base-pointer would go stale (`refuse_heap_param_growth`, the
/// PMAT-1309 posture, now reached through a method body).
#[test]
fn method_param_growth_refuses() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, d: dict[int, int]) -> int:\n        d[9] = 9\n        return len(d)\n\ndef g() -> int:\n    c: C = C()\n    d: dict[int, int] = {1: 1}\n    return c.m(d)\n",
    )
    .expect_err("growth through method param must refuse");
    assert!(
        err.contains("PARAMETER") && err.contains("grow"),
        "growth-refusal message, got: {err}"
    );
}

/// Call-site kind mismatches against the method's declared heap sigs refuse
/// — each would be a silent i32-pointer miscompile.
#[test]
fn method_call_kind_mismatches_refuse() {
    // str-keyed dict passed to an int-keyed method param.
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, d: dict[int, int]) -> int:\n        return len(d)\n\ndef g() -> int:\n    c: C = C()\n    d: dict[str, int] = {\"a\": 1}\n    return c.m(d)\n",
    )
    .expect_err("str-keyed for int-keyed must refuse");
    assert!(
        err.contains("key encoding") || err.contains("keyed"),
        "key-kind message, got: {err}"
    );

    // a set passed to a dict method param.
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, d: dict[int, int]) -> int:\n        return len(d)\n\ndef g() -> int:\n    c: C = C()\n    s: set[int] = {1}\n    return c.m(s)\n",
    )
    .expect_err("set-for-dict must refuse");
    assert!(err.contains("set"), "set-for-dict message, got: {err}");

    // a dict LITERAL argument (bind it to a local first).
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, d: dict[int, int]) -> int:\n        return len(d)\n\ndef g() -> int:\n    c: C = C()\n    return c.m({1: 10})\n",
    )
    .expect_err("dict-literal argument must refuse");
    assert!(err.contains("dict"), "literal-arg message, got: {err}");

    // a dict passed where the method declares NO dict param.
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self, n: int) -> int:\n        return n\n\ndef g() -> int:\n    c: C = C()\n    d: dict[int, int] = {1: 1}\n    return c.m(d)\n",
    )
    .expect_err("dict-for-scalar must refuse");
    assert!(
        err.contains("declares no dict/set parameter"),
        "dict-to-scalar message, got: {err}"
    );
}

/// Dict/set params on ASSOCIATED fns (the desugared explicit `__init__`
/// ctor — no `self` receiver after desugaring) keep refusing: their
/// `Expr::Call` path carries no heap sigs, and an `__init__` storing one
/// would imply a dict-valued FIELD.
#[test]
fn assoc_fn_dict_param_refuses() {
    let err = emit(
        "class C:\n    def __init__(self, d: dict[int, int]) -> None:\n        self.x: int = len(d)\n\ndef g() -> int:\n    d: dict[int, int] = {1: 1}\n    c: C = C(d)\n    return c.x\n",
    )
    .expect_err("assoc-fn dict param must refuse");
    assert!(
        err.contains("free functions and instance methods only") || err.contains("dict"),
        "assoc-param message, got: {err}"
    );
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. The `*_visible` observables are the
/// headline: an in-place mutation through the METHOD param must be seen by
/// the CALLER's reads after the call.
#[test]
fn dict_method_param_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1311: skipping EXECUTED method-param witness — WABT (wat2wasm / \
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
        eprintln!("PMAT-1311: python3 not available — pins stand on the hand-derived values");
        return;
    }
    let oracle = python_oracle().expect("oracle must run");
    for (name, expected) in PINS {
        assert_eq!(
            oracle.get(*name).copied(),
            Some(*expected),
            "{name}: the hand-derived pin diverges from live CPython"
        );
    }
}
