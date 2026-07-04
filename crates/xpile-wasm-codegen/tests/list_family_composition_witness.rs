//! PMAT-1273 — a family-wide DIFFERENTIAL witness for the whole native-WASM
//! list-op family against LIVE CPython (`python3`), fuzzed over COMPOSED
//! op chains rather than single ops.
//!
//! ## The gap this closes
//!
//! Every sibling list witness pins ONE op applied to a FRESH named list:
//! `list_reduction_adversarial_witness` (sum/min/max), `list_slice_adversarial_
//! witness` (`xs[lo:hi]`), `list_contains_witness` (`x in xs`), plus the
//! per-op sorted/reversed/concat/any/all/enumerate/zip executed witnesses. Each
//! catches the cases its author enumerated for that op in isolation. NONE
//! exercises the INTERACTION between the ALLOCATING ops applied in sequence —
//! and the bump-heap list runtime's danger is precisely there:
//!
//!   * `a + b` (concat), `sorted(xs)`, `reversed(xs)`, and `xs[lo:hi]` each
//!     ALLOCATE a NEW list region off the shared bump pointer. Chaining them
//!     (`sorted(reversed(a + b))[1:5]`) allocates FOUR nested temporaries whose
//!     base pointers must not clobber one another — a path no single-op witness
//!     reaches, and exactly where an off-by-one region size or a stale base
//!     pointer silently corrupts a downstream read.
//!   * A REDUCTION (`sum`/`min`/`max`) or a MEMBERSHIP scan (`x in xs`) applied
//!     to an ALLOCATED list (not a source literal) reads a region the emitter
//!     produced, so a wrong element-count header on the allocated temporary
//!     yields a silently short/long fold.
//!   * The gate walkers must recurse into EVERY operand of a composed
//!     expression: a chain declares `$__wasm_list_concat_i` + `_reversed_i` +
//!     `_sorted_i` + `_slice_i` + (`_sum_i64` | `_minmax_i64` |
//!     `_contains_i64`) all in one module — a missed gate-walker recursion
//!     leaves a helper undeclared and `wat2wasm` hard-fails here.
//!
//! This witness lowers a DETERMINISTIC corpus of COMPOSED Python programs
//! through the same profile the CLI uses for `--target wasm`, emits, assembles +
//! runs each in WABT, and asserts every executed scalar VALUE-MATCHES `python3`
//! running the byte-identical (idiomatic) program. `python3` is the literal
//! oracle — zero reimplementation risk.
//!
//! ## Fold fingerprints
//!
//! A list-PRODUCING chain (`sorted`/`reversed`/`concat`/`slice`) terminates in
//! an order-sensitive fold `len(ys)*1e8 + Σ acc*100 + (v + 50)`, so a single
//! matching result certifies the chain's length AND element order/content: an
//! off-by-one region, a wrong wrap, or a stray/clobbered element changes the
//! fingerprint. A REDUCTION chain returns its scalar directly. The `+ 50` bias
//! keeps each folded term in `[0, 100)` so the base-100 positional digits stay
//! unambiguous (every element used here is in `[-49, 49]`).
//!
//! ## Gating
//!
//! Runs the executed diff only when BOTH WABT (`wat2wasm`/`wasm-interp`) AND
//! `python3` are present. On free CI (no WABT) it skips cleanly after still
//! exercising the EMIT path for every composed program.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use depyler_frontend::PythonFrontend;
use xpile_frontend::{AliasSemantics, Frontend, LoweringProfile};
use xpile_meta_hir::Module;
use xpile_wasm_codegen::{emit_module, wasm_runtime_available};

/// The lowering profile the CLI uses for `--target wasm`.
fn wasm_profile() -> LoweringProfile {
    LoweringProfile {
        alias_semantics: AliasSemantics::Reference,
        runtime_abort: true,
    }
}

/// Lower Python source → meta-HIR the way the CLI does for a WASM target.
fn lower(src: &str) -> Result<Module, String> {
    PythonFrontend
        .parse_and_lower_profiled(Path::new("witness.py"), src, wasm_profile())
        .map_err(|e| format!("frontend: {e}"))
}

/// The FULL pipeline: Python source → meta-HIR → WAT text.
fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

/// Assemble the real-emitted WAT + run its zero-arg `go` export in WABT.
/// Returns `(raw_result_string, trapped)`.
fn run_go(wat: &str, tag: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-listcomp-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_path = dir.join("go.wat");
    let wasm_path = dir.join("go.wasm");
    std::fs::write(&wat_path, wat).expect("write wat");

    let assemble = Command::new("wat2wasm")
        .arg(&wat_path)
        .arg("-o")
        .arg(&wasm_path)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        assemble.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT (first 6k)---\n{}",
        String::from_utf8_lossy(&assemble.stderr),
        &wat[..wat.len().min(6144)]
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    if !run.status.success() || stdout.contains("unreachable executed") {
        return (stdout.into_owned(), true);
    }
    let line = stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=>"))
        .unwrap_or_else(|| panic!("no `go` export in interp output for {tag}:\n{stdout}"));
    (line.rsplit(':').next().unwrap().trim().to_string(), false)
}

/// Run a `go() -> int` probe → SIGNED i64 (wasm-interp prints i64 unsigned).
fn run_i64(src: &str, tag: &str) -> i64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let (raw, trapped) = run_go(&wat, tag);
    assert!(!trapped, "{tag} trapped unexpectedly:\n{raw}");
    raw.parse::<u64>()
        .unwrap_or_else(|_| panic!("parse i64 result {raw:?} for {tag}")) as i64
}

/// Run a `go() -> float` probe → f64.
fn run_f64(src: &str, tag: &str) -> f64 {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let (raw, trapped) = run_go(&wat, tag);
    assert!(!trapped, "{tag} trapped unexpectedly:\n{raw}");
    raw.parse::<f64>()
        .unwrap_or_else(|_| panic!("parse f64 result {raw:?} for {tag}"))
}

/// `true` iff `python3` is invocable.
fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The scalar type a composed program returns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ret {
    Int,
    Float,
}

/// One composed program. `wasm_body` is the typed WASM-lane function body;
/// `py_body` is the byte-idiomatic CPython body computing the SAME scalar (it
/// materialises `reversed(...)` with `list(...)` where the WASM lane already
/// produces a real list). Both end in a `return`.
struct Case {
    tag: &'static str,
    ret: Ret,
    wasm_body: &'static str,
    py_body: &'static str,
}

/// The order-sensitive fold appended to a list-PRODUCING chain over a named
/// list `ys` — identical text runs on both the WASM lane and CPython (in both,
/// `ys` is a real list by the time the fold runs).
const FOLD: &str = "    acc: int = 0\n    for v in ys:\n        acc = acc * 100 + (v + 50)\n    return len(ys) * 100000000 + acc";

/// The full deterministic corpus of COMPOSED programs. Every value used is in
/// `[-9, 9]` so the base-100 fold digits stay unambiguous. Reductions return
/// their scalar directly; list-producing chains terminate in [`FOLD`].
fn corpus() -> Vec<Case> {
    vec![
        // ---- list-PRODUCING chains (order-sensitive fold) ----------------
        // concat, then fold — the allocated concat region read in order.
        Case {
            tag: "concat_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    ys: list[int] = a + b\n{FOLD}",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    ys = a + b\n{FOLD}",
        },
        // concat → sort (ascending) → fold: sorted reads the allocated concat.
        Case {
            tag: "sorted_concat_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    ys: list[int] = sorted(c)\n{FOLD}",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    ys = sorted(c)\n{FOLD}",
        },
        // concat → reversed → fold: reversed reads the allocated concat.
        Case {
            tag: "reversed_concat_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    ys: list[int] = reversed(c)\n{FOLD}",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    ys = list(reversed(c))\n{FOLD}",
        },
        // concat → reversed → sort: double allocation, sort reads a reversed temp.
        Case {
            tag: "rev_then_sort_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    r: list[int] = reversed(c)\n    ys: list[int] = sorted(r)\n{FOLD}",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    r = list(reversed(c))\n    ys = sorted(r)\n{FOLD}",
        },
        // sort → slice: an interior cut of an allocated sorted list.
        Case {
            tag: "sort_slice_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [5, 2, 8, 1, 9, 3]\n    s: list[int] = sorted(a)\n    ys: list[int] = s[1:4]\n{FOLD}",
            py_body: "    a = [5, 2, 8, 1, 9, 3]\n    s = sorted(a)\n    ys = s[1:4]\n{FOLD}",
        },
        // concat → slice: cut across the concat seam, with a negative bound.
        Case {
            tag: "concat_neg_slice_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    ys: list[int] = c[-4:-1]\n{FOLD}",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    ys = c[-4:-1]\n{FOLD}",
        },
        // reversed → slice: the reversed temp is itself sliced.
        Case {
            tag: "rev_slice_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [1, 2, 3, 4, 5]\n    r: list[int] = reversed(a)\n    ys: list[int] = r[1:4]\n{FOLD}",
            py_body: "    a = [1, 2, 3, 4, 5]\n    r = list(reversed(a))\n    ys = r[1:4]\n{FOLD}",
        },
        // the DEEP chain: concat → reversed → sorted → slice → fold.
        Case {
            tag: "deep_chain_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4, 1, 5]\n    b: list[int] = [9, 2, 6]\n    c: list[int] = a + b\n    r: list[int] = reversed(c)\n    s: list[int] = sorted(r)\n    ys: list[int] = s[1:5]\n{FOLD}",
            py_body: "    a = [3, 1, 4, 1, 5]\n    b = [9, 2, 6]\n    c = a + b\n    r = list(reversed(c))\n    s = sorted(r)\n    ys = s[1:5]\n{FOLD}",
        },
        // slice → sort: sort an allocated slice (order re-established).
        Case {
            tag: "slice_then_sort_fold",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [9, 3, 7, 1, 8, 2]\n    c: list[int] = a[1:5]\n    ys: list[int] = sorted(c)\n{FOLD}",
            py_body: "    a = [9, 3, 7, 1, 8, 2]\n    c = a[1:5]\n    ys = sorted(c)\n{FOLD}",
        },
        // ---- REDUCTION chains (scalar returned directly) -----------------
        // sum of a concat.
        Case {
            tag: "sum_concat",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    return sum(c)",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    return sum(c)",
        },
        // sum of a sorted concat (== sum(concat), but folds an allocated sort).
        Case {
            tag: "sum_sorted_concat",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    s: list[int] = sorted(c)\n    return sum(s)",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    s = sorted(c)\n    return sum(s)",
        },
        // max of a concat.
        Case {
            tag: "max_concat",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    return max(c)",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    return max(c)",
        },
        // min of a reversed list (order must not change the extremum).
        Case {
            tag: "min_reversed",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4, 1, 5]\n    r: list[int] = reversed(a)\n    return min(r)",
            py_body: "    a = [3, 1, 4, 1, 5]\n    r = list(reversed(a))\n    return min(r)",
        },
        // sum of a slice of a sorted list.
        Case {
            tag: "sum_sort_slice",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [5, 2, 8, 1, 9, 3]\n    s: list[int] = sorted(a)\n    ys: list[int] = s[1:4]\n    return sum(ys)",
            py_body: "    a = [5, 2, 8, 1, 9, 3]\n    s = sorted(a)\n    ys = s[1:4]\n    return sum(ys)",
        },
        // max of a deep rev→sort chain.
        Case {
            tag: "max_of_rev_sort",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    r: list[int] = reversed(c)\n    s: list[int] = sorted(r)\n    return max(s)",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    r = list(reversed(c))\n    s = sorted(r)\n    return max(s)",
        },
        // two reductions over the SAME allocated list (sum + max).
        Case {
            tag: "sum_plus_max_concat",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    return sum(c) + max(c)",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    return sum(c) + max(c)",
        },
        // ---- MEMBERSHIP over an ALLOCATED list ---------------------------
        // membership scan over a sorted (allocated) list — present/absent/not-in.
        Case {
            tag: "mem_in_sorted",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [5, 2, 8, 1, 9]\n    s: list[int] = sorted(a)\n    acc: int = 0\n    if 8 in s:\n        acc = acc + 1\n    if 7 in s:\n        acc = acc + 10\n    if 7 not in s:\n        acc = acc + 100\n    return acc",
            py_body: "    a = [5, 2, 8, 1, 9]\n    s = sorted(a)\n    acc = 0\n    if 8 in s:\n        acc = acc + 1\n    if 7 in s:\n        acc = acc + 10\n    if 7 not in s:\n        acc = acc + 100\n    return acc",
        },
        // membership scan over a concat (allocated) list.
        Case {
            tag: "mem_in_concat",
            ret: Ret::Int,
            wasm_body: "    a: list[int] = [3, 1, 4]\n    b: list[int] = [1, 5, 9]\n    c: list[int] = a + b\n    acc: int = 0\n    if 9 in c:\n        acc = acc + 1\n    if 8 in c:\n        acc = acc + 10\n    return acc",
            py_body: "    a = [3, 1, 4]\n    b = [1, 5, 9]\n    c = a + b\n    acc = 0\n    if 9 in c:\n        acc = acc + 1\n    if 8 in c:\n        acc = acc + 10\n    return acc",
        },
        // ---- FLOAT twins (tolerance-compared) ----------------------------
        // float deep chain summed.
        Case {
            tag: "f_deep_chain_sum",
            ret: Ret::Float,
            wasm_body: "    a: list[float] = [3.5, 1.5, 4.5]\n    b: list[float] = [1.5, 5.5]\n    c: list[float] = a + b\n    r: list[float] = reversed(c)\n    s: list[float] = sorted(r)\n    ys: list[float] = s[1:4]\n    return sum(ys)",
            py_body: "    a = [3.5, 1.5, 4.5]\n    b = [1.5, 5.5]\n    c = a + b\n    r = list(reversed(c))\n    s = sorted(r)\n    ys = s[1:4]\n    return sum(ys)",
        },
        // float sum of a sorted concat.
        Case {
            tag: "f_sum_sorted_concat",
            ret: Ret::Float,
            wasm_body: "    a: list[float] = [3.5, 1.5]\n    b: list[float] = [4.5, 2.5]\n    c: list[float] = a + b\n    s: list[float] = sorted(c)\n    return sum(s)",
            py_body: "    a = [3.5, 1.5]\n    b = [4.5, 2.5]\n    c = a + b\n    s = sorted(c)\n    return sum(s)",
        },
        // float max of a concat.
        Case {
            tag: "f_max_concat",
            ret: Ret::Float,
            wasm_body: "    a: list[float] = [3.5, 1.5]\n    b: list[float] = [4.5, 2.5]\n    c: list[float] = a + b\n    return max(c)",
            py_body: "    a = [3.5, 1.5]\n    b = [4.5, 2.5]\n    c = a + b\n    return max(c)",
        },
        // float min of a reversed list.
        Case {
            tag: "f_min_reversed",
            ret: Ret::Float,
            wasm_body: "    a: list[float] = [3.5, 1.5, 4.5]\n    r: list[float] = reversed(a)\n    return min(r)",
            py_body: "    a = [3.5, 1.5, 4.5]\n    r = list(reversed(a))\n    return min(r)",
        },
        // float sum of a slice of a sorted list.
        Case {
            tag: "f_sum_sort_slice",
            ret: Ret::Float,
            wasm_body: "    a: list[float] = [5.5, 2.5, 8.5, 1.5, 9.5]\n    s: list[float] = sorted(a)\n    ys: list[float] = s[1:4]\n    return sum(ys)",
            py_body: "    a = [5.5, 2.5, 8.5, 1.5, 9.5]\n    s = sorted(a)\n    ys = s[1:4]\n    return sum(ys)",
        },
    ]
}

/// Substitute the shared [`FOLD`] tail into a body template (`{FOLD}` marker).
fn expand(body: &str) -> String {
    body.replace("{FOLD}", FOLD)
}

/// The WASM-lane source for a case.
fn wasm_src(c: &Case) -> String {
    let ret = match c.ret {
        Ret::Int => "int",
        Ret::Float => "float",
    };
    format!("def go() -> {ret}:\n{}\n", expand(c.wasm_body))
}

/// Run every case through CPython and return `tag -> repr(value)`.
fn python_oracle(cases: &[Case]) -> Option<BTreeMap<String, String>> {
    let mut prog = String::new();
    for c in cases {
        prog.push_str(&format!("def {}():\n{}\n", c.tag, expand(c.py_body)));
    }
    for c in cases {
        prog.push_str(&format!("print('{}='+repr({}()))\n", c.tag, c.tag));
    }
    let out = Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "PMAT-1273: python3 oracle failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut map = BTreeMap::new();
    for kv in text.trim().lines() {
        let (k, v) = kv.split_once('=').expect("tag=value");
        map.insert(k.to_string(), v.to_string());
    }
    Some(map)
}

// ---- tests ------------------------------------------------------------------

/// The EMIT path must lower every composed program regardless of WABT (holds on
/// free CI) — a full-family smoke over concat/sorted/reversed/slice/sum/min/max/
/// membership INTERACTIONS.
#[test]
fn composition_corpus_lowers() {
    for c in &corpus() {
        let src = wasm_src(c);
        assert!(
            emit(&src).is_ok(),
            "composed program {} must lower+emit: {:?}\n--- src ---\n{src}",
            c.tag,
            emit(&src)
        );
    }
}

/// The corpus must actually EXERCISE nested allocation — else the fuzz could
/// pass while only ever running trivial single-op programs.
#[test]
fn corpus_exercises_nested_allocation() {
    let cases = corpus();
    let has = |tag: &str| cases.iter().any(|c| c.tag == tag);
    // the deep four-alloc chain (concat→reversed→sorted→slice) must be present.
    assert!(has("deep_chain_fold"), "deep nested-alloc chain missing");
    assert!(has("f_deep_chain_sum"), "float deep chain missing");
    // a reduction over an allocated list AND a membership over an allocated list.
    assert!(has("sum_sorted_concat"), "reduction-over-alloc missing");
    assert!(has("mem_in_sorted"), "membership-over-alloc missing");
    // determinism: the corpus is a fixed literal — same tags, same order.
    let tags: Vec<&str> = cases.iter().map(|c| c.tag).collect();
    let tags2: Vec<&str> = corpus().iter().map(|c| c.tag).collect();
    assert_eq!(tags, tags2, "corpus order unstable");
    // every deep chain declares all four allocating helpers in one module.
    let wat = emit(&wasm_src(
        cases.iter().find(|c| c.tag == "deep_chain_fold").unwrap(),
    ))
    .unwrap();
    for helper in [
        "$__wasm_list_concat_i",
        "$__wasm_list_reversed_i",
        "$__wasm_list_sorted_i",
        "$__wasm_list_slice_i",
    ] {
        assert!(
            wat.contains(helper),
            "deep chain must declare {helper} (gate-walker recursion)"
        );
    }
}

/// The load-bearing differential: every composed program's executed WASM scalar
/// value-matches live CPython running the byte-identical (idiomatic) program.
#[test]
fn composition_family_matches_cpython() {
    let cases = corpus();

    // The EMIT path holds regardless of WABT (also asserted above).
    for c in &cases {
        emit(&wasm_src(c)).unwrap_or_else(|e| panic!("{} lowers: {e}", c.tag));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1273: skipping EXECUTED composition fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every composed program lowered through emit_module \
             (asserted in `composition_corpus_lowers`); a box with WABT + python3 runs \
             every program and value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1273: skipping composition value-diff — python3 (the oracle) absent.");
        return;
    }

    let oracle = match python_oracle(&cases) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1273: python3 oracle unavailable — skipping value diff.");
            return;
        }
    };

    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    for c in &cases {
        let expected = oracle
            .get(c.tag)
            .unwrap_or_else(|| panic!("CPython oracle missing {}", c.tag));
        match c.ret {
            Ret::Int => {
                let want: i64 = expected.parse().expect("int oracle value");
                let got = run_i64(&wasm_src(c), c.tag);
                if got == want {
                    checked += 1;
                } else {
                    mismatches.push(format!("{}: WASM={got} CPython={want}", c.tag));
                }
            }
            Ret::Float => {
                let want: f64 = expected.parse().expect("float oracle value");
                let got = run_f64(&wasm_src(c), c.tag);
                if (got - want).abs() < 1e-9 * want.abs().max(1.0) {
                    checked += 1;
                } else {
                    mismatches.push(format!("{}: WASM={got} CPython={want}", c.tag));
                }
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1273: {} WASM/CPython divergence(s) over the list-composition corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(
        checked,
        cases.len(),
        "every composed program must be executed and matched"
    );
    eprintln!(
        "PMAT-1273: list-composition family fuzz PASSED — {checked} composed programs \
         (concat/sorted/reversed/slice + sum/min/max/membership INTERACTIONS, int AND \
         float) executed in WABT and value-matched live python3. No silent divergence in \
         nested-allocation chains."
    );
}

// ---------------------------------------------------------------------------
// HONEST REFUSALS — composition shapes OUTSIDE the named-list subset must
// error at emit time, not silently miscompile a temporary's base pointer.
// ---------------------------------------------------------------------------

/// Slicing a NON-NAME list (a `sorted(...)` temporary, not a bound name) must
/// refuse — the lane needs a declared base-pointer, not an inline temporary.
#[test]
fn slice_of_temporary_refuses_honestly() {
    let src = "def go() -> int:\n    a: list[int] = [3, 1, 4, 1, 5]\n    ys: list[int] = sorted(a)[1:3]\n    return len(ys)\n";
    let err = emit(src).expect_err("slicing a sorted() temporary must refuse");
    assert!(
        err.contains("non-name list") || err.contains("bind the list to a name"),
        "temporary-slice refusal should name the shape, got: {err}"
    );
}

/// `min` / `max` of an EMPTY list is a runtime ValueError in CPython; the WASM
/// lane must TRAP (`unreachable`), never read past a length-0 region. Uses a
/// param so the empty list is a genuine 0-count header.
#[test]
fn min_of_empty_traps_like_cpython() {
    if !wasm_runtime_available() {
        eprintln!("PMAT-1273: WABT absent — empty-min trap check skipped (still lowered).");
        // still assert it lowers
        let src = "def probe(xs: list[int]) -> int:\n    return min(xs)\ndef go() -> int:\n    xs: list[int] = []\n    return probe(xs)\n";
        assert!(emit(src).is_ok(), "empty-min must still lower");
        return;
    }
    let src = "def probe(xs: list[int]) -> int:\n    return min(xs)\ndef go() -> int:\n    xs: list[int] = []\n    return probe(xs)\n";
    let wat = emit(src).expect("empty-min lowers");
    let (out, trapped) = run_go(&wat, "min_empty");
    assert!(
        trapped,
        "min([]) must TRAP (CPython ValueError analogue), got: {out}"
    );
}
