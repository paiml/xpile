//! PMAT-1317 — EXECUTED witness for dict COMPREHENSIONS over dict/set
//! sources (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`) and the comprehension
//! self-reference clobber belt carried to the dict AND list desugars: the
//! frontend's dict-comp desugar (PMAT-501) now admits `Type::Dict` (iterates
//! KEYS, exactly like the `for k in d:` statement — PMAT-472) and
//! `Type::Set` iterables, so the exact sugar PMAT-1316 pinned as
//! frontend-refused — `{k: d[k] * 2 for k in d}` — lowers to the SAME
//! `ForEach` + `DictSet` HIR as the manual keyed-store loop (PMAT-1314) and
//! rides the hash-order gate unchanged. No WASM-lane edit: this is the
//! frontend slice the PMAT-1316 honesty pin scoped (that pin is flipped in
//! `set_comp_hash_source_witness.rs`) — the SEVENTH already-generic slice.
//!
//! ## Desugar ≡ statement-form loop
//!
//! The widened classification mirrors the statement path arm-for-arm: a
//! dict iterates its keys lazily (`over_keys`) when read-only and takes the
//! owned keys-snapshot + size-change guard when the dict is mutated
//! anywhere in the function (PMAT-742) — which the WASM subset REFUSES
//! exactly as it refuses the statement spelling (pinned below); a set
//! iterates its elements (PMAT-847). Every backend sees the manual loop's
//! HIR, so the PMAT-1292/1314/1315 hash-order gate, the PMAT-1309 param
//! belts, and the PMAT-1310 return relocation all apply without new arms —
//! in particular a store keyed by anything but the loop var (a computed
//! key, the `.items()` VALUE var) and a call in the value refuse THROUGH
//! the sugar, order-gate-verbatim.
//!
//! ## The self-reference belt (the slice's real correctness find)
//!
//! PMAT-1316's audit lead — "dict/list comps share the unguarded clobber
//! shape" — CONFIRMED LIVE. Each desugar binds the EMPTY destination before
//! iterating, so a comprehension reading its own assignment target saw the
//! clobbered name, not the pre-assignment value CPython evaluates. Probed
//! at HEAD before this slice, on the RUST lane:
//!
//! * `d = {k: 1 for k in u if k in d}` — filter read the fresh empty
//!   shadow: always-`{}` where CPython keeps the matching keys (silent
//!   wrong, rustc accepted);
//! * `d = {k: v * 2 for k, v in d.items()}` — the iterable's pair snapshot
//!   read the empty shadow: always-`{}` (silent wrong, rustc accepted);
//! * `ys = [len(ys) for x in u]` — the element read the GROWING fresh
//!   accumulator (CPython reads the constant pre-assignment length);
//! * `xs = [x for x in xs]` — a CPython copy — died downstream at rustc
//!   E0502 instead of refusing at the frontend.
//!
//! All four now refuse loudly at the frontend (all backends) via the shared
//! `comp_self_reference_belt`, with the PMAT-1316 shadow carve-out intact:
//! a comp VARIABLE shadowing the target (`r = {r: r * 2 for r in u}`)
//! stays admitted — the renamed loop var, not the destination, is what the
//! key/value/element read.
//!
//! ## What executes here (value-matched vs CPython)
//!
//! * the flipped headliners: `{k: k * 2 for k in d}` (bare dict = keys) and
//!   `{x: x * 3 for x in s}` (set source); a FILTERED build over a set;
//! * `{k: d[k] + k for k in d}` — the PMAT-1316-pinned source-read shape
//!   (DictGet pure-read in the VALUE, PMAT-1314's `expr_references_any`
//!   fix);
//! * the already-admitted view spellings as family pins: `.values()`
//!   source (duplicate values collapse to distinct KEYS of the result) and
//!   the k-keyed `.items()` pair store `{k: k + v for k, v in d.items()}`;
//! * GROWTH: a 25-key comp destination relocating past the 16-slot literal
//!   slack through the desugared loop's write-back;
//! * str keys over a str-keyed source (store rides `$__wasm_dict_set_s`,
//!   membership `$__wasm_dict_has_s`);
//! * the SHADOW carve-out `{r: r * 2 for r in u}` executing correctly;
//! * a SCRAMBLED set source (`discard` swap-into-hole + re-`add`) —
//!   storage order provably diverges from CPython's; content still matches;
//! * the FULL boundary composition `transform(src)` — dict param IN
//!   (PMAT-1309), dict-comp build, dict return OUT (PMAT-1310) — called
//!   twice, per-call-fresh.
//!
//! Content observables are permutation-invariant by construction (len +
//! base-10000 folds over a BOUND `sorted(r)` key list), so the CPython
//! cross-check cannot flap on hash/storage order.
//!
//! ## What refuses (pinned below)
//!
//! Self-reference through the dict-comp iterable / filter / value /
//! `.items()` iterable AND the list-comp iterable / filter / element; a
//! comp over a dict MUTATED in the function (WASM refuses the
//! keys-snapshot + guard form for comp and statement alike — the Rust lane
//! admits BOTH); a COMPUTED key, the v-KEYED `.items()` swap
//! (`{v: k …}` — last-write-wins is order-dependent), and a CALL value
//! (all three order-gate refusals, through the sugar); a tuple source
//! (vocabulary boundary); and, HONESTLY: the two-generator dict comp over
//! hash sources still refuses at the frontend — the manual nested loop is
//! its supported spelling.
//!
//! ## Witness shape
//!
//! Mirrors `set_comp_hash_source_witness.rs`: ONE module, valid plain
//! `python3` AND wasm-frontend-lowerable; `wasm-interp --run-all-exports`
//! zero-invokes the zero-arg exports and the param-taking `transform` /
//! `fold_dict` stay TOTAL under a zeroed dict pointer (addr-0 count is 0 →
//! the desugared loop never runs). Every `check_*` is pinned to a
//! hand-derived constant AND cross-checked against live `python3` on the
//! IDENTICAL source. Gated on `wasm_runtime_available()` — a clean skip
//! (emit + refusal pins still run) without WABT.

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
    ("check_comp_dict_keys", 3030605100714),
    ("check_comp_src_read", 201110222),
    ("check_comp_set_src", 3010302060309),
    ("check_comp_filtered", 203060408),
    ("check_comp_values_distinct", 207080910),
    ("check_comp_items_kv", 201110222),
    ("check_comp_growth", 250073),
    ("check_comp_str_keys", 207),
    ("check_comp_shadow", 3010202040306),
    ("check_comp_scrambled", 106030604080510),
    ("check_pipeline", 1140218000506),
];

/// The single executed module — every export TOTAL under
/// `--run-all-exports` zeroed-arg invocation (`transform`'s and
/// `fold_dict`'s dict params at address 0 read count 0, so the desugared
/// loops never run).
fn corpus_source() -> String {
    r#"def transform(src: dict[int, int]) -> dict[int, int]:
    r = {k: src[k] * 2 for k in src}
    return r

def fold_dict(d: dict[int, int]) -> int:
    xs = sorted(d)
    acc: int = 0
    for y in xs:
        acc = acc * 10000 + y * 100 + d[y]
    return acc

def check_comp_dict_keys() -> int:
    d: dict[int, int] = {3: 1, 5: 2, 7: 3}
    r = {k: k * 2 for k in d}
    return len(r) * 1000000000000 + fold_dict(r)

def check_comp_src_read() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    r = {k: d[k] + k for k in d}
    return len(r) * 100000000 + fold_dict(r)

def check_comp_set_src() -> int:
    s: set[int] = {1, 2, 3}
    r = {x: x * 3 for x in s}
    return len(r) * 1000000000000 + fold_dict(r)

def check_comp_filtered() -> int:
    s: set[int] = {1, 2, 3, 4}
    r = {x: x * 2 for x in s if x > 2}
    return len(r) * 100000000 + fold_dict(r)

def check_comp_values_distinct() -> int:
    d: dict[int, int] = {1: 7, 2: 7, 3: 9}
    r = {v: v + 1 for v in d.values()}
    return len(r) * 100000000 + fold_dict(r)

def check_comp_items_kv() -> int:
    d: dict[int, int] = {1: 10, 2: 20}
    r = {k: k + v for k, v in d.items()}
    return len(r) * 100000000 + fold_dict(r)

def check_comp_growth() -> int:
    s: set[int] = set()
    i: int = 0
    while i < 25:
        s.add(i * 3)
        i = i + 1
    r = {x: x + 1 for x in s}
    out: int = len(r) * 10000
    if 72 in r:
        out = out + r[72]
    if 1 in r:
        out = out + 1000
    return out

def check_comp_str_keys() -> int:
    d: dict[str, int] = {"aa": 1, "bb": 2}
    r = {k: 7 for k in d}
    out: int = len(r) * 100
    if "aa" in r:
        out = out + r["aa"]
    if "zz" in r:
        out = out + 1000
    return out

def check_comp_shadow() -> int:
    u = [1, 2, 3]
    r = {r: r * 2 for r in u}
    return len(r) * 1000000000000 + fold_dict(r)

def check_comp_scrambled() -> int:
    s: set[int] = {1, 2, 3, 4}
    s.discard(2)
    s.add(5)
    r = {x: x * 2 for x in s}
    return len(r) * 1000000000000 + fold_dict(r)

def check_pipeline() -> int:
    d1: dict[int, int] = {1: 7, 2: 9}
    d2: dict[int, int] = {5: 3}
    r1 = transform(d1)
    r2 = transform(d2)
    return fold_dict(r1) * 1000000 + fold_dict(r2)
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
            "PMAT-1317: python3 oracle failed:\n{}",
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dcomp-{}-{}", std::process::id(), tag));
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

/// The whole corpus lowers through the FULL pipeline and carries the keyed
/// store machinery the desugared loops ride: `$__wasm_dict_set_<k>` (the
/// shared update-or-insert) for BOTH key kinds, membership via
/// `$__wasm_dict_has_<k>`, the scrambler's keyed removal, and the boundary
/// ABI of the dict-in / dict-out `transform`.
#[test]
fn corpus_emits_with_keyed_store_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // dict param in, dict out — the boundary ABI (PMAT-1309/1310).
        "(func $transform (param $src i32) (result i32)",
        // the shared update-or-insert store for BOTH key kinds, and the
        // membership probes.
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

/// The self-reference belt on the DICT desugar, all four read positions.
/// The FILTER and ITERS cases are the slice's real find: before PMAT-1317
/// both emitted Rust that read the fresh EMPTY destination (CPython reads
/// the pre-assignment binding) — `d = {k: 1 for k in u if k in d}` and
/// `d = {k: v * 2 for k, v in d.items()}` both returned `{}` on every
/// input.
#[test]
fn dict_comp_self_reference_refuses_in_all_read_positions() {
    // iterable — `{k: 1 for k in d}` bound to `d`.
    expect_refusal(
        "self-ref-dict-iterable",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    d = {k: 1 for k in d}\n    return len(d)\n",
        "while the comprehension itself reads",
    );
    // filter — the previously-silent-wrong spelling.
    expect_refusal(
        "self-ref-dict-filter",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    u = [1, 2]\n    d = {k: 1 for k in u if k in d}\n    return len(d)\n",
        "while the comprehension itself reads",
    );
    // value.
    expect_refusal(
        "self-ref-dict-value",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    u = [1, 2]\n    d = {k: len(d) for k in u}\n    return len(d)\n",
        "while the comprehension itself reads",
    );
    // tuple-target `.items()` iterable — the natural rebuild idiom,
    // previously silent-wrong through the pair-snapshot.
    expect_refusal(
        "self-ref-dict-items",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    d = {k: v * 2 for k, v in d.items()}\n    return len(d)\n",
        "while the comprehension itself reads",
    );
}

/// The same belt on the LIST desugar — the audit lead's other half. The
/// ELEMENT case read the GROWING fresh accumulator (CPython: the constant
/// pre-assignment length); the FILTER case read the empty shadow; the
/// ITERABLE case (a CPython copy) previously died downstream at rustc
/// E0502 instead of refusing at the frontend.
#[test]
fn list_comp_self_reference_refuses_in_all_read_positions() {
    expect_refusal(
        "self-ref-list-iterable",
        "def go() -> int:\n    xs = [1, 2, 3]\n    xs = [x for x in xs]\n    return len(xs)\n",
        "while the comprehension itself reads",
    );
    expect_refusal(
        "self-ref-list-filter",
        "def go() -> int:\n    ys = [5]\n    u = [1, 2]\n    ys = [x for x in u if x in ys]\n    return len(ys)\n",
        "while the comprehension itself reads",
    );
    expect_refusal(
        "self-ref-list-element",
        "def go() -> int:\n    ys = [5]\n    u = [1, 2]\n    ys = [len(ys) for x in u]\n    return len(ys)\n",
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
        "dict-comp-mutated-dict",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    d[3] = 4\n    r = {k: k for k in d}\n    return len(r)\n",
        "for-loop over a MUTATED dict",
    );
}

/// The PMAT-1314 order gate runs on the desugared HIR verbatim: a COMPUTED
/// key, the v-KEYED `.items()` swap (`{v: k …}` — last-write-wins depends
/// on iteration order), and a CALL value all refuse through the sugar
/// exactly as through the manual loop.
#[test]
fn order_gate_refusals_fire_through_the_sugar() {
    expect_refusal(
        "dict-comp-computed-key",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    r = {k + 1: 2 for k in d}\n    return len(r)\n",
        "keyed by an expression that is not exactly a loop variable",
    );
    expect_refusal(
        "dict-comp-v-keyed-swap",
        "def go() -> int:\n    d: dict[int, int] = {1: 10}\n    r = {v: k for k, v in d.items()}\n    return len(r)\n",
        "which is not a (read-only) loop variable",
    );
    expect_refusal(
        "dict-comp-call-value",
        "def g(a: int) -> int:\n    return a\n\ndef go() -> int:\n    d: dict[int, int] = {1: 10}\n    r = {k: g(k) for k in d}\n    return len(r)\n",
        "a call inside the value stored into",
    );
}

/// Vocabulary boundary: a tuple source still refuses (the widened message
/// names the admitted iterables).
#[test]
fn tuple_source_still_refuses() {
    expect_refusal(
        "dict-comp-tuple-src",
        "def go() -> int:\n    r = {x: x for x in (1, 2)}\n    return len(r)\n",
        "dict-comprehends over an iterable typing as",
    );
}

/// PMAT-1319 FLIPPED this PMAT-1317 honesty pin: the two-generator dict/set
/// comprehension over hash sources now LOWERS (the shared `desugar_comp_2gen`
/// widened its iterable vocabulary to dict/set — see
/// `comp_2gen_hash_source_witness.rs` for the executed witness). A 2-gen dict
/// comp with a key that is a bare loop var (`{a: b for a in s for b in u}`)
/// lowers to valid mHIR, but its store is last-write-wins over the repeated key
/// `a` — order-dependent — so the WASM order-gate refuses it at the backend
/// (identically to the manual `for a in s: for b in u: r[a] = b` loop).
#[test]
fn two_generator_dict_comp_over_hash_sources_now_lowers_gate_refuses_wasm() {
    let src = "def go() -> int:\n    s: set[int] = {1, 2}\n    u: set[int] = {10, 20}\n    r = {a: b for a in s for b in u}\n    return len(r)\n";
    lower(src).expect("2-gen dict comp over hash sources must now LOWER (PMAT-1319)");
    expect_refusal("dict-comp-2gen-hash-src", src, "order-dependent");
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then
/// the same source through live CPython. Headlines: `check_comp_dict_keys` /
/// `check_comp_set_src` (the flipped PMAT-1316 refusals), the growth
/// relocation, the shadow carve-out, and the dict-in/dict-out `transform`
/// pipeline. Every observable folds over a BOUND `sorted(r)` key list, so
/// the cross-check is permutation-invariant and cannot flap on
/// hash/storage order.
#[test]
fn dict_comp_hash_source_witness_executes_and_matches_cpython() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1317: WABT not available — skipping the executed witness");
        return;
    }
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    let (stdout, ok) = assemble_and_run("corpus", &wat);
    assert!(ok, "wasm-interp must exit cleanly:\n{stdout}");

    // Lane 1: WASM values against the hand-derived pins.
    for (name, expected) in PINS {
        let got = parse_scalar(&stdout, name);
        assert_eq!(
            got, *expected,
            "`{name}`: wasm returned {got}, hand-derived CPython value is {expected}"
        );
    }

    // Lane 2: live CPython on the IDENTICAL source (skip cleanly if absent).
    let Some(oracle) = python_oracle() else {
        if python3_available() {
            panic!("python3 is available but the oracle run failed — corpus must be valid Python");
        }
        eprintln!("PMAT-1317: python3 not available — pins stand as the oracle");
        return;
    };
    for (name, expected) in PINS {
        let live = oracle
            .get(*name)
            .unwrap_or_else(|| panic!("oracle missing `{name}`"));
        assert_eq!(
            live, expected,
            "`{name}`: live CPython returned {live}, pinned {expected} — the pin is wrong"
        );
    }
}

/// The SHADOW carve-out is admitted AND correct on the LIST desugar too
/// (`ys = [ys * 2 for ys in u]` — the renamed loop var, not the
/// destination, is what the element reads).
#[test]
fn list_comp_shadow_carve_out_executes() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1317: WABT not available — skipping the executed shadow check");
        return;
    }
    let src = "def go() -> int:\n    u = [1, 2, 3]\n    total = 0\n    ys = [ys * 2 for ys in u]\n    for y in ys:\n        total = total + y\n    return total\n";
    let wat = emit(src).expect("list shadow carve-out must lower + emit");
    let (stdout, ok) = assemble_and_run("list-shadow", &wat);
    assert!(ok, "wasm-interp must exit cleanly:\n{stdout}");
    assert_eq!(parse_scalar(&stdout, "go"), 12, "sum of [2, 4, 6]");
}
