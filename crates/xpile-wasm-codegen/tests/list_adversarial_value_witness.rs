//! PMAT-1275 — EXECUTED adversarial-VALUE differential witness for the native-WASM
//! READ-ONLY `list[scalar]` family (`count` / `index` / membership / slice /
//! `min` / `max` / `sum` / `concat`), diffed against **live** CPython.
//!
//! ## The coverage gap this closes
//!
//! Every shipped per-op witness for this family exercises only NON-NEGATIVE small
//! literals — PMAT-1274 (`count`/`index`) uses `[3, 7, 3, 3, 9]` / `[1.5, 2.5]`;
//! PMAT-1262 (membership) and PMAT-1272 (slice) likewise use only positive values.
//! Nothing locks in the behaviour of the underlying helpers on **negative i64
//! element loads**, **negative needles**, or the **`-0.0` / `0.0` f64 equality**
//! quirk (`[-0.0].count(0.0) == 1` in CPython because `0.0 == -0.0`). A regression
//! that mishandled sign in `$__wasm_list_{count,index,minmax,sum}_*` or that
//! narrowed `f64.eq` would slip through the existing suite. This witness is the
//! adversarial-VALUE axis: a deterministic corpus of programs whose values are
//! chosen to be hostile (negatives, negative needles, signed-zero, boundary slice
//! bounds over a negative-valued list), each EXECUTED in WABT and asserted to
//! VALUE-MATCH the byte-identical program run through live `python3` — the oracle
//! is computed, never hard-coded, so a divergence surfaces as a real diff.
//!
//! ## The one documented, by-design divergence
//!
//! `reversed(xs)` is an ITERATOR in CPython, so `reversed(xs).index(v)` raises
//! `AttributeError` — but the WASM lane models `reversed` as an *allocating list*
//! op (PMAT-1253), so it accepts that form and returns a value. This is
//! deliberate (the whole list lane treats `sorted`/`reversed`/`concat` as
//! list-producing; the composition witness materialises `list(reversed(...))` for
//! its own CPython oracle for exactly this reason). [`reversed_model_is_list`]
//! documents it: the CPython-VALID consuming form (`for v in reversed(xs)`)
//! value-matches, so no VALID Python program is miscompiled — only invalid
//! Python (a list method on an iterator) is over-accepted.
//!
//! Gated on [`wasm_runtime_available`] — a clean skip (still asserting every case
//! LOWERS + EMITS) on a host without WABT / `python3`, so free CI stays green.

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

/// FULL pipeline: Python source → meta-HIR → WAT text.
fn emit(src: &str) -> Result<String, String> {
    emit_module(&lower(src)?).map_err(|e| format!("wasm-codegen: {e}"))
}

/// `true` iff `python3` is invocable (the value oracle).
fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Assemble + run the emitted WAT's zero-arg `go` export in WABT. Returns
/// `Some(i64)` on a clean run (the printed `go` result, reinterpreted signed) or
/// `None` if the module TRAPPED (`unreachable` — the `index`-miss / OOB path).
fn run_go_int(src: &str, tag: &str) -> Option<i64> {
    let wat = emit(src).unwrap_or_else(|e| panic!("emit failed for {tag}: {e}"));
    let dir = std::env::temp_dir().join(format!("advval-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&dir).expect("create work dir");
    let wat_p = dir.join("go.wat");
    let wasm_p = dir.join("go.wasm");
    std::fs::write(&wat_p, &wat).expect("write wat");

    let asm = Command::new("wat2wasm")
        .arg(&wat_p)
        .arg("-o")
        .arg(&wasm_p)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        asm.status.success(),
        "wat2wasm failed for {tag}:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&asm.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_p)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout);
    if !run.status.success() || stdout.contains("unreachable") {
        return None;
    }
    let line = stdout
        .lines()
        .find(|l| l.starts_with("go(") && l.contains("=>"))
        .unwrap_or_else(|| panic!("no `go` export in interp output for {tag}:\n{stdout}"));
    let raw = line.rsplit(':').next().unwrap().trim();
    Some(
        raw.parse::<u64>()
            .unwrap_or_else(|_| panic!("parse i64 {raw:?} for {tag}")) as i64,
    )
}

/// Run the byte-identical `go()` through live CPython. Returns `Some(i64)` for a
/// clean integer result, or `None` if CPython raised (the oracle side of a trap).
fn cpython_int(src: &str, tag: &str) -> Option<i64> {
    let prog =
        format!("{src}\ntry:\n    print(int(go()))\nexcept Exception:\n    print('RAISED')\n");
    let out = Command::new("python3")
        .arg("-c")
        .arg(&prog)
        .output()
        .unwrap_or_else(|_| panic!("spawn python3 for {tag}"));
    assert!(out.status.success(), "python3 process failed for {tag}");
    let s = String::from_utf8_lossy(&out.stdout);
    let last = s.lines().last().unwrap_or("").trim();
    if last == "RAISED" {
        None
    } else {
        Some(
            last.parse::<i64>()
                .unwrap_or_else(|_| panic!("parse cpython {last:?} for {tag}")),
        )
    }
}

/// A deterministic adversarial-VALUE program. `go()` always returns an `int`
/// (booleans / floats are folded into an int checksum) so the WABT result and the
/// CPython result compare as signed integers — or both are `None` (trap ↔ raise).
struct Case {
    tag: &'static str,
    src: &'static str,
}

/// The corpus. Every value is chosen to be hostile to a sign / signed-zero /
/// boundary bug; the CPython answer is COMPUTED, never assumed.
fn corpus() -> Vec<Case> {
    vec![
        // ---- count / index over NEGATIVE element values + negative needles ----
        Case { tag: "neg_count_dupe", src: "def go() -> int:\n    xs: list[int] = [-5, -5, 3, -5]\n    return xs.count(-5)\n" },
        Case { tag: "neg_index_first", src: "def go() -> int:\n    xs: list[int] = [-5, 7, -5]\n    return xs.index(-5)\n" },
        Case { tag: "neg_index_mid", src: "def go() -> int:\n    xs: list[int] = [7, -3, 9]\n    return xs.index(-3)\n" },
        Case { tag: "neg_count_absent", src: "def go() -> int:\n    xs: list[int] = [-1, -2, -3]\n    return xs.count(-9)\n" },
        // index MISS → CPython ValueError ↔ WASM trap (both None).
        Case { tag: "neg_index_miss", src: "def go() -> int:\n    xs: list[int] = [-1, -2]\n    return xs.index(-3)\n" },
        // ---- membership over negatives ----
        Case { tag: "neg_mem_hit", src: "def go() -> int:\n    xs: list[int] = [-8, 2, -1]\n    if -8 in xs:\n        return 1\n    return 0\n" },
        Case { tag: "neg_notmem", src: "def go() -> int:\n    xs: list[int] = [-8, 2, -1]\n    if -9 not in xs:\n        return 1\n    return 0\n" },
        // ---- adversarial slice bounds over a NEGATIVE-valued list ----
        Case { tag: "slice_negneg", src: "def go() -> int:\n    xs: list[int] = [-1, -2, -3, -4, -5]\n    ys: list[int] = xs[-3:-1]\n    return ys[0] * 10 + ys[1]\n" },
        Case { tag: "slice_inverted", src: "def go() -> int:\n    xs: list[int] = [10, 20, 30]\n    ys: list[int] = xs[3:1]\n    return len(ys)\n" },
        Case { tag: "slice_oob_clamp", src: "def go() -> int:\n    xs: list[int] = [4, 5, 6]\n    ys: list[int] = xs[0:100]\n    return len(ys) * 100 + ys[2]\n" },
        Case { tag: "slice_neg_under", src: "def go() -> int:\n    xs: list[int] = [4, 5, 6]\n    ys: list[int] = xs[-100:2]\n    return len(ys) * 10 + ys[0]\n" },
        // ---- count / index after concat with negatives ----
        Case { tag: "concat_count_neg", src: "def go() -> int:\n    a: list[int] = [-1, 2]\n    b: list[int] = [-1, -1]\n    c: list[int] = a + b\n    return c.count(-1)\n" },
        // ---- min / max / sum over negatives ----
        Case { tag: "min_neg", src: "def go() -> int:\n    xs: list[int] = [-1, -9, -3]\n    return min(xs)\n" },
        Case { tag: "max_neg", src: "def go() -> int:\n    xs: list[int] = [-1, -9, -3]\n    return max(xs)\n" },
        Case { tag: "sum_neg", src: "def go() -> int:\n    xs: list[int] = [-5, 3, -1]\n    return sum(xs)\n" },
        // ---- float: -0.0 / 0.0 equality quirk (CPython: 0.0 == -0.0) ----
        Case { tag: "f_negzero_count", src: "def go() -> int:\n    xs: list[float] = [-0.0, 1.0]\n    return xs.count(0.0)\n" },
        Case { tag: "f_zero_mem", src: "def go() -> int:\n    xs: list[float] = [-0.0]\n    if 0.0 in xs:\n        return 1\n    return 0\n" },
        Case { tag: "f_neg_count", src: "def go() -> int:\n    xs: list[float] = [-1.5, -1.5, 2.5]\n    return xs.count(-1.5)\n" },
        Case { tag: "f_neg_index", src: "def go() -> int:\n    xs: list[float] = [2.5, -1.5, 9.0]\n    return xs.index(-1.5)\n" },
        Case { tag: "f_min_neg", src: "def go() -> int:\n    xs: list[float] = [-1.5, -9.5, 2.0]\n    m: float = min(xs)\n    if m == -9.5:\n        return 1\n    return 0\n" },
    ]
}

/// The EXECUTED adversarial-VALUE differential — every case run in WABT and
/// value-matched to live CPython (trap ↔ raise both count as agreement).
#[test]
fn adversarial_values_match_cpython() {
    // First assert the WHOLE corpus LOWERS + EMITS (holds without any tooling).
    for c in corpus() {
        emit(c.src).unwrap_or_else(|e| panic!("{} failed to lower+emit: {e}", c.tag));
    }

    if !wasm_runtime_available() {
        eprintln!("PMAT-1275: WABT absent — emit-only check passed, execution skipped.");
        return;
    }
    if !python3_available() {
        eprintln!("PMAT-1275: python3 (the value oracle) absent — execution diff skipped.");
        return;
    }

    let mut n = 0;
    for c in corpus() {
        let wasm = run_go_int(c.src, c.tag);
        let py = cpython_int(c.src, c.tag);
        assert_eq!(
            wasm, py,
            "PMAT-1275 DIVERGENCE {}: wasm={wasm:?} cpython={py:?}\n---src---\n{}",
            c.tag, c.src
        );
        n += 1;
    }
    eprintln!(
        "PMAT-1275: {n} adversarial-VALUE programs (negatives / negative needles / \
         signed-zero / boundary slices) executed in WABT and value-matched live python3."
    );
}

/// Documents the ONE by-design divergence: `reversed(xs)` is modelled as an
/// allocating LIST (not a CPython iterator). Consuming it the CPython-VALID way
/// (`for v in reversed(xs)`) value-matches — so no valid program is miscompiled.
/// The invalid `reversed(xs).index(v)` form (a list method on a CPython iterator)
/// is over-accepted; that is deliberate and not exercised as a value diff here.
#[test]
fn reversed_model_is_list() {
    // reversed([-1, 2, -3]) == [-3, 2, -1]; fold base-10 with +5 bias so the
    // order is observable: ((0*10 + 2)*10 + 7)*10 + 4 = 274.
    let src = "def go() -> int:\n    xs: list[int] = [-1, 2, -3]\n    r: list[int] = reversed(xs)\n    acc: int = 0\n    for v in r:\n        acc = acc * 10 + (v + 5)\n    return acc\n";

    emit(src).unwrap_or_else(|e| panic!("reversed-model emit failed: {e}"));

    if !wasm_runtime_available() || !python3_available() {
        eprintln!("PMAT-1275: tooling absent — reversed-model execution skipped.");
        return;
    }
    // CPython here uses `for v in reversed(xs)` (valid) so the oracle is honest.
    let wasm = run_go_int(src, "reversed_model");
    let py = cpython_int(src, "reversed_model");
    assert_eq!(
        wasm, py,
        "reversed consumed as a list must value-match: wasm={wasm:?} cpython={py:?}"
    );
    assert_eq!(wasm, Some(274), "reversed([-1,2,-3]) folded == 274");
}
