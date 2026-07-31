//! PMAT-1318 — ADVERSARIAL-VERIFY differential witness over the comprehension
//! and hash-source-store surface: PMAT-1314 dict-iteration stores, PMAT-1315
//! set-BUILD inserts, PMAT-1316 set COMPREHENSIONS over dict/set sources,
//! PMAT-1317 dict COMPREHENSIONS, and the shared `comp_self_reference_belt`.
//! This is the scheduled ~4-slices-since-PMAT-1313 skeptic pass: the four
//! preceding slices each shipped a "REAL CORRECTNESS FIND" (the
//! self-reference clobber belt hoisted across all three desugars), so the
//! belt and the order-gate are the highest-EV place to look for a
//! hollow/always-empty miscompile or a refusal that silently doesn't fire.
//!
//! ## What the pass did (and its verdict)
//!
//! ~30 adversarial comprehension modules were driven END-TO-END —
//! `python3 → xpile --target wasm → wat2wasm → wasm-interp` — and
//! value-diffed against LIVE CPython on the IDENTICAL source. VERDICT:
//! **NOTHING REFUTED.** Every claimed capability value-matches CPython;
//! every claimed refusal (belt self-reference + order-gate) refuses. The
//! lone apparent mismatch during the sweep was a TEST artifact — a fold
//! `acc = acc * 100000 + …` over 40 keys overflows i64, and the WASM lane
//! wraps where CPython uses bignum (the pre-existing, documented
//! C-PY-INT-ARITH limitation, NOT a comprehension defect); every executed
//! pin below is bounded under i64 by construction (`c_big_growth` /
//! `c_scrambled_set` fold via a bounded permutation-invariant sum).
//!
//! ## Executed corpus (17 admitted cases, value-matched vs CPython)
//!
//! One int-keyed module + one str-keyed module, each self-contained valid
//! `python3` AND wasm-frontend-lowerable. Every `c_*` observable is pinned
//! to a hand-derived constant AND cross-checked against live `python3` on
//! the IDENTICAL source, so a wrong constant here fails against BOTH lanes.
//! Content observables are permutation-invariant (base-N folds over a BOUND
//! `sorted(...)` key list, or a commutative sum), so the CPython cross-check
//! cannot flap on hash/storage order — `c_scrambled_set` deliberately
//! `discard`s into a hole + re-`add`s so bump-heap storage order provably
//! diverges from CPython's, yet the content pin holds.
//!
//! Coverage: bare-dict / `.keys()` / `.values()` / `.items()` sources; set
//! sources; filtered comps; the shadow carve-out (`{r: r+100 for r in u}`
//! and `{t*2 for t in u}` — the loop var, not the destination, is what the
//! key/value/element read); set-BUILD stores (cross-product `t.add(a+b)`,
//! degenerate `t.add(7)`); the source-read value `{k: src[k]*k for k in
//! src}` (PMAT-1314's `DictGet` pure-read widening); growth past the
//! 16-slot literal slack; str keys + `dict[str, str]`; and the full
//! `transform(src)` boundary pipeline (dict IN → comp build → dict OUT,
//! called twice, per-call-fresh).
//!
//! ## Refusal corpus (the belt + the order-gate have teeth)
//!
//! * BELT (frontend refusal — `comp_self_reference_belt`): a comprehension
//!   reading its own destination through the ITERABLE (`{v: v for v in
//!   d.values()}`), a FILTER, the VALUE (`{k: v + d[k] for k, v in
//!   d.items()}`), a nested inner comp, or a 2-generator inner iterable /
//!   element — each refuses at `parse_and_lower` naming the clobber. Were
//!   the belt removed, these EMIT and silently read the freshly-materialised
//!   empty destination (`{}` where CPython keeps the pre-assignment value) —
//!   the exact silent-wrong PMAT-1316/1317 found live.
//! * ORDER-GATE (backend refusal — the PMAT-1292 hash-order whitelist): a
//!   store keyed by a constant (`r[5] = k`), a computed key (`r[k*2]`), a
//!   call in the value, a v-keyed `.items()` swap (last-write-wins), a
//!   non-commutative fold, and a comp over a dict MUTATED elsewhere in the
//!   function — each refuses at `emit_module`.
//!
//! ## Gating
//!
//! The executed diff needs WABT (`wat2wasm` / `wasm-interp`) AND `python3`;
//! without either it skips cleanly after asserting the EMIT path + every
//! refusal pin (which run everywhere, no WABT needed).

use std::collections::{BTreeMap, BTreeSet};
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

/// FULL pipeline: Python source → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the executed admitted corpus ------------------------------------------

/// Shared int-keyed folds + `transform`, then every int-keyed `c_*` case.
/// Under `--run-all-exports` the param-taking `fold_dict` / `fold_set` /
/// `transform` are zero-invoked with a null (addr-0, count-0) heap pointer,
/// so their loops never run and they stay TOTAL.
fn admitted_int_module() -> &'static str {
    r#"def fold_dict(d: dict[int, int]) -> int:
    xs = sorted(d)
    acc: int = 0
    for y in xs:
        acc = acc * 100000 + y * 1000 + d[y]
    return acc

def fold_set(s: set[int]) -> int:
    xs = sorted(s)
    acc: int = 0
    for y in xs:
        acc = acc * 1000 + y
    return acc

def transform(src: dict[int, int]) -> dict[int, int]:
    r = {k: src[k] * 2 for k in src}
    return r

def c_dict_keys() -> int:
    d: dict[int, int] = {3: 1, 5: 2, 7: 3}
    r = {k: k * 2 for k in d}
    return len(r) * 1000000000000 + fold_dict(r)

def c_src_read_value() -> int:
    src: dict[int, int] = {1: 5, 2: 6, 3: 7}
    r = {k: src[k] * k for k in src}
    return fold_dict(r)

def c_set_source() -> int:
    s: set[int] = {1, 2, 3}
    r = {x: x * 3 for x in s}
    return fold_dict(r)

def c_filter_srcread() -> int:
    src: dict[int, int] = {1: 5, 2: 6, 3: 7, 4: 8}
    r = {k: k for k in src if src[k] > 6}
    return fold_dict(r)

def c_values_distinct() -> int:
    d: dict[int, int] = {1: 7, 2: 7, 3: 9}
    r = {v: v * 10 for v in d.values()}
    return fold_dict(r)

def c_items_paired() -> int:
    d: dict[int, int] = {5: 1, 6: 2, 7: 3}
    r = {k: k * 100 + v for k, v in d.items()}
    return fold_dict(r)

def c_dict_shadow() -> int:
    u = [3, 1, 2]
    r = {r: r + 100 for r in u}
    return fold_dict(r)

def c_set_shadow() -> int:
    u = [1, 2, 3, 2, 1]
    t = {t * 2 for t in u}
    return fold_set(t)

def c_setcomp_items() -> int:
    d: dict[int, int] = {1: 10, 2: 20, 3: 30}
    t = {v + k for k, v in d.items()}
    return fold_set(t)

def c_setcomp_dict_filt() -> int:
    d: dict[int, int] = {1: 10, 2: 20, 3: 30, 4: 40}
    t = {k * 2 for k in d if d[k] > 20}
    return fold_set(t)

def c_set_crossprod() -> int:
    a = [1, 2, 3]
    b = [1, 2, 3]
    t: set[int] = set()
    for x in a:
        for y in b:
            t.add(x + y)
    return fold_set(t)

def c_set_const_add() -> int:
    s: set[int] = {1, 2, 3}
    t: set[int] = set()
    for x in s:
        t.add(7)
    return fold_set(t)

def c_scrambled_set() -> int:
    s: set[int] = {1, 2, 3, 4, 5}
    s.discard(2)
    s.discard(4)
    s.add(8)
    s.add(9)
    r = {x: x + 1 for x in s}
    tot: int = 0
    for k in r:
        tot = tot + k * 1000 + r[k]
    return len(r) * 1000000 + tot

def c_big_growth() -> int:
    s: set[int] = set()
    i: int = 0
    while i < 40:
        s.add(i)
        i = i + 1
    r = {x: x + 1 for x in s}
    tot: int = 0
    for k in r:
        tot = tot + k * r[k]
    return len(r) * 1000000 + tot

def c_pipeline() -> int:
    d1: dict[int, int] = {1: 7, 2: 9}
    d2: dict[int, int] = {5: 3}
    r1 = transform(d1)
    r2 = transform(d2)
    return fold_dict(r1) * 1000000 + fold_dict(r2)
"#
}

/// Str-keyed cases probe content via membership (`in` / `d[k]` / `==`) — a
/// `sorted(str-dict)` would yield `list[str]`, which the WASM list subset
/// refuses, so these deliberately avoid it.
fn admitted_str_module() -> &'static str {
    r#"def c_str_keys() -> int:
    d: dict[str, int] = {"aa": 1, "bb": 2, "cc": 3}
    r = {k: d[k] * 10 for k in d}
    out: int = len(r) * 1000
    if "aa" in r:
        out = out + r["aa"]
    if "cc" in r:
        out = out + r["cc"]
    return out

def c_str_str() -> int:
    d: dict[str, str] = {"a": "x", "b": "y"}
    r = {k: d[k] for k in d}
    out: int = len(r) * 100
    if "a" in r:
        out = out + 10
    if r["a"] == "x":
        out = out + 5
    if r["b"] == "y":
        out = out + 3
    return out
"#
}

/// `(observable, hand-derived CPython value)`. The oracle re-derives each at
/// runtime from the IDENTICAL source, so a wrong constant fails BOTH lanes.
/// Every value is bounded under i64 (no bignum overflow — see the module
/// doc); folds are permutation-invariant.
const INT_PINS: &[(&str, i64)] = &[
    ("c_dict_keys", 33060501007014),
    ("c_src_read_value", 10050201203021),
    ("c_set_source", 10030200603009),
    ("c_filter_srcread", 300304004),
    ("c_values_distinct", 707009090),
    ("c_items_paired", 55010660207703),
    ("c_dict_shadow", 11010210203103),
    ("c_set_shadow", 2004006),
    ("c_setcomp_items", 11022033),
    ("c_setcomp_dict_filt", 6008),
    ("c_set_crossprod", 2003004005006),
    ("c_set_const_add", 7),
    ("c_scrambled_set", 5026031),
    ("c_big_growth", 40021320),
    ("c_pipeline", 101402018005006),
];

const STR_PINS: &[(&str, i64)] = &[("c_str_keys", 3040), ("c_str_str", 218)];

// ---- refusal corpora --------------------------------------------------------

/// Belt (frontend) refusals: a comprehension reading its own destination.
/// Were the belt removed, these EMIT and silently read the empty
/// materialised destination — the PMAT-1316/1317 silent-wrong.
fn belt_refusals() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "values_iterable_selfref",
            r#"def check() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    d = {v: v for v in d.values()}
    return len(d)
"#,
        ),
        (
            "items_value_selfref",
            r#"def check() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    d = {k: v + d[k] for k, v in d.items()}
    return len(d)
"#,
        ),
        (
            "items_len_selfref",
            r#"def check() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    d = {k: len(d) for k, v in d.items()}
    return len(d)
"#,
        ),
        (
            "nested_comp_selfref",
            r#"def check() -> int:
    u = [1, 2, 3]
    d = {k: sum([d[k] for z in u]) for k in u}
    return len(d)
"#,
        ),
        (
            "l2gen_inner_iter_selfref",
            r#"def check() -> int:
    a = [1, 2]
    t = [x for x in a for y in t]
    return len(t)
"#,
        ),
        (
            "l2gen_element_selfref",
            r#"def check() -> int:
    a = [1, 2]
    b = [3, 4]
    t = [len(t) for x in a for y in b]
    return len(t)
"#,
        ),
    ]
}

/// Order-gate (backend) refusals: these are valid mHIR (lower is Ok) but the
/// WASM hash-order whitelist refuses them at `emit_module` — a store keyed by
/// anything but a loop var, a call in the value, a non-commutative fold, and
/// a comp over a dict mutated elsewhere in the function.
fn gate_refusals() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "const_key_store",
            r#"def check() -> int:
    s: set[int] = {1, 2, 3}
    r: dict[int, int] = {}
    for k in s:
        r[5] = k
    return len(r)
"#,
        ),
        (
            "computed_key_sugar",
            r#"def check() -> int:
    s: set[int] = {1, 2, 3}
    r = {x * 2: x for x in s}
    return len(r)
"#,
        ),
        (
            "call_in_value",
            r#"def helper(x: int) -> int:
    return x + 1
def check() -> int:
    s: set[int] = {1, 2, 3}
    r: dict[int, int] = {}
    for k in s:
        r[k] = helper(k)
    return len(r)
"#,
        ),
        (
            "vkeyed_items_swap",
            r#"def check() -> int:
    d: dict[int, int] = {1: 100, 2: 200}
    r: dict[int, int] = {}
    for k, v in d.items():
        r[v] = k
    return len(r)
"#,
        ),
        (
            "noncommutative_fold",
            r#"def check() -> int:
    d: dict[int, int] = {1: 1, 2: 2, 3: 3}
    acc: int = 0
    for k in d:
        acc = acc * 10 + k
    return acc
"#,
        ),
        (
            "comp_over_mutated_dict",
            r#"def check() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    r = {k: k for k in d}
    d[3] = 30
    return len(r)
"#,
        ),
    ]
}

// ---- WABT harness -----------------------------------------------------------

fn assemble_and_run(tag: &str, wat: &str) -> (String, bool) {
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-comphs-{}-{}", std::process::id(), tag));
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

/// `{name → value}` from `python3` running the IDENTICAL module source.
fn python_oracle(module_src: &str, names: &[&str]) -> Option<BTreeMap<String, i64>> {
    let mut prog = String::from(module_src);
    prog.push_str("\nimport sys\n_v = {}\n");
    for name in names {
        prog.push_str(&format!("_v['{name}'] = {name}()\n"));
    }
    prog.push_str("print(';'.join(f'{k}={val}' for k, val in _v.items()))\n");

    let out = Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "PMAT-1318: python3 oracle failed:\n{}",
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

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---- emit-path pins (run everywhere, no WABT needed) ------------------------

#[test]
fn admitted_modules_emit_with_keyed_store_helpers() {
    let int_wat = emit(admitted_int_module()).expect("int module lowers + emits");
    // The desugared comprehension loops ride the shared keyed-store /
    // membership machinery for BOTH key kinds.
    assert!(
        int_wat.contains("$__wasm_dict_set_i"),
        "int-keyed store helper missing"
    );
    assert!(
        int_wat.contains("$__wasm_dict_has_i"),
        "int-keyed membership helper missing"
    );
    let str_wat = emit(admitted_str_module()).expect("str module lowers + emits");
    assert!(
        str_wat.contains("$__wasm_dict_set_s"),
        "str-keyed store helper missing"
    );
    assert!(
        str_wat.contains("$__wasm_dict_has_s"),
        "str-keyed membership helper missing"
    );
}

#[test]
fn belt_refusals_refuse_at_the_frontend() {
    for (tag, src) in belt_refusals() {
        let err = lower(src)
            .err()
            .unwrap_or_else(|| panic!("belt case `{tag}` should refuse at parse_and_lower"));
        assert!(
            err.contains("comprehension itself reads"),
            "belt case `{tag}` refused with an unexpected message: {err}"
        );
    }
}

#[test]
fn gate_refusals_refuse_at_the_backend() {
    for (tag, src) in gate_refusals() {
        // Valid mHIR — the refusal is the WASM order-gate, not the frontend.
        let module = lower(src)
            .unwrap_or_else(|e| panic!("gate case `{tag}` should lower to valid mHIR: {e}"));
        let err = emit_module(&module)
            .err()
            .unwrap_or_else(|| panic!("gate case `{tag}` should refuse at emit_module"));
        let msg = format!("{err}");
        assert!(
            msg.contains("order-dependent")
                || msg.contains("order-INDEPENDENT")
                || msg.contains("iteration order")
                || msg.contains("keys-snapshot")
                || msg.contains("mutated"),
            "gate case `{tag}` refused with an unexpected message: {msg}"
        );
    }
}

// ---- executed cross-checks (WABT + python3) ---------------------------------

fn run_executed_pins(tag: &str, module_src: &str, pins: &[(&str, i64)]) {
    if !wasm_runtime_available() || !python3_available() {
        eprintln!("PMAT-1318: WABT or python3 unavailable — skipping executed diff for {tag}");
        return;
    }
    let wat = emit(module_src).expect("module lowers + emits");
    let (stdout, ok) = assemble_and_run(tag, &wat);
    assert!(ok, "wasm-interp must exit cleanly for {tag}:\n{stdout}");

    let names: Vec<&str> = pins.iter().map(|(n, _)| *n).collect();
    let oracle = python_oracle(module_src, &names).expect("python3 oracle");

    for (name, hand) in pins {
        let wasm_v = parse_scalar(&stdout, name);
        let cpython_v = *oracle.get(*name).expect("oracle has observable");
        assert_eq!(
            wasm_v, *hand,
            "{tag}/{name}: WASM value {wasm_v} != hand-derived pin {hand}"
        );
        assert_eq!(
            cpython_v, *hand,
            "{tag}/{name}: live CPython {cpython_v} != hand-derived pin {hand}"
        );
    }
}

#[test]
fn executed_int_pins_match_cpython_and_hand_constants() {
    run_executed_pins("int", admitted_int_module(), INT_PINS);
}

#[test]
fn executed_str_pins_match_cpython_and_hand_constants() {
    run_executed_pins("str", admitted_str_module(), STR_PINS);
}

/// MUTATION-VERIFIED teeth: a deliberately-wrong pin must fail the executed
/// diff. If the belt/gate ever silently admitted a miscompile (the
/// always-empty destination), the executed value would drift off the pin and
/// this cross-check fires. Documents that the pins are not vacuous.
#[test]
fn pins_are_not_vacuous() {
    if !python3_available() {
        eprintln!("PMAT-1318: python3 unavailable — skipping vacuity check");
        return;
    }
    let names: Vec<&str> = INT_PINS.iter().map(|(n, _)| *n).collect();
    let oracle = python_oracle(admitted_int_module(), &names).expect("python3 oracle");

    // PMAT-1505: the assertion that used to stand here was
    // `assert_ne!(*hand, cpython_v + 1)`, one line below
    // `assert_eq!(cpython_v, *hand)` — which reduces to `h != h + 1` and can
    // never fail for any `i64`. A test named `pins_are_not_vacuous` whose
    // distinguishing assertion is a TAUTOLOGY is the same defect as a test
    // whose guard can never say yes: it executes, it passes, and it discharges
    // nothing. The comment beside it already described the RIGHT property; only
    // the code did not implement it.
    let mut distinct: BTreeSet<i64> = BTreeSet::new();
    for (name, hand) in INT_PINS {
        let cpython_v = *oracle.get(*name).expect("oracle has observable");
        assert_eq!(cpython_v, *hand, "{name}: pin drifted from CPython");
        // The property the comment always named: an always-empty destination
        // collapses these len-scaled observables toward 0/1, so a pin sitting
        // on one of those would not distinguish a miscompile from a match.
        assert!(
            *hand != 0 && *hand != 1,
            "{name}: pin is {hand}, one of the degenerate values an always-empty \
             miscompile collapses toward — this pin cannot tell the two apart"
        );
        distinct.insert(*hand);
    }
    // And a miscompile that collapsed every observable to one constant would
    // still satisfy the per-pin check above. This one can fail; the tautology
    // could not.
    assert!(
        distinct.len() > 1,
        "every pin holds the same value ({distinct:?}); the pin set cannot \
         distinguish a per-case result from a constant"
    );
}
