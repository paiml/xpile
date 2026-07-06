//! PMAT-1310 — EXECUTED witness for dict/set RETURN VALUES in the WASM lane
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): `def make() -> dict[int, int]`
//! lowers with the result riding the SAME i32 base-pointer ABI a str/struct
//! return does (PMAT-993/1023) — the first dict/set flowing OUT of a function
//! boundary, closing the loop PMAT-1309's params opened (flow IN).
//!
//! ## Why a dict/set return is SOUND where growth-through-param refused
//!
//! The bump heap lives in MODULE-GLOBAL linear memory, so a record a callee
//! builds survives the return — the returned base-pointer is simply handed to
//! the caller's binding. Growth INSIDE the callee before the return is safe
//! (a relocation lands in the callee's local; the FINAL pointer is what is
//! returned — `check_callee_growth` outruns the 16-slot literal slack to pin
//! the relocated case). Growth in the CALLER on the received record is safe
//! too, because the result NEVER aliases another caller-visible name: the
//! callee's `return` refuses PARAMS (the one shape that would hand the caller
//! a second name for a record it already holds) and the lane has no dict name
//! copies. `check_caller_growth` pins exactly the op family that refuses
//! through a param.
//!
//! ## What executes here (value-matched vs CPython)
//!
//! * zero-arg factories: int-keyed, str-keyed, str-VALUED, `dict[str, str]`,
//!   and `set[int]` returns, read through the caller's full surface —
//!   subscript / `get` / `len` / `in` / `==` (including the content-comparing
//!   sv-twin over a RETURNED str-valued dict);
//! * callee-side growth PAST the literal slack (relocation before return) and
//!   caller-side growth on the received record;
//! * dict flowing IN and OUT of ONE call (`shifted(src)` — param read + fresh
//!   result), and the `{**src}` COPY of a param as the sanctioned alternative
//!   to returning the param itself (mutating the copy leaves the original
//!   untouched — pinned);
//! * EARLY returns (branch-selected records; also mutate-after-early-return),
//!   a set-algebra result bound then returned, and a statement-position
//!   discarded dict-returning call.
//!
//! ## What refuses (pinned below)
//!
//! Returning a PARAM (the aliasing channel), returning a literal / set-algebra
//! expression directly (bind to a local first — the return must be a NAME so
//! the kind triple is known), a dict/set return on a struct METHOD (free
//! functions only, like params), and a dict-returning call consumed in a
//! value position (`len(make())` — bind it first). Backend kind-mismatch
//! checks (returning a dict where a set is declared, binding a dict[_, str]
//! result to a dict[_, int] local, …) are defense-in-depth behind the
//! frontend's declared-vs-produced type check and are not reachable from
//! Python source, so they are not pinned here.
//!
//! ## Witness shape
//!
//! Mirrors `dict_param_witness.rs`: ONE module of standalone `def`s (valid
//! plain `python3` AND wasm-frontend-lowerable through the real CLI profile);
//! `wasm-interp --run-all-exports` invokes every export with ZEROED args, so
//! every helper is TOTAL (a zeroed dict param reads count=0 at address 0 —
//! get-with-default only). Each `check_*` is pinned to a hand-derived
//! constant AND cross-checked against live `python3` on the IDENTICAL source.
//! Gated on `wasm_runtime_available()` — a clean skip (still asserting the
//! EMIT path + helper carriage) without WABT.

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
    ("check_make_read", 33),
    ("check_callee_growth", 40117),
    ("check_caller_growth", 39049),
    ("check_in_and_out", 1142),
    ("check_early_return", 111222),
    ("check_str_key", 412),
    ("check_str_val", 51),
    ("check_set_ret", 21),
    ("check_ss_eq", 1),
    ("check_copy_of_param", 241),
    ("check_setalg_ret", 3),
    ("check_early_then_mutate", 12),
    ("check_discard_stmt", 1),
];

/// The single executed module — factories return dicts/sets; every export is
/// TOTAL (no trap under `--run-all-exports` zeroed-arg invocation).
fn corpus_source() -> String {
    r#"def make_counts() -> dict[int, int]:
    d: dict[int, int] = {1: 10, 2: 20}
    d[3] = 30
    return d

def check_make_read() -> int:
    d = make_counts()
    return d[3] + len(d)

def make_big() -> dict[int, int]:
    d: dict[int, int] = {0: 0}
    i: int = 1
    while i < 40:
        d[i] = i * 3
        i = i + 1
    return d

def check_callee_growth() -> int:
    d = make_big()
    return len(d) * 1000 + d[39]

def make_seed() -> dict[int, int]:
    d: dict[int, int] = {1: 10}
    return d

def check_caller_growth() -> int:
    d = make_seed()
    i: int = 2
    while i < 40:
        d[i] = i
        i = i + 1
    return len(d) * 1000 + d[39] + d[1]

def shifted(src: dict[int, int]) -> dict[int, int]:
    r: dict[int, int] = {}
    r[1] = src.get(1, 0) * 2
    r[2] = src.get(2, 0) * 2
    return r

def check_in_and_out() -> int:
    d: dict[int, int] = {1: 5, 2: 7}
    e = shifted(d)
    return e[1] * 100 + e[2] * 10 + len(d)

def pick(flag: int) -> dict[int, int]:
    a: dict[int, int] = {1: 111}
    b: dict[int, int] = {1: 222}
    if flag > 0:
        return a
    return b

def check_early_return() -> int:
    x = pick(1)
    y = pick(0)
    return x[1] * 1000 + y[1]

def make_ages() -> dict[str, int]:
    d: dict[str, int] = {"amy": 30}
    d["bob"] = 41
    return d

def check_str_key() -> int:
    d = make_ages()
    return d["bob"] * 10 + len(d)

def make_names() -> dict[int, str]:
    d: dict[int, str] = {1: "alice"}
    d[2] = "bo"
    return d

def check_str_val() -> int:
    d = make_names()
    m: dict[int, str] = {2: "bo", 1: "alice"}
    if d == m:
        return len(d.get(1, "?")) * 10 + 1
    return 0

def make_set() -> set[int]:
    s: set[int] = {3, 5}
    return s

def check_set_ret() -> int:
    s = make_set()
    if 3 in s:
        return len(s) * 10 + 1
    return 0

def make_pair_ss() -> dict[str, str]:
    d: dict[str, str] = {"k": "vv"}
    return d

def check_ss_eq() -> int:
    d = make_pair_ss()
    m: dict[str, str] = {"k": "vv"}
    if d == m:
        return 1
    return 0

def snapshot(src: dict[int, int]) -> dict[int, int]:
    r: dict[int, int] = {**src}
    return r

def check_copy_of_param() -> int:
    d: dict[int, int] = {1: 7, 2: 9}
    e = snapshot(d)
    e[3] = 11
    return len(d) * 100 + len(e) * 10 + e.get(3, -1) - d.get(3, 0)

def combine() -> set[int]:
    a: set[int] = {1, 2}
    b: set[int] = {2, 3}
    c: set[int] = a | b
    return c

def check_setalg_ret() -> int:
    s = combine()
    return len(s)

def pick2(flag: int) -> dict[int, int]:
    a: dict[int, int] = {1: 1}
    if flag > 0:
        return a
    a[2] = 2
    return a

def check_early_then_mutate() -> int:
    x = pick2(1)
    y = pick2(0)
    return len(x) * 10 + len(y)

def check_discard_stmt() -> int:
    make_seed()
    d = make_seed()
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
            "PMAT-1310: python3 oracle failed:\n{}",
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dret-{}-{}", std::process::id(), tag));
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
/// carries the return ABI: dict/set-returning fns as `(result i32)`, the
/// helper families for every kind flowing through a return, and the sv-twins
/// for `==` over RETURNED str-valued dicts.
#[test]
fn corpus_emits_with_return_abi_and_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // dict/set returns ride i32 base-pointers (zero-arg factory + the
        // dict-in-dict-out shape + a set-returning fn).
        "(func $make_counts (result i32)",
        "(func $shifted (param $src i32) (result i32)",
        "(func $combine (result i32)",
        // helper families for both key kinds.
        "$__wasm_dict_get_i",
        "$__wasm_dict_get_s",
        "$__wasm_dict_set_i",
        "$__wasm_set_union_i",
        // dict equality over RETURNED records: int-keyed sv-twin
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

/// Returning a dict/set PARAM refuses: the caller already holds that record
/// under the argument's name, so the return would create a second
/// caller-visible name for one record — the exact aliasing channel the
/// growth-refusal keeps closed.
#[test]
fn returning_a_param_refuses() {
    let err = emit("def echo(d: dict[int, int]) -> dict[int, int]:\n    return d\n")
        .expect_err("returning a dict param must refuse");
    assert!(
        err.contains("PARAMETER"),
        "param-return refusal must name the posture, got: {err}"
    );
    let err = emit("def echo_s(s: set[int]) -> set[int]:\n    return s\n")
        .expect_err("returning a set param must refuse");
    assert!(err.contains("PARAMETER"), "set param-return, got: {err}");
}

/// A dict/set return must be a LOCAL NAME — a literal or a set-algebra
/// expression in return position refuses (bind it first, so the kind triple
/// is known and the record provably fresh).
#[test]
fn returning_a_non_name_refuses() {
    let err = emit("def make() -> dict[int, int]:\n    return {1: 2}\n")
        .expect_err("returning a dict literal must refuse");
    assert!(
        err.contains("LOCAL name"),
        "literal-return refusal, got: {err}"
    );
    let err = emit(
        "def combine() -> set[int]:\n    a: set[int] = {1}\n    b: set[int] = {2}\n    return a | b\n",
    )
    .expect_err("returning a set-algebra expression must refuse");
    assert!(
        err.contains("LOCAL name"),
        "set-algebra-return refusal, got: {err}"
    );
}

/// Dict/set returns are FREE-function-only, like params: a struct method
/// declaring one refuses at the method registry.
#[test]
fn method_dict_return_refuses() {
    let err = emit(
        "class C:\n    def __init__(self) -> None:\n        self.x: int = 1\n    def m(self) -> dict[int, int]:\n        d: dict[int, int] = {1: 1}\n        return d\n\ndef g() -> int:\n    c: C = C()\n    return c.x\n",
    )
    .expect_err("method dict return must refuse");
    assert!(
        err.contains("method `m` return type"),
        "method-return message, got: {err}"
    );
}

/// A dict-returning call consumed in a VALUE position (not a dict/set
/// binding) refuses — the pointer must register under a name before any
/// keyed/len/eq use.
#[test]
fn call_result_in_value_position_refuses() {
    let err = emit(
        "def make() -> dict[int, int]:\n    d: dict[int, int] = {1: 2}\n    return d\n\ndef use() -> int:\n    return len(make())\n",
    )
    .expect_err("len(make()) must refuse");
    assert!(err.contains("len"), "len-of-call refusal, got: {err}");
}

/// The growth boundary is unchanged by the return leg: growth through a PARAM
/// still refuses even in a function that legally returns a fresh dict.
#[test]
fn param_growth_still_refuses_alongside_returns() {
    let err = emit(
        "def f(d: dict[int, int]) -> dict[int, int]:\n    d[1] = 5\n    r: dict[int, int] = {**d}\n    return r\n",
    )
    .expect_err("param growth must still refuse");
    assert!(err.contains("PARAMETER"), "growth refusal, got: {err}");
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. The growth observables are the
/// headline: relocation INSIDE the callee before the return
/// (`check_callee_growth` outruns the literal slack) and caller-side growth
/// on the received record (`check_caller_growth` — the op family that refuses
/// through a param is sound through a return).
#[test]
fn dict_return_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1310: skipping EXECUTED dict-return witness — WABT (wat2wasm / \
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
        eprintln!("PMAT-1310: python3 not available — pins stand on the hand-derived values");
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
