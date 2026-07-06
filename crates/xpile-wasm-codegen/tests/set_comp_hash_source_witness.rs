//! PMAT-1316 — EXECUTED witness for set COMPREHENSIONS over dict/set sources
//! (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): the frontend's set-comp
//! desugar (PMAT-501b) now admits `Type::Dict` (iterates KEYS, exactly like
//! the `for k in d:` statement — PMAT-472) and `Type::Set` iterables, so the
//! exact sugar PMAT-1315 pinned as frontend-refused — `{x * 2 for x in s}` —
//! lowers to the SAME `ForEach` + `SetAdd` HIR as the manual build loop and
//! rides the PMAT-1315 order gate unchanged. No WASM-lane edit: this is the
//! frontend slice that PMAT-1315's honesty pin scoped (that pin is flipped in
//! `set_build_store_witness.rs`).
//!
//! ## Desugar ≡ statement-form loop
//!
//! The widened classification mirrors the statement path arm-for-arm: a dict
//! iterates its keys lazily (`over_keys`) when read-only and takes the owned
//! keys-snapshot + size-change guard when the dict is mutated anywhere in
//! the function (PMAT-742) — which the WASM subset REFUSES exactly as it
//! refuses the statement spelling (pinned below); a set iterates its
//! elements (PMAT-847). Every backend sees the manual loop's HIR, so the
//! PMAT-1292/1314/1315 hash-order gate, the PMAT-1309 param belts, and the
//! PMAT-1310 return relocation all apply without new arms.
//!
//! ## The self-reference belt (the slice's real correctness find)
//!
//! The desugar binds the EMPTY destination before iterating, so a
//! comprehension that reads its own assignment target would see the
//! clobbered name, not the pre-assignment value CPython evaluates. Before
//! this slice that was a LIVE silent-wrong for list sources:
//! `t = {1}; t = {x for x in u if x in t}` emitted Rust whose filter read
//! the fresh empty set (returned 0 where CPython returns 1) — rustc
//! accepted it because `contains` + `insert` don't overlap borrows. Now ANY
//! read of the target from the iterable, a filter, or the element refuses
//! loudly at the frontend (all backends), while a comp VARIABLE that
//! shadows the target (`t = {t * 2 for t in u}`) stays admitted — the
//! renamed loop var, not the destination, is what the element reads.
//!
//! ## What executes here (value-matched vs CPython)
//!
//! * the flipped headliners: `{x * 2 for x in s}` (set source) and
//!   `{k * 3 for k in d}` (bare dict = keys);
//! * a FILTERED build `{x * 2 for x in s if x > 2}` (guard → body `If`);
//! * the already-admitted view spellings as family pins: `.values()`
//!   distinct (duplicates collapse), `.items()` tuple-target pair-sum;
//! * `{d[k] + k for k in d}` — the DictGet pure-read vocabulary in the
//!   element; the degenerate constant `{7 for x in s}`;
//! * GROWTH: a 25-element comp destination relocating past the 16-slot
//!   literal slack through the desugared loop's write-back;
//! * str elements over str keys (membership rides `$__wasm_dict_has_s`);
//! * the SHADOW carve-out `{t * 2 for t in u}` executing correctly;
//! * a SCRAMBLED source (`discard` swap-into-hole + re-`add`) — storage
//!   order provably diverges from CPython's; membership still matches;
//! * the FULL boundary composition `keyset(src)` — dict param IN (PMAT-1309),
//!   comp build, set return OUT (PMAT-1310) — called twice, per-call-fresh.
//!
//! Content observables are permutation-invariant by construction (len,
//! membership, base-100 folds over a BOUND `sorted(t)` list), so the
//! CPython cross-check cannot flap on set order.
//!
//! ## What refuses (pinned below)
//!
//! Self-reference through the iterable / a filter / the element; a comp over
//! a dict MUTATED in the function (WASM refuses the keys-snapshot + guard
//! form for comp and statement alike — the Rust lane admits BOTH); a call
//! element (the order gate, through the sugar); a tuple source (vocabulary
//! boundary); and, HONESTLY: the two-generator comp over hash sources and
//! the dict COMPREHENSION `{k: f(k) for k in d}` (the PMAT-1314 sugar) still
//! refuse at the frontend — the next frontend slice; the manual loop is
//! their supported spelling. `len({… for x in s})` in EXPRESSION position
//! lowers through different machinery and still refuses in the WASM lane
//! (len of a non-name collection), unchanged by this slice.
//!
//! ## Witness shape
//!
//! Mirrors `set_build_store_witness.rs`: ONE module, valid plain `python3`
//! AND wasm-frontend-lowerable; `wasm-interp --run-all-exports` zero-invokes
//! every export, so the param-taking `keyset` is TOTAL under a zeroed dict
//! pointer (addr-0 count is 0 → the desugared loop never runs). Every
//! `check_*` is pinned to a hand-derived constant AND cross-checked against
//! live `python3` on the IDENTICAL source. Gated on
//! `wasm_runtime_available()` — a clean skip (emit + refusal pins still run)
//! without WABT.

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
    ("check_comp_set_src", 320406),
    ("check_comp_dict_keys", 30336699),
    ("check_comp_filtered", 20608),
    ("check_comp_distinct_values", 20709),
    ("check_comp_items_pairsum", 21122),
    ("check_comp_src_read", 21122),
    ("check_comp_const_elem", 107),
    ("check_comp_growth", 250011),
    ("check_comp_str_elems", 211),
    ("check_comp_shadow", 320406),
    ("check_comp_scrambled", 42060810),
    ("check_pipeline", 3102033050607),
];

/// The single executed module — every export TOTAL under
/// `--run-all-exports` zeroed-arg invocation (`keyset`'s dict param at
/// address 0 reads count 0, so the desugared loop never runs).
fn corpus_source() -> String {
    r#"def keyset(src: dict[int, int]) -> set[int]:
    t = {k for k in src}
    return t

def fold_sorted(t: set[int]) -> int:
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return acc

def check_comp_set_src() -> int:
    s: set[int] = {1, 2, 3}
    t = {x * 2 for x in s}
    return len(t) * 100000 + fold_sorted(t)

def check_comp_dict_keys() -> int:
    d: dict[int, int] = {11: 1, 22: 2, 33: 3}
    t = {k * 3 for k in d}
    return len(t) * 10000000 + fold_sorted(t)

def check_comp_filtered() -> int:
    s: set[int] = {1, 2, 3, 4}
    t = {x * 2 for x in s if x > 2}
    return len(t) * 10000 + fold_sorted(t)

def check_comp_distinct_values() -> int:
    d: dict[int, int] = {1: 7, 2: 7, 3: 9}
    t = {v for v in d.values()}
    return len(t) * 10000 + fold_sorted(t)

def check_comp_items_pairsum() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    t = {k + v for k, v in d.items()}
    return len(t) * 10000 + fold_sorted(t)

def check_comp_src_read() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    t = {d[k] + k for k in d}
    return len(t) * 10000 + fold_sorted(t)

def check_comp_const_elem() -> int:
    s: set[int] = {1, 2, 3}
    t = {7 for x in s}
    return len(t) * 100 + fold_sorted(t)

def check_comp_growth() -> int:
    s: set[int] = set()
    i: int = 0
    while i < 25:
        s.add(i * 3)
        i = i + 1
    t = {x * 2 for x in s}
    r: int = len(t) * 10000
    if 144 in t:
        r = r + 1
    if 0 in t:
        r = r + 10
    if 145 in t:
        r = r + 100
    return r

def check_comp_str_elems() -> int:
    d: dict[str, int] = {"aa": 1, "bb": 2}
    t = {k for k in d}
    r: int = len(t) * 100
    if "aa" in t:
        r = r + 1
    if "bb" in t:
        r = r + 10
    if "zz" in t:
        r = r + 1000
    return r

def check_comp_shadow() -> int:
    u = [1, 2, 3]
    t = {t * 2 for t in u}
    return len(t) * 100000 + fold_sorted(t)

def check_comp_scrambled() -> int:
    s: set[int] = {1, 2, 3, 4}
    s.discard(2)
    s.add(5)
    t = {x * 2 for x in s}
    return len(t) * 10000000 + fold_sorted(t)

def check_pipeline() -> int:
    d1: dict[int, int] = {1: 7, 2: 7, 3: 9}
    d2: dict[int, int] = {5: 3, 6: 3, 7: 3}
    t1 = keyset(d1)
    t2 = keyset(d2)
    return len(t1) * 1000000000000 + fold_sorted(t1) * 10000000 + len(t2) * 1000000 + fold_sorted(t2)
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
            "PMAT-1316: python3 oracle failed:\n{}",
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-scomp-{}-{}", std::process::id(), tag));
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
/// machinery the desugared loops ride: `$__wasm_dict_set_<k>` (the shared
/// update-or-insert with the 0-sentinel value) for BOTH element kinds,
/// membership via `$__wasm_dict_has_<k>`, the scrambler's keyed removal, and
/// the boundary ABI of the param-in / set-out `keyset`.
#[test]
fn corpus_emits_with_set_add_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // dict param in, set out — the boundary ABI (PMAT-1309/1310).
        "(func $keyset (param $src i32) (result i32)",
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
        panic!(
            "`{name}` must refuse — a silent accept here is either the \
             self-reference clobber or the PMAT-1292 storage-order miscompile class"
        );
    };
    assert!(
        err.contains(needle),
        "`{name}` refusal should contain {needle:?}, got: {err}"
    );
}

/// The self-reference belt, all three read positions. The FILTER case is the
/// slice's real find: before PMAT-1316 it emitted Rust whose guard read the
/// fresh EMPTY destination (CPython reads the pre-assignment binding —
/// `t = {1}; u = [1, 2]; t = {x for x in u if x in t}` returned 0, not 1).
#[test]
fn self_reference_refuses_in_all_read_positions() {
    // iterable — `{x * 2 for x in t}` bound to `t`.
    expect_refusal(
        "self-ref-iterable",
        "def go() -> int:\n    t: set[int] = {1, 2}\n    t = {x * 2 for x in t}\n    return len(t)\n",
        "while the comprehension itself reads",
    );
    // filter — the previously-silent-wrong spelling.
    expect_refusal(
        "self-ref-filter",
        "def go() -> int:\n    t: set[int] = {1}\n    u = [1, 2]\n    t = {x for x in u if x in t}\n    return len(t)\n",
        "while the comprehension itself reads",
    );
    // element.
    expect_refusal(
        "self-ref-element",
        "def go() -> int:\n    t: set[int] = {5}\n    u = [1, 2]\n    t = {x + len(t) for x in u}\n    return len(t)\n",
        "while the comprehension itself reads",
    );
    // LIST source too — the same clobber (previously died downstream at
    // rustc E0502 instead of refusing at the frontend).
    expect_refusal(
        "self-ref-list-source",
        "def go() -> int:\n    t = [1, 2, 3]\n    t = {x for x in t}\n    return len(t)\n",
        "while the comprehension itself reads",
    );
}

/// A comp over a dict MUTATED anywhere in the function desugars to the
/// keys-snapshot + size-change-guard loop (exactly the `for k in d:`
/// statement shape, PMAT-742) — which the WASM subset refuses for comp and
/// statement alike. The Rust lane admits both.
#[test]
fn comp_over_mutated_dict_refuses_in_wasm_like_the_statement() {
    expect_refusal(
        "comp-mutated-dict",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    d[3] = 4\n    t = {k for k in d}\n    return len(t)\n",
        "for-loop over a MUTATED dict",
    );
}

/// A CALL element refuses through the sugar exactly as through the manual
/// loop — the PMAT-1315 order gate runs on the desugared HIR.
#[test]
fn call_element_refuses_through_the_sugar() {
    expect_refusal(
        "comp-call-elem",
        "def g(a: int) -> int:\n    return a\n\ndef go() -> int:\n    s: set[int] = {1, 2}\n    t = {g(x) for x in s}\n    return len(t)\n",
        "a call inside the element added to",
    );
}

/// Vocabulary boundary: a tuple source still refuses (the widened message
/// names the admitted iterables).
#[test]
fn tuple_source_still_refuses() {
    expect_refusal(
        "comp-tuple-src",
        "def go() -> int:\n    t = {x for x in (1, 2, 3)}\n    return len(t)\n",
        "set-comprehends over an iterable typing as",
    );
}

/// HONESTY pins for what this slice does NOT widen: the two-generator comp
/// over hash sources and the dict COMPREHENSION (the PMAT-1314 keyed-store
/// sugar) still refuse at the frontend — the manual loop is their supported
/// spelling; both are follow-up frontend slices.
#[test]
fn two_generator_and_dict_comp_over_hash_sources_still_refuse() {
    expect_refusal(
        "comp-2gen-hash-src",
        "def go() -> int:\n    s: set[int] = {1, 2}\n    u: set[int] = {10, 20}\n    t = {a + b for a in s for b in u}\n    return len(t)\n",
        "multi-generator set comprehension over an iterable typing as",
    );
    expect_refusal(
        "dict-comp-hash-src",
        "def go() -> int:\n    d: dict[int, int] = {1: 10, 2: 20}\n    r = {k: d[k] * 2 for k in d}\n    return len(r)\n",
        "dict-comprehends over an iterable typing as",
    );
}

/// HONESTY pin: an EXPRESSION-position comp (`len({x * 2 for x in s})`)
/// lowers through separate machinery and still refuses in the WASM lane —
/// len() needs a NAMED collection there; unchanged by this slice.
#[test]
fn expression_position_comp_still_refuses_in_wasm() {
    expect_refusal(
        "comp-expr-position",
        "def go() -> int:\n    s: set[int] = {1, 2, 3}\n    return len({x * 2 for x in s})\n",
        "len() of a non-name collection",
    );
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. Headlines: `check_comp_set_src` /
/// `check_comp_dict_keys` (the exact spellings the PMAT-1315 honesty pin
/// held refused), `check_pipeline` (dict param IN, comp build, set return
/// OUT — twice, per-call-fresh), `check_comp_growth` (the comp destination
/// relocates past the literal slack THROUGH the desugared loop),
/// `check_comp_shadow` (the belt's shadow carve-out executes), and
/// `check_comp_scrambled` (storage order provably diverged; membership
/// still matches).
#[test]
fn set_comp_hash_source_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1316: skipping EXECUTED set-comp witness — WABT \
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
        eprintln!("PMAT-1316: python3 not available — pins stand on the hand-derived values");
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
