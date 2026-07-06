//! PMAT-1312 — EXECUTED witness for dict/set INSTANCE-METHOD RETURNS in the
//! WASM lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): `def make(self) ->
//! dict[int, int]` lowers with the result riding the SAME i32 base-pointer
//! ABI a free-function dict/set return does (PMAT-1310) — closing the
//! "method dict RETURNS refused" leg PMAT-1311 (method params) left open,
//! completing the dict/set METHOD boundary in both directions.
//!
//! ## Why a METHOD return is as sound as a free-fn return
//!
//! The PMAT-1310 aliasing argument carries over whole: the returned record
//! must never alias a caller-visible name, and a method has exactly one
//! channel a free fn lacks — `self`. But `self` cannot smuggle a dict/set
//! (struct layouts refuse dict/set FIELDS), the method's `return` refuses
//! its dict/set PARAMS (the generic `emit_heap_ret` belt — method params
//! register in `heap_map_params` since PMAT-1311), and name copies don't
//! exist in-lane. So every returned name is a method-LOCAL fresh record;
//! caller-side GROWTH on it stays the sanctioned escape hatch
//! (`check_caller_growth` outruns the 16-slot slack to pin relocation).
//!
//! ## Why the slice was small (the third Let/no-Let case, inverted twice)
//!
//! The callee side was ALREADY generic — methods emit through the same
//! `emit_function` that computes `ret_heap` from the declared return type,
//! so every `return` site kind-checks via `emit_heap_ret` with zero new
//! code. The caller binds via the ORDINARY `Let` ty (the frontend's
//! `infer_type_in_ctx` types `d = b.make()` from the method signature), so
//! every Let-ty-driven walker sees method-call-bound dicts for FREE — the
//! PMAT-1310 lesson verbatim. The whole slice: `callable_ret` lifts the
//! refusal for `self`-receiver methods (assoc fns keep refusing — their
//! call path carries no heap sigs), `emit_heap_map_bind` grows a
//! `MethodCall` arm (kind triple checked against the `<Struct>.<method>`
//! heap-sig ret leg PMAT-1311 already populated), and the value-position
//! guard extends to method calls.
//!
//! ## What executes here (value-matched vs CPython)
//!
//! * factories reading `self` state (two INSTANCES with different state
//!   produce different records — `check_two_objects`);
//! * callee-side growth PAST the literal slack (relocation before return)
//!   and caller-side growth on the received record;
//! * dict IN and OUT of ONE method call (`b.shifted(d)` — param read +
//!   fresh result), a method composing `self` state AND a dict param into
//!   the result, and a method-returned record handed BACK IN as a method
//!   param (`check_out_then_in`);
//! * EARLY returns (branch-selected records; mutate-after-early-return);
//! * str-keyed / str-VALUED / `dict[str, str]` / `set[int]` returns, read
//!   through the caller's full surface — subscript / `get` / `len` / `in` /
//!   `==` (both sv-twins over RETURNED records);
//! * a statement-position discarded dict-returning method call.
//!
//! ## What refuses (pinned below)
//!
//! Returning a method's dict/set PARAM (the aliasing channel — generic
//! `emit_heap_ret`, method params included since PMAT-1311), returning a
//! literal directly (bind to a local first), a dict-returning method call
//! consumed in a VALUE position (`len(b.make())` — bind it first), and
//! growth through a method param UNCHANGED alongside the return leg. The
//! assoc-fn dict-return refusal (`callable_ret`) is defense-in-depth — a
//! Python `__init__` returns `None`/`Self`, so it is not reachable from
//! Python source and not pinned here (other frontends construct mHIR
//! directly). Backend kind-mismatch checks at the binding (set-vs-dict, key
//! kind, value kind) are likewise behind the frontend's declared-vs-produced
//! check.
//!
//! ## Witness shape
//!
//! Mirrors `dict_method_param_witness.rs`: ONE module (valid plain `python3`
//! AND wasm-frontend-lowerable through the real CLI profile); `wasm-interp
//! --run-all-exports` invokes every export with ZEROED args, so every method
//! is TOTAL under `self = 0` (field reads/writes at address ~0 land in the
//! reserved host region below `LITERAL_BASE`; a zeroed dict param reads its
//! count wherever addr 0 points — param-taking methods use `get`-with-default
//! only). Each `check_*` is pinned to a hand-derived constant AND
//! cross-checked against live `python3` on the IDENTICAL source. Gated on
//! `wasm_runtime_available()` — a clean skip (still asserting the EMIT path +
//! refusals) without WABT.

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

/// FULL pipeline: Python source (a class + free `def`s) → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the executed corpus ----------------------------------------------------

/// `(observable, hand-derived CPython value)` — the oracle re-derives each at
/// runtime, so a wrong constant here fails against BOTH lanes.
const PINS: &[(&str, i64)] = &[
    ("check_make_read", 43),
    ("check_callee_growth", 40117),
    ("check_caller_growth", 39049),
    ("check_in_and_out", 1142),
    ("check_self_and_param", 151),
    ("check_out_then_in", 202),
    ("check_early_return", 111222),
    ("check_early_then_mutate", 12),
    ("check_two_objects", 1011),
    ("check_str_key", 412),
    ("check_str_val", 51),
    ("check_set_ret", 21),
    ("check_ss_eq", 1),
    ("check_discard_stmt", 1),
];

/// The single executed module — a class whose methods RETURN dicts/sets;
/// every export TOTAL (no trap under `--run-all-exports` zeroed-arg
/// invocation, `self = 0` included).
fn corpus_source() -> String {
    r#"class Builder:
    def __init__(self) -> None:
        self.base: int = 10

    def bump(self) -> None:
        self.base = self.base + 1

    def make(self) -> dict[int, int]:
        d: dict[int, int] = {1: self.base, 2: self.base * 2}
        d[3] = 30
        return d

    def make_big(self) -> dict[int, int]:
        d: dict[int, int] = {0: 0}
        i: int = 1
        while i < 40:
            d[i] = i * 3
            i = i + 1
        return d

    def seed(self) -> dict[int, int]:
        d: dict[int, int] = {1: self.base}
        return d

    def shifted(self, src: dict[int, int]) -> dict[int, int]:
        r: dict[int, int] = {}
        r[1] = src.get(1, 0) * 2
        r[2] = src.get(2, 0) * 2
        return r

    def boosted(self, src: dict[int, int]) -> dict[int, int]:
        r: dict[int, int] = {}
        r[1] = src.get(1, 0) + self.base
        return r

    def pick(self, flag: int) -> dict[int, int]:
        a: dict[int, int] = {1: 111}
        b: dict[int, int] = {1: 222}
        if flag > 0:
            return a
        return b

    def pick2(self, flag: int) -> dict[int, int]:
        a: dict[int, int] = {1: 1}
        if flag > 0:
            return a
        a[2] = 2
        return a

    def names(self) -> dict[int, str]:
        d: dict[int, str] = {1: "alice"}
        d[2] = "bo"
        return d

    def ages(self) -> dict[str, int]:
        d: dict[str, int] = {"amy": self.base * 3}
        d["bob"] = 41
        return d

    def tags(self) -> set[int]:
        s: set[int] = {3, 5}
        return s

    def pair_ss(self) -> dict[str, str]:
        d: dict[str, str] = {"k": "vv"}
        return d

def check_make_read() -> int:
    b: Builder = Builder()
    d = b.make()
    return d[3] + len(d) + d[1]

def check_callee_growth() -> int:
    b: Builder = Builder()
    d = b.make_big()
    return len(d) * 1000 + d[39]

def check_caller_growth() -> int:
    b: Builder = Builder()
    d = b.seed()
    i: int = 2
    while i < 40:
        d[i] = i
        i = i + 1
    return len(d) * 1000 + d[39] + d[1]

def check_in_and_out() -> int:
    b: Builder = Builder()
    d: dict[int, int] = {1: 5, 2: 7}
    e = b.shifted(d)
    return e[1] * 100 + e[2] * 10 + len(d)

def check_self_and_param() -> int:
    b: Builder = Builder()
    d: dict[int, int] = {1: 5}
    e = b.boosted(d)
    return e[1] * 10 + len(e)

def check_out_then_in() -> int:
    b: Builder = Builder()
    d = b.seed()
    e = b.shifted(d)
    return e[1] * 10 + len(e)

def check_early_return() -> int:
    b: Builder = Builder()
    x = b.pick(1)
    y = b.pick(0)
    return x[1] * 1000 + y[1]

def check_early_then_mutate() -> int:
    b: Builder = Builder()
    x = b.pick2(1)
    y = b.pick2(0)
    return len(x) * 10 + len(y)

def check_two_objects() -> int:
    b: Builder = Builder()
    c: Builder = Builder()
    c.bump()
    x = b.seed()
    y = c.seed()
    return x[1] * 100 + y[1]

def check_str_key() -> int:
    b: Builder = Builder()
    d = b.ages()
    return d["bob"] * 10 + len(d)

def check_str_val() -> int:
    b: Builder = Builder()
    d = b.names()
    m: dict[int, str] = {2: "bo", 1: "alice"}
    if d == m:
        return len(d.get(1, "?")) * 10 + 1
    return 0

def check_set_ret() -> int:
    b: Builder = Builder()
    s = b.tags()
    if 3 in s:
        return len(s) * 10 + 1
    return 0

def check_ss_eq() -> int:
    b: Builder = Builder()
    d = b.pair_ss()
    m: dict[str, str] = {"k": "vv"}
    if d == m:
        return 1
    return 0

def check_discard_stmt() -> int:
    b: Builder = Builder()
    b.seed()
    d = b.seed()
    return len(d)
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
            "PMAT-1312: python3 oracle failed:\n{}",
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-mret-{}-{}", std::process::id(), tag));
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
/// carries the method-return ABI: dict/set-returning METHODS as `(param
/// $self i32) (result i32)`, the helper families for every kind flowing
/// through a method return, and the sv-twins for `==` over RETURNED records.
#[test]
fn corpus_emits_with_method_return_abi_and_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // dict/set method returns ride i32 base-pointers: the self-state
        // factory, the dict-in-dict-out method, and a set-returning method.
        "(func $Builder.make (param $self i32) (result i32)",
        "(func $Builder.shifted (param $self i32) (param $src i32) (result i32)",
        "(func $Builder.tags (param $self i32) (result i32)",
        // helper families for both key kinds.
        "$__wasm_dict_get_i",
        "$__wasm_dict_get_s",
        "$__wasm_dict_set_i",
        "$__wasm_dict_has_i",
        // dict equality over method-RETURNED records: int-keyed sv-twin
        // (check_str_val) AND str-keyed sv-twin (check_ss_eq).
        "$__wasm_dict_eq_sv_i",
        "$__wasm_dict_eq_sv_s",
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

// ---- the refusal belt -----------------------------------------------------------

/// Returning a method's dict/set PARAM refuses: the caller already holds
/// that record under the argument's name — the exact aliasing channel the
/// free-fn return refusal keeps closed (PMAT-1310), reached through the
/// generic `emit_heap_ret` belt because method params register in
/// `heap_map_params` (PMAT-1311).
#[test]
fn returning_a_method_param_refuses() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def echo(self, d: dict[int, int]) -> dict[int, int]:\n        return d\n\ndef g() -> int:\n    c: C = C()\n    return c.x\n",
    )
    .expect_err("returning a method's dict param must refuse");
    assert!(
        err.contains("PARAMETER"),
        "method-param-return refusal must name the posture, got: {err}"
    );
}

/// A method's dict/set return must be a LOCAL NAME — a literal in return
/// position refuses (bind it first; same rule as free fns).
#[test]
fn returning_a_non_name_from_method_refuses() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def make(self) -> dict[int, int]:\n        return {1: 2}\n\ndef g() -> int:\n    c: C = C()\n    return c.x\n",
    )
    .expect_err("returning a dict literal from a method must refuse");
    assert!(
        err.contains("LOCAL name"),
        "literal-return refusal, got: {err}"
    );
}

/// A dict-returning METHOD call consumed in a VALUE position (not a dict/set
/// binding) refuses — the pointer must register under a name before any
/// keyed/len/eq use (the PMAT-1310 free-fn guard, extended to methods).
#[test]
fn method_call_result_in_value_position_refuses() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def make(self) -> dict[int, int]:\n        d: dict[int, int] = {1: 2}\n        return d\n\ndef use() -> int:\n    c: C = C()\n    return len(c.make())\n",
    )
    .expect_err("len(c.make()) must refuse");
    assert!(
        err.contains("outside a dict/set binding") || err.contains("len"),
        "value-position refusal, got: {err}"
    );
}

/// The growth boundary is unchanged by the method-return leg: growth through
/// a method PARAM still refuses even in a method that legally returns a
/// fresh dict (the sanctioned alternative: grow the RESULT, not the param).
#[test]
fn method_param_growth_still_refuses_alongside_returns() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def f(self, d: dict[int, int]) -> dict[int, int]:\n        d[1] = 5\n        r: dict[int, int] = {**d}\n        return r\n\ndef g() -> int:\n    c: C = C()\n    return c.x\n",
    )
    .expect_err("method param growth must still refuse");
    assert!(err.contains("PARAMETER"), "growth refusal, got: {err}");
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. The headline observables:
/// `check_two_objects` (per-INSTANCE state flows into the returned record —
/// the capability free-fn returns cannot express), `check_callee_growth` /
/// `check_caller_growth` (relocation before return + growth on the received
/// record, both past the literal slack), and `check_out_then_in` (a
/// method-returned record handed back in as a method param).
#[test]
fn dict_method_return_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1312: skipping EXECUTED dict-method-return witness — WABT \
             (wat2wasm / wasm-interp) not on PATH; emit-path + refusal pins \
             still ran"
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
        eprintln!("PMAT-1312: python3 not available — pins stand on the hand-derived values");
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
