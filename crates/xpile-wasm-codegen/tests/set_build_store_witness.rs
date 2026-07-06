//! PMAT-1315 — EXECUTED witness for ORDER-INDEPENDENT set-BUILD inserts in
//! the WASM lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): the insert
//! `dst.add(<pure fn of the loop vars>)` joins the PMAT-1292 hash-order body
//! gate's whitelist alongside the commutative folds and the PMAT-1314 keyed
//! store, unlocking the set BUILD/TRANSFORM loop (`for x in s: t.add(x * 2)`)
//! over bare / `.keys()` / `.values()` / `.items()` / set sources — and with
//! it the full `def distinct(src): t = set(); for v in src.values():
//! t.add(v); return t` boundary pipeline (params PMAT-1309 + returns
//! PMAT-1310).
//!
//! ## Why the set-build insert is order-independent
//!
//! A hash-container iteration walks bump-heap STORAGE order, which matches
//! neither CPython's set hash order nor a dict's insertion order — so the
//! gate admits only bodies whose NET EFFECT is invariant under permutation
//! of the iteration sequence. The insert `dst.add(e)` qualifies for ANY `e`
//! that is a pure function of the loop variable(s): set membership DEDUPS,
//! so EQUAL elements repeat an IDENTICAL idempotent insert and DISTINCT
//! elements commute — the final MEMBERSHIP is permutation-invariant with NO
//! injectivity requirement. That is strictly WEAKER than the keyed-store
//! precondition: where `r[b] = a` refuses under nesting (last-write-wins
//! over the outer order), `t.add(a + b)` admits (the cross-product
//! membership is the same under any interleaving), and the degenerate
//! `t.add(7)` admits where `r[5] = 3` refuses. The element must still be
//! iteration-invariant otherwise: NO accumulator, NO stored-into dict/set
//! (intermediate contents/membership are order-dependent), NO body-`let`
//! temp (could smuggle one), NO call (reference mutation mid-iteration,
//! PMAT-1309).
//!
//! ## What executes here (value-matched vs CPython)
//!
//! * the headline build `t.add(x * 2)` over a set source, key-copy over a
//!   bare dict, distinct-VALUES over `.values()` (duplicates collapse — the
//!   set edition of PMAT-1314's `seen[v] = 1` idiom), `.items()` pair-sum
//!   `t.add(k + v)` and value-only `t.add(v)`;
//! * the NESTED relaxation the keyed store must refuse: the cross-product
//!   `t.add(a + b)` and the outer-var-only `t.add(a)` under an inner loop;
//! * the FULL boundary composition `distinct(src)` — dict param IN, set
//!   build, set return OUT (PMAT-1309 + PMAT-1310 + this), called twice for
//!   per-call-fresh builds;
//! * a SCRAMBLED source (`discard` swap-into-hole + re-`add`) whose storage
//!   order provably diverges from CPython's — membership still matches;
//! * growth of the destination past the 16-slot literal slack THROUGH the
//!   desugared loop (25 inserts from empty: realloc + write-back mid-loop);
//! * a PRESEEDED destination, a guarded (filtered) build, build + fold in
//!   one body, an element reading the iterated dict (`t.add(d[k] + k)` —
//!   the DictGet pure-read vocabulary), the degenerate constant element,
//!   and str elements (`t.add(k)` over str keys, membership-probed via
//!   `$__wasm_dict_has_s`).
//!
//! Content observables are permutation-invariant by construction (len,
//! membership, base-100 folds over a BOUND `sorted(t)` list), so the
//! CPython cross-check cannot flap on set order.
//!
//! ## What refuses (pinned below)
//!
//! An element reading an accumulator / a body-`let` temp, a call in the
//! element, an `add` into the ITERATED set, an `add` into a set PARAM (the
//! PMAT-1309 growth-through-param belt — unchanged, refusing at EMIT after
//! the order gate admits), a guard observing the built set (`if x in t:`),
//! and a fold reading it (`acc = acc + len(t)`) — the built set joins the
//! body-wide forbidden union exactly as PMAT-1314 store targets do. Also
//! pinned HONESTLY: a set COMPREHENSION over a set/dict source still
//! refuses at the FRONTEND (its desugar accepts list/range iterables only),
//! so the manual loop is the supported spelling of this build — that gap is
//! a frontend slice, not an order-safety one.
//!
//! ## Witness shape
//!
//! Mirrors `dict_iter_store_witness.rs`: ONE module, valid plain `python3`
//! AND wasm-frontend-lowerable; `wasm-interp --run-all-exports` zero-invokes
//! every export, so the param-taking `distinct` is TOTAL under a zeroed dict
//! pointer (addr-0 count is 0 → the build loop never runs). Every `check_*`
//! is pinned to a hand-derived constant AND cross-checked against live
//! `python3` on the IDENTICAL source. Gated on `wasm_runtime_available()` —
//! a clean skip (emit + refusal pins still run) without WABT.

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

/// FULL pipeline: Python source → meta-HIR → WAT.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- the executed corpus ----------------------------------------------------

/// `(observable, hand-derived CPython value)` — the oracle re-derives each at
/// runtime, so a wrong constant here fails against BOTH lanes.
const PINS: &[(&str, i64)] = &[
    ("check_build_double", 320406),
    ("check_copy_keys", 3112233),
    ("check_distinct_values", 20709),
    ("check_items_pairsum", 21122),
    ("check_items_value_elem", 20709),
    ("check_cross_product", 411122122),
    ("check_nested_outer_elem", 20102),
    ("check_guarded", 20304),
    ("check_build_and_fold", 6320406),
    ("check_src_read", 21122),
    ("check_const_elem", 107),
    ("check_preseeded", 320499),
    ("check_scrambled_src", 42060810),
    ("check_growth", 250011),
    ("check_str_elems", 211),
    ("check_pipeline", 270913),
];

/// The single executed module — every export TOTAL under
/// `--run-all-exports` zeroed-arg invocation (`distinct`'s dict param at
/// address 0 reads count 0, so its build loop never runs).
fn corpus_source() -> String {
    r#"def distinct(src: dict[int, int]) -> set[int]:
    t: set[int] = set()
    for v in src.values():
        t.add(v)
    return t

def fold_sorted(t: set[int]) -> int:
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return acc

def check_build_double() -> int:
    s: set[int] = {1, 2, 3}
    t: set[int] = set()
    for x in s:
        t.add(x * 2)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 100000 + acc

def check_copy_keys() -> int:
    d: dict[int, int] = {11: 1, 22: 2, 33: 3}
    t: set[int] = set()
    for k in d:
        t.add(k)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 1000000 + acc

def check_distinct_values() -> int:
    d: dict[int, int] = {1: 7, 2: 7, 3: 9}
    t: set[int] = set()
    for v in d.values():
        t.add(v)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000 + acc

def check_items_pairsum() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    t: set[int] = set()
    for k, v in d.items():
        t.add(k + v)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000 + acc

def check_items_value_elem() -> int:
    d: dict[int, int] = {1: 7, 2: 7, 3: 9}
    t: set[int] = set()
    for k, v in d.items():
        t.add(v)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000 + acc

def check_cross_product() -> int:
    s: set[int] = {1, 2}
    u: set[int] = {10, 20}
    t: set[int] = set()
    for a in s:
        for b in u:
            t.add(a + b)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 100000000 + acc

def check_nested_outer_elem() -> int:
    s1: set[int] = {1, 2}
    s2: set[int] = {3, 4}
    t: set[int] = set()
    for a in s1:
        for b in s2:
            t.add(a)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000 + acc

def check_guarded() -> int:
    s: set[int] = {1, 2, 3, 4}
    t: set[int] = set()
    for x in s:
        if x > 2:
            t.add(x)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000 + acc

def check_build_and_fold() -> int:
    s: set[int] = {1, 2, 3}
    t: set[int] = set()
    total: int = 0
    for x in s:
        t.add(x * 2)
        total = total + x
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return total * 1000000 + len(t) * 100000 + acc

def check_src_read() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    t: set[int] = set()
    for k in d:
        t.add(d[k] + k)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000 + acc

def check_const_elem() -> int:
    s: set[int] = {1, 2, 3}
    t: set[int] = set()
    for x in s:
        t.add(7)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 100 + acc

def check_preseeded() -> int:
    s: set[int] = {1, 2}
    t: set[int] = {99}
    for x in s:
        t.add(x * 2)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 100000 + acc

def check_scrambled_src() -> int:
    s: set[int] = {1, 2, 3, 4}
    s.discard(2)
    s.add(5)
    t: set[int] = set()
    for x in s:
        t.add(x * 2)
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return len(t) * 10000000 + acc

def check_growth() -> int:
    src: dict[int, int] = {0: 0}
    i: int = 1
    while i < 25:
        src[i] = i * 2
        i = i + 1
    t: set[int] = set()
    for k in src.keys():
        t.add(k)
    r: int = len(t) * 10000
    if 24 in t:
        r = r + 1
    if 0 in t:
        r = r + 10
    if 25 in t:
        r = r + 100
    return r

def check_str_elems() -> int:
    d: dict[str, int] = {"aa": 1, "bb": 2}
    t: set[str] = set()
    for k in d:
        t.add(k)
    r: int = len(t) * 100
    if "aa" in t:
        r = r + 1
    if "bb" in t:
        r = r + 10
    if "zz" in t:
        r = r + 1000
    return r

def check_pipeline() -> int:
    d1: dict[int, int] = {1: 7, 2: 7, 3: 9}
    d2: dict[int, int] = {5: 3, 6: 3, 7: 3}
    t1 = distinct(d1)
    t2 = distinct(d2)
    return len(t1) * 100000 + fold_sorted(t1) * 100 + len(t2) * 10 + fold_sorted(t2)
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
            "PMAT-1315: python3 oracle failed:\n{}",
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
        std::env::temp_dir().join(format!("xpile-wasm-sbuild-{}-{}", std::process::id(), tag));
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

/// The whole corpus lowers through the FULL pipeline and carries the set-add
/// machinery: `s.add(e)` IS `$__wasm_dict_set_<k>` with the 0-sentinel value
/// (a set is a keys-only dict) for BOTH element kinds, membership rides
/// `$__wasm_dict_has_<k>`, and the boundary pipeline exports the param-in /
/// set-out `distinct`.
#[test]
fn corpus_emits_with_set_add_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // dict param in, set out — the boundary ABI (PMAT-1309/1310).
        "(func $distinct (param $src i32) (result i32)",
        // the shared update-or-insert store (set-add's engine) for BOTH
        // element kinds, and the membership probes.
        "$__wasm_dict_set_i",
        "$__wasm_dict_set_s",
        "$__wasm_dict_has_i",
        "$__wasm_dict_has_s",
        // the scrambler: `discard` is the shared keyed removal.
        "$__wasm_dict_pop_i",
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

fn expect_refusal(name: &str, src: &str, needle: &str) {
    let Err(err) = emit(src) else {
        panic!("`{name}` is order-DEPENDENT (or belt-refused) and MUST refuse — a silent accept is the PMAT-1292 storage-order miscompile class");
    };
    assert!(
        err.contains(needle),
        "`{name}` refusal should contain {needle:?}, got: {err}"
    );
}

/// An element reading an ACCUMULATOR depends on how many iterations ran
/// before it — order-dependent.
#[test]
fn accumulator_element_refuses() {
    expect_refusal(
        "acc-elem",
        "def go() -> int:\n    s: set[int] = {1, 2, 3}\n    t: set[int] = set()\n    total: int = 0\n    for x in s:\n        total = total + x\n        t.add(total)\n    return len(t)\n",
        "reads an accumulator",
    );
}

/// A CALL in the element refuses — a call can mutate a dict argument by
/// reference mid-iteration (PMAT-1309 reference params).
#[test]
fn call_element_refuses() {
    expect_refusal(
        "call-elem",
        "def g(a: int) -> int:\n    return a\n\ndef go() -> int:\n    s: set[int] = {1, 2}\n    t: set[int] = set()\n    for x in s:\n        t.add(g(x))\n    return len(t)\n",
        "a call inside the element added to",
    );
}

/// The `let`-temp smuggle: an element may not read a body `let` temp — a temp
/// is checked only for accumulator-freedom, so it could carry order-dependent
/// content the element check would then miss.
#[test]
fn let_temp_element_refuses() {
    expect_refusal(
        "let-temp-elem",
        "def go() -> int:\n    s: set[int] = {1, 2}\n    t: set[int] = set()\n    for x in s:\n        y: int = x * 2\n        t.add(y)\n    return len(t)\n",
        "body `let` temp",
    );
}

/// An `add` into the ITERATED set is mutation-during-iteration — no defined
/// order to preserve.
#[test]
fn add_while_iterating_refuses() {
    expect_refusal(
        "add-iterated",
        "def go() -> int:\n    t: set[int] = {1, 2}\n    for x in t:\n        t.add(x + 10)\n    return len(t)\n",
        "WHILE iterating it",
    );
}

/// An `add` into a set PARAM passes the order gate but refuses at EMIT via
/// the PMAT-1309 growth-through-param belt — an insert can relocate the
/// record and strand the caller's pointer. The belt composes UNDER the new
/// whitelist arm, unchanged.
#[test]
fn add_into_param_set_refuses_at_growth_belt() {
    expect_refusal(
        "param-dst",
        "def fill(dst: set[int], src: set[int]) -> int:\n    for x in src:\n        dst.add(x)\n    return len(dst)\n\ndef go() -> int:\n    a: set[int] = {1}\n    b: set[int] = {2, 3}\n    return fill(a, b)\n",
        "PARAMETER",
    );
}

/// A guard observing the BUILT set reads order-dependent intermediate
/// membership — the set-add target joins the body-wide forbidden union
/// exactly as PMAT-1314 store targets do.
#[test]
fn membership_guard_on_built_set_refuses() {
    expect_refusal(
        "guard-reads-dst",
        "def go() -> int:\n    s: set[int] = {1, 2, 3}\n    t: set[int] = set()\n    for x in s:\n        if x in t:\n            t.add(x)\n    return len(t)\n",
        "a conditional guard observes an accumulator or a stored-into dict/set",
    );
}

/// A fold reading the BUILT set (`acc = acc + len(t)`) observes intermediate
/// cardinality — refuses through the same widened forbidden union.
#[test]
fn fold_reading_built_set_refuses() {
    expect_refusal(
        "fold-reads-dst",
        "def go() -> int:\n    s: set[int] = {1, 2, 3}\n    t: set[int] = set()\n    acc: int = 0\n    for x in s:\n        t.add(x)\n        acc = acc + len(t)\n    return acc\n",
        "not a commutative accumulation",
    );
}

/// PMAT-1316 FLIPPED the old honesty pin: a set COMPREHENSION over a
/// set/dict source — the exact sugar for this build loop — now LOWERS: the
/// frontend desugar produces the SAME `ForEach` + `SetAdd` HIR as the manual
/// loop this witness certifies, and the order gate rides it unchanged. The
/// executed differential for the sugar lives in
/// `set_comp_hash_source_witness.rs`.
#[test]
fn set_comprehension_over_hash_source_now_lowers() {
    for (name, src) in [
        (
            "comprehension-set-src",
            "def go() -> int:\n    s: set[int] = {1, 2, 3}\n    t = {x * 2 for x in s}\n    return len(t)\n",
        ),
        (
            "comprehension-dict-src",
            "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    t = {k * 3 for k in d}\n    return len(t)\n",
        ),
    ] {
        emit(src).unwrap_or_else(|e| {
            panic!("`{name}` must lower through the PMAT-1316 widened desugar, got: {e}")
        });
    }
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. Headlines: `check_pipeline` (dict
/// param IN, set build, set return OUT — the full boundary composition,
/// twice for per-call freshness), `check_scrambled_src` (storage order
/// provably diverged via a `discard` swap-into-hole; membership still
/// matches), `check_growth` (the destination relocates past the literal
/// slack THROUGH the loop's write-back), `check_distinct_values` (duplicate
/// elements collapse — the dedup that MAKES the insert order-independent),
/// and `check_cross_product` (the nested relaxation the keyed store must
/// refuse).
#[test]
fn set_build_store_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1315: skipping EXECUTED set-build witness — WABT \
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
        eprintln!("PMAT-1315: python3 not available — pins stand on the hand-derived values");
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
