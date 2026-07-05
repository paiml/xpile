//! PMAT-1287 — ADVERSARIAL-VERIFY (skeptic) witness: a family-wide DIFFERENTIAL
//! fuzz over the native-WASM list-MUTATION family against LIVE CPython
//! (`python3`), fuzzed over COMPOSED mutation SEQUENCES rather than single ops.
//!
//! ## The gap this closes (the skeptic's charge)
//!
//! The six in-place mutations shipped back-to-back — `append` (PMAT-1276),
//! `pop` (PMAT-1278), `insert` (PMAT-1282), `del xs[i]` (PMAT-1284),
//! `remove(v)` (PMAT-1285), `reverse()` (PMAT-1286) — each carry a per-op
//! witness pinning ONE op on a FRESH list. The family-wide composition fuzz
//! (`list_family_composition_witness`, PMAT-1273) predates ALL of them and
//! deliberately covers only the READ-ONLY/ALLOCATING family (concat / sorted /
//! reversed / slice + reductions). NOTHING exercises:
//!
//!   * **Mutation ↔ allocation interference** — the bump-heap's core hazard.
//!     `s = sorted(a)` allocates `s` DIRECTLY AFTER `a`'s fixed-capacity
//!     record; a subsequent `a.append(v)` / `a.insert(i, v)` must write into
//!     `a`'s OWN slack (`LIST_GROWTH_SLACK` spare slots reserved at
//!     construction) and must NEVER spill into `s`'s region. A one-slot
//!     capacity-accounting bug corrupts the NEIGHBOUR list silently — a class
//!     no per-op witness (always a fresh, final list) can reach.
//!   * **Count-header consistency across a mutation CHAIN** — every op reads
//!     the live i32 count at `base+0` and grow/shrink ops rewrite it. A chain
//!     `append → insert → del → remove → pop → reverse` threads that header
//!     through six updates; a stale-count read anywhere shears the tail off or
//!     resurrects a deleted element.
//!   * **Allocating ops READING a mutated list** — `sorted(a)` / `a + s` /
//!     `a[lo:hi]` / `sum(a)` / `x in a` after mutations must observe the LIVE
//!     count, not the construction-time literal count.
//!   * **Shift-loop composition** — `insert` shifts right, `del`/`remove`
//!     shift left, `reverse` swaps in place. Interleavings (insert-then-del at
//!     overlapping positions, remove-then-reverse-then-remove over duplicates)
//!     compose the shifts; an off-by-one in any shift bound only surfaces when
//!     a LATER op re-reads the shifted region.
//!
//! This witness lowers a DETERMINISTIC corpus of composed mutation programs
//! through the production `PythonFrontend` (the CLI `--target wasm` profile),
//! emits, assembles + runs each in WABT, and asserts every executed scalar
//! VALUE-MATCHES `python3` running the BYTE-IDENTICAL program (in-place
//! mutations are plain statements and type annotations are legal Python, so
//! the two sources are the same bytes — zero oracle-reimplementation risk).
//!
//! ## Trap posture (fail-LOUD, never silent)
//!
//! A second, wasm-only corpus asserts the DOCUMENTED failure modes actually
//! trap (`unreachable`) instead of silently corrupting:
//!
//!   * `pop` on a drained list, `remove` of a gone value, `del` past the live
//!     count — CPython raises (IndexError / ValueError / IndexError); the WASM
//!     lane must trap. EXCEPTION-PARITY is asserted on the CPython side.
//!   * `append` past the fixed capacity (`literal_count + LIST_GROWTH_SLACK`)
//!     — CPython succeeds (lists grow unboundedly); the WASM lane's bounded
//!     bump-heap posture is an HONEST, documented divergence ONLY IF it traps
//!     loudly. This test pins exactly that: the 17th append onto a 1-literal
//!     list must trap, never wrap, truncate, or spill into a neighbour.
//!
//! ## Fold fingerprints
//!
//! Mutated lists terminate in the sibling witness's order-sensitive fold
//! `acc = acc * 100 + (v + 50)` (every element kept in `[-9, 9]`), combined
//! with lengths / popped values via fixed co-prime weights. A wrong count,
//! a mis-shifted element, or a clobbered neighbour changes the fingerprint.
//! All int fingerprints stay well inside i64; float cases use dyadic values
//! (`.5`-quantised) so the f64 arithmetic is EXACT on both sides.
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
    let dir = std::env::temp_dir().join(format!("xpile-listmut-{}-{}", std::process::id(), tag));
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

/// One composed mutation program. `body` is BYTE-IDENTICAL on both sides:
/// in-place mutations are plain statements and the `list[int]` / `: int`
/// annotations are legal Python, so the WASM-lane source and the CPython
/// oracle source are the same text (no `reversed(...)` materialisation — the
/// corpus uses only the in-place `xs.reverse()`, never the allocating
/// `reversed(xs)`).
struct Case {
    tag: &'static str,
    ret: Ret,
    body: &'static str,
}

/// The deterministic corpus of COMPOSED mutation programs. Every list element
/// is in `[-9, 9]` so the base-100 fold digits stay unambiguous; fingerprint
/// weights are fixed co-primes keeping every int result inside i64 and every
/// float result dyadic-exact.
fn corpus() -> Vec<Case> {
    vec![
        // ---- mutation ↔ allocation INTERFERENCE (the bump-heap hazard) ----
        // sorted(a) allocates DIRECTLY AFTER a's record; the appends + insert
        // then grow a IN PLACE (into its own slack). Both folds must survive:
        // a spill of even one slot out of a's region rewrites s's header or
        // payload and changes accB.
        Case {
            tag: "grow_after_alloc_no_clobber",
            ret: Ret::Int,
            body: "    a: list[int] = [3, 1, 4]\n    s: list[int] = sorted(a)\n    a.append(9)\n    a.append(8)\n    a.insert(0, 7)\n    accA: int = 0\n    for v in a:\n        accA = accA * 100 + (v + 50)\n    accB: int = 0\n    for w in s:\n        accB = accB * 100 + (w + 50)\n    return accA * 1000003 + accB * 997 + len(a) * 31 + len(s)",
        },
        // The mirror: mutate FIRST, allocate AFTER — sorted/concat/slice must
        // read a's LIVE count (5), not its construction-time literal count (4).
        Case {
            tag: "alloc_reads_mutated_count",
            ret: Ret::Int,
            body: "    a: list[int] = [3, 1, 4, 1]\n    a.remove(1)\n    a.reverse()\n    a.append(2)\n    s: list[int] = sorted(a)\n    c: list[int] = a + s\n    ys: list[int] = c[1:6]\n    acc: int = 0\n    for v in ys:\n        acc = acc * 100 + (v + 50)\n    return acc * 1009 + len(c) * 53 + len(s)",
        },
        // A slice TEMPORARY allocated mid-stream, then the source list grows:
        // t's region sits after a's record and must survive the insert shift.
        Case {
            tag: "slice_temp_survives_growth",
            ret: Ret::Int,
            body: "    a: list[int] = [5, 2, 8, 1]\n    t: list[int] = a[1:3]\n    a.insert(0, 3)\n    a.append(6)\n    accA: int = 0\n    for v in a:\n        accA = accA * 100 + (v + 50)\n    accB: int = 0\n    for w in t:\n        accB = accB * 100 + (w + 50)\n    return accA * 1000003 + accB * 997",
        },
        // ---- the SIX-OP churn chain (count-header threading) --------------
        // append → insert → del → remove → pop → reverse in one program; the
        // popped value is folded in so pop's result is also value-checked.
        Case {
            tag: "six_op_churn",
            ret: Ret::Int,
            body: "    a: list[int] = [1, 2, 3]\n    a.append(4)\n    a.insert(0, 5)\n    del a[2]\n    a.remove(3)\n    t: int = a.pop()\n    a.reverse()\n    acc: int = 0\n    for v in a:\n        acc = acc * 100 + (v + 50)\n    return t * 10000019 + acc * 101 + len(a)",
        },
        // ---- shift-loop COMPOSITION ---------------------------------------
        // CPython insert-clamp semantics under composition: -1 → before last,
        // 100 → append, -100 → prepend; the three shifts compose.
        Case {
            tag: "insert_clamp_chain",
            ret: Ret::Int,
            body: "    a: list[int] = [1, 2, 3]\n    a.insert(-1, 7)\n    a.insert(100, 8)\n    a.insert(-100, 9)\n    acc: int = 0\n    for v in a:\n        acc = acc * 100 + (v + 50)\n    return acc",
        },
        // Negative-index deletes recompute against the LIVE (shrinking) count.
        Case {
            tag: "del_negative_chain",
            ret: Ret::Int,
            body: "    a: list[int] = [4, 5, 6, 7]\n    del a[-1]\n    del a[-3]\n    a.insert(1, -3)\n    acc: int = 0\n    for v in a:\n        acc = acc * 100 + (v + 50)\n    return acc",
        },
        // remove() takes the FIRST duplicate; reverse re-orders which one is
        // "first" for the NEXT remove — order-sensitivity of the scan.
        Case {
            tag: "remove_dup_reverse_remove",
            ret: Ret::Int,
            body: "    a: list[int] = [2, 1, 2, 3, 2]\n    a.remove(2)\n    a.reverse()\n    a.remove(2)\n    acc: int = 0\n    for v in a:\n        acc = acc * 100 + (v + 50)\n    return acc",
        },
        // reverse → grow → reverse: the double reversal brackets a shift.
        Case {
            tag: "reverse_grow_reverse",
            ret: Ret::Int,
            body: "    a: list[int] = [1, 2, 3]\n    a.append(4)\n    a.reverse()\n    a.insert(2, 5)\n    a.reverse()\n    acc: int = 0\n    for v in a:\n        acc = acc * 100 + (v + 50)\n    return acc",
        },
        // ---- reads INTERLEAVED with mutations ------------------------------
        // Reductions between mutations: each must see the live count/content.
        Case {
            tag: "reductions_interleaved",
            ret: Ret::Int,
            body: "    a: list[int] = [3, 1, 4]\n    r1: int = sum(a) + max(a)\n    a.append(9)\n    r2: int = sum(a) * 100 + min(a)\n    del a[0]\n    r3: int = sum(a) + len(a) * 1000\n    return r1 * 1000003 + r2 * 997 + r3",
        },
        // Membership scans see removals and re-appends.
        Case {
            tag: "membership_tracks_mutation",
            ret: Ret::Int,
            body: "    a: list[int] = [1, 2, 3]\n    acc: int = 0\n    a.remove(2)\n    if 2 in a:\n        acc = acc + 1\n    if 2 not in a:\n        acc = acc + 10\n    a.append(2)\n    if 2 in a:\n        acc = acc + 100\n    v: int = a.pop()\n    return acc * 10 + v",
        },
        // ---- loop-driven mutation ------------------------------------------
        // Build from EMPTY via loop-append (empty-collection inference), then
        // shift ops over the loop-built record.
        Case {
            tag: "build_from_empty_then_shift",
            ret: Ret::Int,
            body: "    a: list[int] = []\n    i: int = 0\n    while i < 4:\n        a.append(i * 2 - 3)\n        i = i + 1\n    a.insert(2, 0)\n    del a[0]\n    acc: int = 0\n    for v in a:\n        acc = acc * 100 + (v + 50)\n    return acc",
        },
        // Drain via pop in a while loop — the order-sensitive drain folds each
        // popped value, so every intermediate count AND value is pinned.
        Case {
            tag: "pop_drain_after_insert",
            ret: Ret::Int,
            body: "    a: list[int] = [5, 3]\n    a.insert(1, 4)\n    total: int = 0\n    while len(a) > 0:\n        total = total * 100 + (a.pop() + 50)\n    return total",
        },
        // ---- capacity EDGE (inside the documented bound) --------------------
        // Exactly LIST_GROWTH_SLACK (16) appends onto a 1-literal list reach
        // the capacity WITHOUT crossing it — must succeed and value-match.
        // (17 elements overflow the base-100 fold, so a reduction fingerprint
        // pins content; ORDER is covered by the folds above.)
        Case {
            tag: "capacity_edge_16_appends",
            ret: Ret::Int,
            body: "    a: list[int] = [5]\n    i: int = 0\n    while i < 16:\n        a.append(i - 8)\n        i = i + 1\n    return len(a) * 1000003 + sum(a) * 997 + (max(a) + 50) * 31 + (min(a) + 50)",
        },
        // ---- FLOAT twins (dyadic values → EXACT f64 on both sides) ----------
        // The six-op churn over f64 elements (typed remove twin + word shifts).
        Case {
            tag: "f_six_op_churn",
            ret: Ret::Float,
            body: "    a: list[float] = [1.5, 2.5, 3.5]\n    a.append(4.5)\n    a.insert(0, 0.5)\n    del a[1]\n    a.remove(3.5)\n    t: float = a.pop()\n    a.reverse()\n    acc: float = 0.0\n    for v in a:\n        acc = acc * 100.0 + (v + 50.0)\n    return t * 100000.0 + acc",
        },
        // Float interference: sorted(a) allocated, then a grows + removes.
        Case {
            tag: "f_grow_after_alloc",
            ret: Ret::Float,
            body: "    a: list[float] = [3.5, 1.5, 4.5]\n    s: list[float] = sorted(a)\n    a.append(9.5)\n    a.remove(1.5)\n    accA: float = 0.0\n    for v in a:\n        accA = accA * 100.0 + (v + 50.0)\n    accB: float = 0.0\n    for w in s:\n        accB = accB * 100.0 + (w + 50.0)\n    return accA * 1000.0 + accB",
        },
    ]
}

/// One trap-posture program: `body` must TRAP in the WASM lane; `py_outcome`
/// is what BYTE-IDENTICAL CPython does with it (exception-parity, or the one
/// documented bounded-capacity divergence).
struct TrapCase {
    tag: &'static str,
    body: &'static str,
    /// `Some(exc)` — CPython raises `exc` (parity); `None` — CPython SUCCEEDS
    /// (the documented bounded-capacity divergence; loud trap required).
    py_raises: Option<&'static str>,
}

/// Failure modes under COMPOSITION: each trap fires only because an EARLIER
/// mutation changed the live count — a fresh-list per-op witness cannot reach
/// these states.
fn trap_corpus() -> Vec<TrapCase> {
    vec![
        TrapCase {
            tag: "trap_pop_drained",
            body: "    a: list[int] = [1]\n    t: int = a.pop()\n    u: int = a.pop()\n    return t + u",
            py_raises: Some("IndexError"),
        },
        TrapCase {
            tag: "trap_remove_gone",
            body: "    a: list[int] = [2]\n    a.remove(2)\n    a.remove(2)\n    return len(a)",
            py_raises: Some("ValueError"),
        },
        TrapCase {
            tag: "trap_del_oob_after_shrink",
            body: "    a: list[int] = [1, 2]\n    del a[0]\n    del a[1]\n    return len(a)",
            py_raises: Some("IndexError"),
        },
        // The 17th append onto a 1-literal record (capacity = 1 + 16 slack):
        // CPython grows unboundedly; the bounded bump-heap MUST trap loudly —
        // never wrap the count, truncate, or spill into a neighbour region.
        TrapCase {
            tag: "trap_append_past_capacity",
            body: "    a: list[int] = [1]\n    i: int = 0\n    while i < 17:\n        a.append(2)\n        i = i + 1\n    return len(a)",
            py_raises: None,
        },
    ]
}

/// The full `def go()` source for a case body (shared by both sides).
fn go_src(ret: Ret, body: &str) -> String {
    let ret = match ret {
        Ret::Int => "int",
        Ret::Float => "float",
    };
    format!("def go() -> {ret}:\n{body}\n")
}

/// Run every case through CPython and return `tag -> repr(value)`.
fn python_oracle(cases: &[Case]) -> Option<BTreeMap<String, String>> {
    let mut prog = String::new();
    for c in cases {
        prog.push_str(&format!("def {}():\n{}\n", c.tag, c.body));
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
            "PMAT-1287: python3 oracle failed:\n{}",
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

/// Run ONE trap-case body under CPython; returns `Ok(value_repr)` on success
/// or `Err(exception_class_name)` if it raised.
fn python_trap_outcome(body: &str) -> Result<String, String> {
    let prog = format!(
        "def probe():\n{body}\ntry:\n    print('OK='+repr(probe()))\nexcept Exception as e:\n    print('EXC='+type(e).__name__)\n"
    );
    let out = Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn python3");
    assert!(
        out.status.success(),
        "python3 trap-probe crashed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.trim().lines().last().expect("probe output");
    if let Some(v) = line.strip_prefix("OK=") {
        Ok(v.to_string())
    } else if let Some(e) = line.strip_prefix("EXC=") {
        Err(e.to_string())
    } else {
        panic!("unexpected python trap-probe output: {text}");
    }
}

// ---- tests ------------------------------------------------------------------

/// The EMIT path must lower every composed mutation program regardless of WABT
/// (holds on free CI) — including the trap corpus (traps are RUNTIME events;
/// every trap program must still compile).
#[test]
fn mutation_composition_corpus_lowers() {
    for c in &corpus() {
        let src = go_src(c.ret, c.body);
        assert!(
            emit(&src).is_ok(),
            "composed mutation program {} must lower+emit: {:?}\n--- src ---\n{src}",
            c.tag,
            emit(&src)
        );
    }
    for t in &trap_corpus() {
        let src = go_src(Ret::Int, t.body);
        assert!(
            emit(&src).is_ok(),
            "trap program {} must lower+emit (the trap is a RUNTIME event): {:?}",
            t.tag,
            emit(&src)
        );
    }
}

/// The corpus must actually EXERCISE the interference/chain classes — else the
/// fuzz could pass while only running trivial single-op programs.
#[test]
fn corpus_exercises_mutation_interference() {
    let cases = corpus();
    let has = |tag: &str| cases.iter().any(|c| c.tag == tag);
    // the bump-heap hazard classes must be present.
    assert!(
        has("grow_after_alloc_no_clobber"),
        "grow-after-alloc clobber probe missing"
    );
    assert!(
        has("alloc_reads_mutated_count"),
        "alloc-reads-live-count probe missing"
    );
    assert!(has("six_op_churn"), "six-op churn chain missing");
    assert!(has("f_six_op_churn"), "float churn twin missing");
    assert!(
        has("capacity_edge_16_appends"),
        "capacity boundary probe missing"
    );
    // determinism: the corpus is a fixed literal — same tags, same order.
    let tags: Vec<&str> = cases.iter().map(|c| c.tag).collect();
    let tags2: Vec<&str> = corpus().iter().map(|c| c.tag).collect();
    assert_eq!(tags, tags2, "corpus order unstable");
    // the six-op churn must declare ALL FOUR mutation helpers in ONE module
    // (append/pop are inline WAT; insert/del/remove/reverse are helpers) —
    // a missed gate-walker leaves one undeclared and wat2wasm hard-fails.
    let churn = cases.iter().find(|c| c.tag == "six_op_churn").unwrap();
    let wat = emit(&go_src(churn.ret, churn.body)).unwrap();
    for helper in [
        "$__wasm_list_insert_i64",
        "$__wasm_list_delitem",
        "$__wasm_list_remove_i64",
        "$__wasm_list_reverse",
    ] {
        assert!(
            wat.contains(helper),
            "six-op churn must declare {helper} (gate-walker recursion)"
        );
    }
}

/// The load-bearing differential: every composed mutation program's executed
/// WASM scalar value-matches live CPython running the BYTE-IDENTICAL program.
#[test]
fn mutation_composition_matches_cpython() {
    let cases = corpus();

    // The EMIT path holds regardless of WABT (also asserted above).
    for c in &cases {
        emit(&go_src(c.ret, c.body)).unwrap_or_else(|e| panic!("{} lowers: {e}", c.tag));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1287: skipping EXECUTED mutation-composition fuzz — WABT (wat2wasm / \
             wasm-interp) absent. Every composed program lowered through emit_module \
             (asserted in `mutation_composition_corpus_lowers`); a box with WABT + \
             python3 runs every program and value-matches live CPython."
        );
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1287: skipping mutation value-diff — python3 (the oracle) absent.");
        return;
    }

    let oracle = match python_oracle(&cases) {
        Some(o) => o,
        None => {
            eprintln!("PMAT-1287: python3 oracle unavailable — skipping value diff.");
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
                let got = run_i64(&go_src(c.ret, c.body), c.tag);
                if got == want {
                    checked += 1;
                } else {
                    mismatches.push(format!("{}: WASM={got} CPython={want}", c.tag));
                }
            }
            Ret::Float => {
                let want: f64 = expected.parse().expect("float oracle value");
                let got = run_f64(&go_src(c.ret, c.body), c.tag);
                // dyadic corpus → both sides are EXACT; compare exactly.
                if got == want {
                    checked += 1;
                } else {
                    mismatches.push(format!("{}: WASM={got} CPython={want}", c.tag));
                }
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "PMAT-1287: {} WASM/CPython divergence(s) over the mutation-composition corpus:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(
        checked,
        cases.len(),
        "every composed mutation program must be executed and matched"
    );
    eprintln!(
        "PMAT-1287: list-MUTATION composition fuzz PASSED — {checked} composed programs \
         (append/pop/insert/del/remove/reverse INTERLEAVED with sorted/concat/slice/\
         reductions/membership, int AND float) executed in WABT and value-matched live \
         python3. No neighbour-clobber, no stale count, no shift off-by-one."
    );
}

/// Fail-LOUD posture under composition: every trap program must TRAP in the
/// WASM lane (`unreachable`), with CPython exception-PARITY asserted for the
/// three shrink-side traps and the bounded-capacity append pinned as a LOUD
/// (documented) divergence — CPython succeeds, the WASM lane must trap rather
/// than silently wrap/truncate/spill.
#[test]
fn mutation_traps_are_loud() {
    let traps = trap_corpus();

    for t in &traps {
        emit(&go_src(Ret::Int, t.body)).unwrap_or_else(|e| panic!("{} lowers: {e}", t.tag));
    }

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1287: skipping EXECUTED trap-posture probes — WABT absent (every trap \
             program still lowered through emit_module)."
        );
        return;
    }

    for t in &traps {
        let wat = emit(&go_src(Ret::Int, t.body)).unwrap();
        let (raw, trapped) = run_go(&wat, t.tag);
        assert!(
            trapped,
            "{}: the WASM lane must TRAP here, got a silent result: {raw}",
            t.tag
        );
    }

    if !python3_available() {
        eprintln!("PMAT-1287: skipping trap exception-parity — python3 absent.");
        return;
    }
    for t in &traps {
        let outcome = python_trap_outcome(t.body);
        match t.py_raises {
            Some(exc) => {
                assert_eq!(
                    outcome.as_ref().err().map(String::as_str),
                    Some(exc),
                    "{}: CPython parity — expected a {exc}, got {outcome:?}",
                    t.tag
                );
            }
            None => {
                assert!(
                    outcome.is_ok(),
                    "{}: CPython must SUCCEED here (the bounded-capacity trap is the \
                     documented WASM-side divergence), got {outcome:?}",
                    t.tag
                );
            }
        }
    }
    eprintln!(
        "PMAT-1287: trap posture PASSED — {} composed failure modes trap LOUDLY in WASM \
         (3 with CPython exception-parity; the bounded-capacity append pinned as the one \
         documented, loud divergence).",
        traps.len()
    );
}
