//! PMAT-1319 — EXECUTED witness for TWO-GENERATOR comprehensions over dict/set
//! sources (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`): the shared two-generator
//! desugar (`desugar_comp_2gen`, PMAT-502fc/fd) widens its iterable vocabulary
//! from `list[T]` / `range(...)` to `Type::Dict` (iterates KEYS, exactly like
//! the `for k in d:` statement — PMAT-472/742) and `Type::Set` (elements,
//! PMAT-847) — the SAME widening PMAT-1316 (set comp) / PMAT-1317 (dict comp)
//! applied to the ONE-generator desugars, now carried to the nested-loop form.
//! So the exact sugar the PMAT-1316/1317 honesty pins held refused —
//! `{x + y for x in a for y in b}` over hash sources — lowers to the SAME
//! nested `ForEach` + `SetAdd` HIR as the manual
//! `for x in a: for y in b: r.add(x + y)` loop, and the PMAT-1292/1314/1315
//! hash-order gate, the PMAT-1309 param belts, and the PMAT-1310 return
//! relocation all ride it with ZERO WASM-lane edits.
//!
//! ## Desugar ≡ nested statement-form loop
//!
//! `comp_iter_source_binding` classifies each generator's iterable exactly as
//! the statement path does: a set iterates its elements (`over_keys: false`);
//! a dict iterates its keys — lazily (`over_keys: true`) when read-only, or via
//! an owned keys-snapshot (`DictView{Keys}`) + a size-change guard when the
//! source dict is mutated in the function (PMAT-742). Both generators run
//! through it, so a `{f(x, y) for x in A for y in B}` over ANY mix of
//! list/dict/set sources produces the manual loop's HIR. The self-reference
//! belt (PMAT-1316, fired at the TOP of the desugar) already covers both
//! generator legs (PMAT-1318 verified this).
//!
//! ## What executes here (value-matched vs live CPython)
//!
//! Every observable below runs `{expr for x in A for y in B}` over SET sources
//! (the WASM-admitted case) and folds a permutation-invariant reduction over
//! the result:
//!
//! * `check_2gen_set_src` — the headliner `{x + y for x in a for y in b}`
//!   (the flipped PMAT-1316/1317 honesty pin);
//! * `check_2gen_filtered` — a filter on BOTH generators (`if x > 1` / `if
//!   y < 20`) → each loop body wrapped in its `If`;
//! * `check_2gen_dedup` — a cross product that COLLIDES (`a == b == {0,1,2}` →
//!   9 pairs collapse to 5 distinct sums), so set dedup is load-bearing;
//! * `check_2gen_sub` — a non-additive element (`x - y`), 6 distinct;
//! * `check_2gen_growth` — a 5×5 product-free sum reaching 25 distinct
//!   elements, relocating the accumulator past the 16-slot literal slack
//!   through the nested loop's write-back;
//! * `check_2gen_param_pipeline` — a `cross_len(a, b)` taking TWO set params
//!   and building the 2-gen comp from them, called both ways (the PMAT-1309
//!   param-in boundary carried through the sugar).
//!
//! Content observables fold over a BOUND `sorted(t)` list (base-100) or a
//! commutative `sum`, so the CPython cross-check cannot flap on set order.
//!
//! ## What refuses (pinned below)
//!
//! FRONTEND (invalid mHIR): a tuple source (vocabulary boundary — the
//! 1-generator path materialises a homogeneous tuple to a list; this one does
//! not); 3+ generators; and the self-reference belt through a generator.
//! WASM ORDER-GATE (valid mHIR the backend refuses, IDENTICALLY to the manual
//! loop): a 2-gen comp over DICT sources (a nested loop over bump-heap dict-key
//! storage order is order-sensitive); a 2-gen LIST-build over set sources (list
//! ORDER is observable and diverges); and a 2-gen DICT comp with a computed key
//! (last-write-wins depends on order). Each refuses through the sugar exactly
//! as through the manual spelling — the honest boundary. (The Rust lane, whose
//! IndexMap/IndexSet iteration is deterministic, ADMITS the dict-source and
//! computed-key forms; those are covered by the rust-codegen lane, not here.)
//!
//! ## Witness shape
//!
//! Mirrors `set_comp_hash_source_witness.rs`: ONE module, valid plain `python3`
//! AND wasm-frontend-lowerable; `wasm-interp --run-all-exports` zero-invokes
//! every export, so the param-taking `cross_len` is TOTAL under zeroed set
//! pointers (addr-0 count is 0 → the nested loop never runs). Every `check_*`
//! is pinned to a hand-derived constant AND cross-checked against live
//! `python3` on the IDENTICAL source. Gated on `wasm_runtime_available()` — a
//! clean skip (emit + refusal pins still run) without WABT.

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
    ("check_2gen_set_src", 60111213212223),
    ("check_2gen_filtered", 201213),
    ("check_2gen_dedup", 6020304),
    ("check_2gen_sub", 60080918192829),
    ("check_2gen_growth", 250007575),
    ("check_2gen_param_pipeline", 606),
];

/// The single executed module — every export TOTAL under `--run-all-exports`
/// zeroed-arg invocation (`cross_len`'s set params at address 0 read count 0,
/// so the nested loop never runs).
fn corpus_source() -> String {
    r#"def fold_sorted(t: set[int]) -> int:
    xs = sorted(t)
    acc: int = 0
    for y in xs:
        acc = acc * 100 + y
    return acc

def sum_set(t: set[int]) -> int:
    acc: int = 0
    for y in t:
        acc = acc + y
    return acc

def cross_len(a: set[int], b: set[int]) -> int:
    t = {x + y for x in a for y in b}
    return len(t)

def check_2gen_set_src() -> int:
    a: set[int] = {1, 2, 3}
    b: set[int] = {10, 20}
    t = {x + y for x in a for y in b}
    return len(t) * 10000000000000 + fold_sorted(t)

def check_2gen_filtered() -> int:
    a: set[int] = {1, 2, 3}
    b: set[int] = {10, 20}
    t = {x + y for x in a if x > 1 for y in b if y < 20}
    return len(t) * 100000 + fold_sorted(t)

def check_2gen_dedup() -> int:
    a: set[int] = {0, 1, 2}
    b: set[int] = {0, 1, 2}
    t = {x + y for x in a for y in b}
    return len(t) * 1000000 + fold_sorted(t)

def check_2gen_sub() -> int:
    a: set[int] = {10, 20, 30}
    b: set[int] = {1, 2}
    t = {x - y for x in a for y in b}
    return len(t) * 10000000000000 + fold_sorted(t)

def check_2gen_growth() -> int:
    a: set[int] = {1, 2, 3, 4, 5}
    b: set[int] = {100, 200, 300, 400, 500}
    t = {x + y for x in a for y in b}
    r: int = len(t) * 10000000 + sum_set(t)
    return r

def check_2gen_param_pipeline() -> int:
    a: set[int] = {1, 2}
    b: set[int] = {10, 20, 30}
    return cross_len(a, b) * 100 + cross_len(b, a)
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
            "PMAT-1319: python3 oracle failed:\n{}",
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-2gen-{}-{}", std::process::id(), tag));
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
/// machinery the nested desugared loops ride: `$__wasm_dict_set_i` (the shared
/// update-or-insert with the 0-sentinel value that set-add rides), membership
/// via `$__wasm_dict_has_i`, the shared keyed removal, and the TWO-set-param
/// boundary ABI of `cross_len` (the PMAT-1309 param-in surface through the
/// sugar).
#[test]
fn corpus_emits_with_set_add_helpers() {
    let wat = emit(&corpus_source()).expect("corpus must lower + emit");
    for needle in [
        // two set params in — the boundary ABI (PMAT-1309), carried by the sugar.
        "(func $cross_len (param $a i32) (param $b i32) (result i64)",
        // the shared update-or-insert store (set-add's engine) + membership.
        "$__wasm_dict_set_i",
        "$__wasm_dict_has_i",
        // the shared keyed removal (present via the runtime's dict helper set).
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

/// A refusal that must fire at the FRONTEND (`lower` returns Err — the mHIR is
/// never built).
fn expect_frontend_refusal(name: &str, src: &str, needle: &str) {
    let Err(err) = lower(src) else {
        panic!("`{name}` must refuse at the frontend — got a lowered module");
    };
    assert!(
        err.contains(needle),
        "`{name}` frontend refusal should contain {needle:?}, got: {err}"
    );
}

/// A refusal that is VALID mHIR (`lower` Ok) which the WASM order-gate refuses
/// at `emit_module` — so a regression that drops the gate fails a test rather
/// than silently miscompiling a storage-order-dependent build.
fn expect_order_gate_refusal(name: &str, src: &str, needle: &str) {
    lower(src).unwrap_or_else(|e| panic!("`{name}` must LOWER (valid mHIR), got: {e}"));
    let Err(err) = emit(src) else {
        panic!(
            "`{name}` must refuse at the WASM backend — a silent accept here is \
             the PMAT-1292 storage-order miscompile class"
        );
    };
    assert!(
        err.contains(needle),
        "`{name}` order-gate refusal should contain {needle:?}, got: {err}"
    );
}

/// FRONTEND teeth: the vocabulary boundary and the self-reference belt, through
/// the two-generator sugar.
#[test]
fn frontend_refusals_refuse_at_lowering() {
    // Tuple source — the 1-generator path materialises a homogeneous tuple to a
    // list; the two-generator path does not, so it refuses (the widened message
    // names the admitted iterables).
    expect_frontend_refusal(
        "2gen-tuple-src",
        "def go() -> int:\n    a: set[int] = {1, 2}\n    r = {x + y for x in a for y in (10, 20)}\n    return len(r)\n",
        "multi-generator set comprehension over an iterable typing as",
    );
    // Three generators — still deferred.
    expect_frontend_refusal(
        "3gen",
        "def go() -> int:\n    a: set[int] = {1, 2}\n    r = {x + y + z for x in a for y in a for z in a}\n    return len(r)\n",
        "3 `for` clauses",
    );
    // Self-reference through a generator ITERABLE — the shared belt fires at the
    // top of the desugar, BEFORE the 2-gen dispatch (PMAT-1318).
    expect_frontend_refusal(
        "2gen-self-ref",
        "def go() -> int:\n    t: set[int] = {1}\n    b: set[int] = {2, 3}\n    t = {x + y for x in t for y in b}\n    return len(t)\n",
        "while the comprehension itself reads",
    );
}

/// WASM ORDER-GATE teeth: valid mHIR (the sugar lowered) that the backend
/// refuses IDENTICALLY to the manual nested loop — the honest boundary.
#[test]
fn order_gate_refusals_are_valid_mhir_the_wasm_lane_refuses() {
    // A 2-gen comp over DICT sources: a nested loop over bump-heap dict-key
    // storage order is order-sensitive (the manual `for k in d1: for j in d2:`
    // refuses identically).
    expect_order_gate_refusal(
        "2gen-dict-src",
        "def go() -> int:\n    d1: dict[int, int] = {1: 0}\n    d2: dict[int, int] = {2: 0}\n    r = {k + j for k in d1 for j in d2}\n    return len(r)\n",
        "over a dict",
    );
    // A 2-gen LIST build over set sources: list ORDER is observable and diverges
    // from CPython's set-iteration order.
    expect_order_gate_refusal(
        "2gen-list-build",
        "def go() -> int:\n    a: set[int] = {1, 2}\n    b: set[int] = {3, 4}\n    r = [x + y for x in a for y in b]\n    return len(r)\n",
        "order-dependent",
    );
    // A 2-gen DICT comp with a computed key: last-write-wins depends on order.
    expect_order_gate_refusal(
        "2gen-dict-comp-computed-key",
        "def go() -> int:\n    a: set[int] = {1, 2}\n    b: set[int] = {10, 20}\n    r = {x * 100 + y: x + y for x in a for y in b}\n    return len(r)\n",
        "order-dependent",
    );
}

/// PARITY: the two-generator SUGAR and the manual nested loop lower to the same
/// executable — they emit to WAT that runs to the SAME value (the sugar IS the
/// loop). Gated on WABT; a clean skip otherwise.
#[test]
fn sugar_matches_the_manual_nested_loop() {
    let sugar = "def go() -> int:\n    a: set[int] = {1, 2, 3}\n    b: set[int] = {10, 20}\n    t = {x + y for x in a for y in b}\n    xs = sorted(t)\n    acc: int = 0\n    for y in xs:\n        acc = acc * 100 + y\n    return len(t) * 1000000 + acc\n";
    let manual = "def go() -> int:\n    a: set[int] = {1, 2, 3}\n    b: set[int] = {10, 20}\n    t: set[int] = set()\n    for x in a:\n        for y in b:\n            t.add(x + y)\n    xs = sorted(t)\n    acc: int = 0\n    for y in xs:\n        acc = acc * 100 + y\n    return len(t) * 1000000 + acc\n";
    let wat_sugar = emit(sugar).expect("sugar emits");
    let wat_manual = emit(manual).expect("manual emits");
    if !wasm_runtime_available() {
        eprintln!("PMAT-1319: skipping parity execution — WABT not on PATH");
        return;
    }
    let (out_s, ok_s) = assemble_and_run("parity-sugar", &wat_sugar);
    let (out_m, ok_m) = assemble_and_run("parity-manual", &wat_manual);
    assert!(
        ok_s && ok_m,
        "both must run:\nsugar:\n{out_s}\nmanual:\n{out_m}"
    );
    assert_eq!(
        parse_scalar(&out_s, "go"),
        parse_scalar(&out_m, "go"),
        "the 2-gen sugar must run to the SAME value as the manual nested loop"
    );
}

// ---- the executed differential --------------------------------------------------

/// Hand-derived pins → WASM (wat2wasm + wasm-interp) → value equality, then the
/// same source through live CPython. Headlines: `check_2gen_set_src` (the
/// flipped PMAT-1316/1317 honesty pin), `check_2gen_dedup` (cross-product
/// collisions collapse), `check_2gen_growth` (the accumulator relocates past
/// the literal slack through the nested loop), and `check_2gen_param_pipeline`
/// (two set params IN, comp build, called both ways).
#[test]
fn comp_2gen_hash_source_witness_executes_and_matches_cpython() {
    let wat = emit(&corpus_source()).expect("corpus must emit");
    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1319: skipping EXECUTED 2-gen-comp witness — WABT \
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
        eprintln!("PMAT-1319: python3 not available — pins stand on the hand-derived values");
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
