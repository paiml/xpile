//! PMAT-1314 — EXECUTED witness for ORDER-INDEPENDENT dict-iteration STORES
//! in the WASM lane (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): the keyed
//! store `dst[k] = <pure fn of k>` joins the commutative-fold whitelist of
//! the PMAT-1292 hash-order body gate, unlocking the dict BUILD/TRANSFORM
//! loop (`for k in src: r[k] = src[k] * 2`) — the exact shape PMAT-1310
//! pinned as refused ("the build-fresh-from-param loop is its own future
//! slice"). Composed with dict function params (PMAT-1309) and dict returns
//! (PMAT-1310), this closes the full `def transform(src): r = {}; for k in
//! src: r[k] = f(src[k]); return r` pipeline.
//!
//! ## Why the keyed store is order-independent
//!
//! A hash-container iteration walks bump-heap STORAGE order, which matches
//! neither CPython's set hash order nor a dict's insertion order — so the
//! gate admits only bodies whose NET EFFECT is invariant under permutation
//! of the iteration sequence. The store `dst[kv] = e` qualifies iff the KEY
//! is exactly a loop variable and the VALUE is a pure function of that key:
//! distinct elements hit distinct keys, and EQUAL elements (a `.values()`
//! source can repeat) repeat an IDENTICAL write — so the final MAPPING is
//! permutation-invariant even though the write sequence is not. The value
//! must therefore read NO accumulator, NO stored-into dict (intermediate
//! contents are order-dependent), NO body-`let` temp (`t = a; r[b] = t`
//! would smuggle another loop var), NO OTHER loop variable (`r[b] = a`
//! under nesting varies the stored value with the outer element), and
//! contain NO call (a call can mutate a dict argument by reference,
//! PMAT-1309). An `.items()` store keyed by the KEY var may also read its
//! paired VALUE var — `v` is `src[k]`, a function of the key. This is a
//! SOUND UNDER-approximation: `r[k * 2] = 1` (injective in fact) still
//! refuses; `r[5] = k` (genuinely last-write-wins) always refuses.
//!
//! ## What executes here (value-matched vs CPython)
//!
//! * the headline transform `r[k] = src[k] * 2` over bare / `.keys()` /
//!   `.items()` / set / `.values()` sources (the `.values()`-keyed store is
//!   the distinct-count idiom `seen[v] = 1` — duplicate elements repeat an
//!   identical write);
//! * the FULL boundary composition: `transform(src)` reading a dict PARAM,
//!   building a fresh local, RETURNING it (PMAT-1309 + PMAT-1310 + this);
//! * a SCRAMBLED source (`del` swap-into-hole + reinsert, iterated via
//!   `.keys()`) whose storage order provably diverges from CPython's
//!   insertion order — the built mapping still matches keyed observables;
//! * store + commutative fold in ONE body; a filtered (guarded) build; a
//!   membership-guarded build (`if k in allow:` — `DictContains` joined the
//!   pure-read vocabulary); growth of the destination past the literal
//!   slack THROUGH the desugared loop (realloc + write-back mid-loop);
//! * str-KEYED and str-VALUED builds (the sv-twin `==` pins content).
//!
//! ## What refuses (pinned below)
//!
//! A fixed or computed key (`r[5] = k`, `r[k * 2] = 1`), a value reading an
//! accumulator / a stored-into dict / a `let` temp / another loop variable
//! (both smuggle shapes), a call in the value, a store into the ITERATED
//! dict (mutation-during-iteration, via the frontend's keys-snapshot form),
//! and a store into a dict PARAM (the PMAT-1309 growth-through-param belt —
//! unchanged by this slice, refusing at EMIT after the order gate admits).
//!
//! ## Witness shape
//!
//! Mirrors `dict_method_return_witness.rs`: ONE module, valid plain
//! `python3` AND wasm-frontend-lowerable; `wasm-interp --run-all-exports`
//! zero-invokes every export, so the param-taking `transform` is TOTAL
//! under a zeroed dict pointer (addr-0 count is 0 → the loop never runs).
//! Every `check_*` is pinned to a hand-derived constant AND cross-checked
//! against live `python3` on the IDENTICAL source. Gated on
//! `wasm_runtime_available()` — a clean skip (emit + refusal pins still
//! run) without WABT.

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
    ("check_transform_double", 3120),
    ("check_get_shift", 260),
    ("check_keys_view_const", 214),
    ("check_items_kv", 260),
    ("check_items_kv_sum", 233),
    ("check_set_square", 3110),
    ("check_values_dedup", 2),
    ("check_filtered", 250),
    ("check_contains_guard", 240),
    ("check_param_transform", 403),
    ("check_str_keys", 290),
    ("check_store_and_fold", 30060),
    ("check_preseeded", 275),
    ("check_growth", 25050),
    ("check_keysview_built", 25050),
    ("check_scrambled_src", 4260),
    ("check_str_valued", 1205),
    ("check_nested_inner", 270),
];

/// The single executed module — every export TOTAL under
/// `--run-all-exports` zeroed-arg invocation (`transform`'s dict param at
/// address 0 reads count 0, so its build loop never runs).
fn corpus_source() -> String {
    r#"def transform(src: dict[int, int]) -> dict[int, int]:
    r: dict[int, int] = {}
    for k in src:
        r[k] = src[k] + 100
    return r

def check_transform_double() -> int:
    src: dict[int, int] = {1: 10, 2: 20, 3: 30}
    r: dict[int, int] = {}
    for k in src:
        r[k] = src[k] * 2
    return len(r) * 1000 + r[1] + r[2] + r[3]

def check_get_shift() -> int:
    src: dict[int, int] = {1: 10, 2: 20}
    r: dict[int, int] = {}
    for k in src:
        r[k] = src.get(k, 0) * 2
    return len(r) * 100 + r[1] + r[2]

def check_keys_view_const() -> int:
    src: dict[int, int] = {1: 10, 2: 20}
    r: dict[int, int] = {}
    for k in src.keys():
        r[k] = 7
    return len(r) * 100 + r[1] + r[2]

def check_items_kv() -> int:
    src: dict[int, int] = {1: 10, 2: 20}
    r: dict[int, int] = {}
    for k, v in src.items():
        r[k] = v * 2
    return len(r) * 100 + r[1] + r[2]

def check_items_kv_sum() -> int:
    src: dict[int, int] = {1: 10, 2: 20}
    r: dict[int, int] = {}
    for k, v in src.items():
        r[k] = k + v
    return len(r) * 100 + r[1] + r[2]

def check_set_square() -> int:
    s: set[int] = {2, 5, 9}
    r: dict[int, int] = {}
    for x in s:
        r[x] = x * x
    return len(r) * 1000 + r[2] + r[5] + r[9]

def check_values_dedup() -> int:
    src: dict[int, int] = {1: 7, 2: 7, 3: 9}
    seen: dict[int, int] = {}
    for v in src.values():
        seen[v] = 1
    return len(seen)

def check_filtered() -> int:
    src: dict[int, int] = {1: 10, 2: 20, 3: 30}
    r: dict[int, int] = {}
    for k in src:
        if src[k] > 15:
            r[k] = src[k]
    return len(r) * 100 + r.get(2, 0) + r.get(3, 0)

def check_contains_guard() -> int:
    src: dict[int, int] = {1: 10, 2: 20, 3: 30}
    allow: dict[int, int] = {1: 0, 3: 0}
    r: dict[int, int] = {}
    for k in src:
        if k in allow:
            r[k] = src[k]
    return len(r) * 100 + r.get(1, 0) + r.get(3, 0)

def check_param_transform() -> int:
    d: dict[int, int] = {1: 1, 2: 2}
    e = transform(d)
    return len(e) * 100 + e[1] + e[2]

def check_str_keys() -> int:
    src: dict[str, int] = {"aa": 10, "bb": 20}
    r: dict[str, int] = {}
    for k in src:
        r[k] = src[k] * 3
    return len(r) * 100 + r["aa"] + r["bb"]

def check_store_and_fold() -> int:
    src: dict[int, int] = {1: 10, 2: 20}
    r: dict[int, int] = {}
    acc: int = 0
    for k in src:
        r[k] = src[k] * 2
        acc = acc + src[k]
    return acc * 1000 + r[1] + r[2]

def check_preseeded() -> int:
    src: dict[int, int] = {1: 5}
    r: dict[int, int] = {1: 999, 7: 70}
    for k in src:
        r[k] = src[k]
    return len(r) * 100 + r[1] + r[7]

def check_growth() -> int:
    src: dict[int, int] = {0: 0, 1: 2, 2: 4, 3: 6, 4: 8, 5: 10, 6: 12, 7: 14, 8: 16, 9: 18, 10: 20, 11: 22, 12: 24, 13: 26, 14: 28, 15: 30, 16: 32, 17: 34, 18: 36, 19: 38, 20: 40, 21: 42, 22: 44, 23: 46, 24: 48}
    r: dict[int, int] = {}
    for k in src:
        r[k] = src[k] + 1
    return len(r) * 1000 + r[24] + r[0]

def check_keysview_built() -> int:
    src: dict[int, int] = {0: 0}
    i: int = 1
    while i < 25:
        src[i] = i * 2
        i = i + 1
    r: dict[int, int] = {}
    for k in src.keys():
        r[k] = src[k] + 1
    return len(r) * 1000 + r[24] + r[0]

def check_scrambled_src() -> int:
    src: dict[int, int] = {1: 10, 2: 20, 3: 30, 4: 40}
    del src[2]
    src[5] = 50
    r: dict[int, int] = {}
    for k in src.keys():
        r[k] = src[k] * 2
    return len(r) * 1000 + r[1] + r[3] + r[4] + r[5]

def check_str_valued() -> int:
    src: dict[int, str] = {1: "ab", 2: "cde"}
    r: dict[int, str] = {}
    for k in src:
        r[k] = src[k]
    m: dict[int, str] = {2: "cde", 1: "ab"}
    if r == m:
        return len(r) * 100 + len(r[1]) + len(r[2]) + 1000
    return 0

def check_nested_inner() -> int:
    s1: set[int] = {1, 2}
    s2: set[int] = {3, 4}
    r: dict[int, int] = {}
    for a in s1:
        for b in s2:
            r[b] = b * 10
    return len(r) * 100 + r[3] + r[4]
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
            "PMAT-1314: python3 oracle failed:\n{}",
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
        std::env::temp_dir().join(format!("xpile-wasm-istore-{}-{}", std::process::id(), tag));
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

/// The whole corpus lowers through the FULL pipeline and carries the store
/// machinery: the shared update-or-insert helper for both key kinds (the
/// store IS `$__wasm_dict_set_<k>`, write-back included), the keyed read
/// helpers feeding the values, and the sv-twin for the str-valued build.
#[test]
fn corpus_emits_with_store_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // param-in, dict-out transform rides the boundary ABI.
        "(func $transform (param $src i32) (result i32)",
        // the store helper (update-or-insert, returns possibly-grown base)
        // for BOTH key kinds, and the keyed reads feeding values.
        "$__wasm_dict_set_i",
        "$__wasm_dict_set_s",
        "$__wasm_dict_get_i",
        "$__wasm_dict_get_s",
        "$__wasm_dict_has_i",
        // the str-VALUED build pins content equality via the sv-twin.
        "$__wasm_dict_eq_sv_i",
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

fn expect_refusal(name: &str, src: &str, needle: &str) {
    let Err(err) = emit(src) else {
        panic!("`{name}` is order-DEPENDENT (or belt-refused) and MUST refuse — a silent accept is the PMAT-1292 storage-order miscompile class");
    };
    assert!(
        err.contains(needle),
        "`{name}` refusal should contain {needle:?}, got: {err}"
    );
}

/// A FIXED key (`r[5] = k`) is genuinely last-write-wins — which write lands
/// last depends on storage order. Always refuses.
#[test]
fn fixed_key_store_refuses() {
    expect_refusal(
        "fixed-key",
        "def go() -> int:\n    src: dict[int, int] = {1: 10, 2: 20}\n    r: dict[int, int] = {}\n    for k in src:\n        r[5] = k\n    return len(r)\n",
        "not exactly a loop variable",
    );
}

/// A COMPUTED key (`r[k * 2] = 1`) refuses even though this particular map is
/// injective — the gate is a sound under-approximation and does not prove
/// injectivity.
#[test]
fn computed_key_store_refuses() {
    expect_refusal(
        "computed-key",
        "def go() -> int:\n    src: dict[int, int] = {1: 10}\n    r: dict[int, int] = {}\n    for k in src:\n        r[k * 2] = 1\n    return len(r)\n",
        "not exactly a loop variable",
    );
}

/// A stored value reading an ACCUMULATOR depends on how many iterations ran
/// before it — order-dependent.
#[test]
fn accumulator_valued_store_refuses() {
    expect_refusal(
        "acc-value",
        "def go() -> int:\n    src: dict[int, int] = {1: 10, 2: 20}\n    r: dict[int, int] = {}\n    acc: int = 0\n    for k in src:\n        acc = acc + 1\n        r[k] = acc\n    return len(r)\n",
        "not a pure function",
    );
}

/// A fold reading the STORED-INTO dict observes intermediate contents —
/// refuses through the widened forbidden set (accumulators ∪ store targets).
#[test]
fn fold_reading_store_target_refuses() {
    expect_refusal(
        "fold-reads-store",
        "def go() -> int:\n    src: dict[int, int] = {1: 10}\n    r: dict[int, int] = {}\n    acc: int = 0\n    for k in src:\n        r[k] = src[k]\n        acc = acc + r[k]\n    return acc\n",
        "not a commutative accumulation",
    );
}

/// An `.items()` store keyed by the VALUE var refuses — values are not
/// distinct, so `r[v] = k` is last-write-wins over colliding values.
#[test]
fn items_value_keyed_store_refuses() {
    expect_refusal(
        "items-value-key",
        "def go() -> int:\n    src: dict[int, int] = {1: 10}\n    r: dict[int, int] = {}\n    for k, v in src.items():\n        r[v] = k\n    return len(r)\n",
        "not a (read-only) loop variable",
    );
}

/// A store into the ITERATED dict is mutation-during-iteration: the frontend
/// routes it to the keys-snapshot + size-change-guard form, which the WASM
/// subset refuses distinctly (before the order gate ever sees it).
#[test]
fn store_into_iterated_dict_refuses() {
    expect_refusal(
        "store-iterated",
        "def go() -> int:\n    src: dict[int, int] = {1: 10}\n    for k in src:\n        src[k] = 0\n    return len(src)\n",
        "MUTATED dict",
    );
}

/// A CALL in the stored value refuses — a call can mutate a dict argument by
/// reference mid-iteration (PMAT-1309 reference params).
#[test]
fn call_in_stored_value_refuses() {
    expect_refusal(
        "call-value",
        "def f(x: int) -> int:\n    return x + 1\n\ndef go() -> int:\n    src: dict[int, int] = {1: 10}\n    r: dict[int, int] = {}\n    for k in src:\n        r[k] = f(k)\n    return len(r)\n",
        "a call inside the value stored into",
    );
}

/// A store into a dict PARAM passes the order gate but refuses at EMIT via
/// the PMAT-1309 growth-through-param belt — an insert can relocate the
/// record and strand the caller's pointer. The belt composes UNDER the new
/// whitelist arm, unchanged.
#[test]
fn store_into_param_dict_refuses_at_growth_belt() {
    expect_refusal(
        "param-dst",
        "def fill(src: dict[int, int], out: dict[int, int]) -> int:\n    for k in src:\n        out[k] = src[k]\n    return len(out)\n\ndef go() -> int:\n    a: dict[int, int] = {1: 10}\n    b: dict[int, int] = {}\n    return fill(a, b)\n",
        "PARAMETER",
    );
}

/// The `let`-temp smuggle: `t = a` (outer var) then `r[b] = t` in the inner
/// loop would store the OUTER element under an inner key — order-dependent.
/// Stored values may not read body `let` temps.
#[test]
fn let_temp_smuggle_refuses() {
    expect_refusal(
        "let-smuggle",
        "def go() -> int:\n    s1: set[int] = {1, 2}\n    s2: set[int] = {3, 4}\n    r: dict[int, int] = {}\n    for a in s1:\n        for b in s2:\n            t: int = a\n            r[b] = t\n    return len(r)\n",
        "not a pure function",
    );
}

/// The direct nested smuggle `r[b] = a`: the stored value varies with the
/// OUTER loop variable while the key is the INNER one — last-write-wins over
/// the outer order. Refuses via the other-loop-var exclusion.
#[test]
fn nested_outer_var_value_refuses() {
    expect_refusal(
        "nested-smuggle",
        "def go() -> int:\n    s1: set[int] = {1, 2}\n    s2: set[int] = {3, 4}\n    r: dict[int, int] = {}\n    for a in s1:\n        for b in s2:\n            r[b] = a\n    return len(r)\n",
        "not a pure function",
    );
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. Headlines: `check_param_transform`
/// (dict param IN, keyed-store build, dict return OUT — the full boundary
/// composition), `check_scrambled_src` (storage order provably diverged from
/// insertion order via a `del` swap-into-hole; the built mapping still
/// matches), `check_growth` (the destination relocates past the literal
/// slack THROUGH the loop's write-back), and `check_values_dedup` (duplicate
/// elements repeat an identical write — the distinct-count idiom).
#[test]
fn dict_iter_store_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1314: skipping EXECUTED dict-iter-store witness — WABT \
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
        eprintln!("PMAT-1314: python3 not available — pins stand on the hand-derived values");
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
