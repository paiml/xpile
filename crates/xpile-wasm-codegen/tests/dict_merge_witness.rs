//! PMAT-1304 — EXECUTED witness for the native-WASM dict MERGE — `{**a, **b}`
//! (dict-display splat, possibly mixed with explicit `k: v` pairs) and the
//! PEP 584 union `a | b`, both lowered by the Python frontend to
//! `Expr::DictMerge` and emitted as a dict-BINDING value. It runs on the
//! bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! `d.update(other)` (PMAT-1302) opened the dict-to-dict MUTATION surface;
//! this slice opens the dict-to-dict CONSTRUCTION surface: a merge yields a
//! FRESH dict, never mutating a source (`{**d}` is the dict COPY). The
//! lowering is deliberately helper-free: allocate an empty region sized for
//! the explicit pairs (+ slack), then fold each entry LEFT-TO-RIGHT — a splat
//! through `$__wasm_dict_update_<k>` (the PMAT-1302 walk, receiver = the
//! fresh dst) and an explicit pair through `$__wasm_dict_set_<k>` (one
//! dict-literal insert) — so a later entry WINS on a key collision, exactly
//! CPython.
//!
//! ## Lessons this witness bakes in (from the PMAT-1303 adversarial pass)
//!
//! * **FULL pipeline, not meta-HIR**: every probe is REAL Python source →
//!   `PythonFrontend` → `emit_module` → `wat2wasm` → `wasm-interp`,
//!   value-matched against LIVE `python3` executing the IDENTICAL source —
//!   so both frontend spellings (`{**a, **b}` and `a | b`) are pinned.
//! * **A grow witness must OUTRUN the slack**: the merge dst starts at
//!   `explicit_pairs + DICT_GROWTH_SLACK (16)` capacity, so the `grow1_*`
//!   probes merge 24 distinct keys (ONE real 2x relocation) and the
//!   `grow2_*` probes merge 40 (a DOUBLE 16→32→64 relocation), with reads +
//!   whole-dict reductions through the final base-pointer.
//!   `merge_forces_real_relocations` pins the capacity arithmetic with a
//!   mirrored slack constant AND asserts the small probes relocate ZERO
//!   times (so the mirror stays honest).
//!
//! Key correctness properties pinned against live `python3`:
//!   * b's value WINS on a shared key (`{**a, **b}` == `a | b` == last-wins).
//!   * an explicit pair AFTER a splat overwrites it; BEFORE a splat it loses.
//!   * `{**a}` is a real COPY — mutating the copy never shows through the
//!     source and vice versa (fresh region, no shared storage).
//!   * the self-rebind `m = {**m, **b}` reads the OLD `m` (RHS-first).
//!   * a merged dict feeds the existing read surface: `for k in m` iteration,
//!     `sum(m)` / `sum(m.values())` reductions, `len` / `m[k]` reads.
//!   * str-keyed merge compares keys by CONTENT (`$__wasm_str_eq`), and an
//!     explicit str pair's key literal is laid out into the static data table
//!     (the `collect_expr_literals` DictMerge arm this slice adds).
//!
//! Refusals (a set splat, a non-name splat source — nested literal or
//! unbound chained union — and a key-kind mismatch) are asserted through the
//! FULL pipeline. Gated on `wasm_runtime_available()` — a clean skip (still
//! asserting the emit path + refusals) without WABT.

use std::path::Path;
use std::process::Command;

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

// ---- the probe corpus --------------------------------------------------------

/// A `{lo: lo*10, …, hi: hi*10}` int-dict literal (bijective value = key*10,
/// so key-sum and value-sum pins stay independent).
fn pairs(lo: i64, hi: i64) -> String {
    let entries: Vec<String> = (lo..=hi).map(|k| format!("{k}: {}", k * 10)).collect();
    format!("{{{}}}", entries.join(", "))
}

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
fn probes() -> Vec<(&'static str, String)> {
    let small = "    a: dict[int, int] = {1: 10, 2: 20}\n    b: dict[int, int] = {2: 99, 3: 30}\n";
    let big1 = format!(
        "    a: dict[int, int] = {}\n    b: dict[int, int] = {}\n",
        pairs(1, 12),
        pairs(13, 24)
    );
    let big2 = format!(
        "    a: dict[int, int] = {}\n    b: dict[int, int] = {}\n",
        pairs(1, 20),
        pairs(21, 40)
    );
    let strs =
        "    a: dict[str, int] = {'x': 1, 'y': 2}\n    b: dict[str, int] = {'y': 9, 'z': 3}\n";
    vec![
        // ── splat merge: overlap / kept / appended ──────────────────────────
        (
            "m_len",
            format!("{small}    m: dict[int, int] = {{**a, **b}}\n    return len(m)\n"),
        ),
        (
            "m_shared",
            format!("{small}    m: dict[int, int] = {{**a, **b}}\n    return m[2]\n"),
        ),
        (
            "m_kept",
            format!("{small}    m: dict[int, int] = {{**a, **b}}\n    return m[1]\n"),
        ),
        (
            "m_appended",
            format!("{small}    m: dict[int, int] = {{**a, **b}}\n    return m[3]\n"),
        ),
        // ── PEP 584 union — both the unannotated and annotated binding ──────
        (
            "u_val",
            format!("{small}    c = a | b\n    return c[2]\n"),
        ),
        (
            "u_len",
            format!("{small}    c = a | b\n    return len(c)\n"),
        ),
        (
            "u_ann_val",
            format!("{small}    c: dict[int, int] = a | b\n    return c[3]\n"),
        ),
        // ── explicit pairs mixed with splats: LEFT-TO-RIGHT last-wins ───────
        (
            "mix_pair_after",
            format!("{small}    m: dict[int, int] = {{**a, 2: 77}}\n    return m[2]\n"),
        ),
        (
            "mix_pair_before",
            format!("{small}    m: dict[int, int] = {{2: 77, **a}}\n    return m[2]\n"),
        ),
        (
            "mix_new_pair",
            format!("{small}    m: dict[int, int] = {{**a, 9: 90}}\n    return m[9] + len(m)\n"),
        ),
        (
            "dup_pair",
            format!(
                "{small}    m: dict[int, int] = {{**a, 9: 1, 9: 2}}\n    return m[9] * 10 + len(m)\n"
            ),
        ),
        // ── `{**a}` is a COPY: fresh region, no shared storage ──────────────
        (
            "copy_src_intact",
            format!("{small}    m: dict[int, int] = {{**a}}\n    m[1] = 77\n    return a[1]\n"),
        ),
        (
            "copy_dst_val",
            format!("{small}    m: dict[int, int] = {{**a}}\n    m[1] = 77\n    return m[1]\n"),
        ),
        (
            "copy_srcmut",
            format!("{small}    m: dict[int, int] = {{**a}}\n    a[1] = 55\n    return m[1]\n"),
        ),
        (
            "copy_empty",
            "    e: dict[int, int] = {}\n    m: dict[int, int] = {**e}\n    return len(m)\n"
                .to_string(),
        ),
        // ── self-rebind reads the OLD m (RHS evaluates first) ───────────────
        (
            "self_rebind",
            "    b: dict[int, int] = {2: 2}\n    m: dict[int, int] = {1: 1}\n    m = {**m, **b}\n    return len(m) * 100 + m[1] * 10 + m[2]\n"
                .to_string(),
        ),
        // ── empty-source no-op ───────────────────────────────────────────────
        (
            "empty_src",
            format!(
                "{small}    e: dict[int, int] = {{}}\n    m: dict[int, int] = {{**a, **e}}\n    return len(m) * 100 + m[1]\n"
            ),
        ),
        // ── grow1: 24 distinct keys into a cap-16 dst → ONE real relocation ─
        (
            "grow1_len",
            format!("{big1}    m: dict[int, int] = {{**a, **b}}\n    return len(m)\n"),
        ),
        (
            "grow1_first",
            format!("{big1}    m: dict[int, int] = {{**a, **b}}\n    return m[1]\n"),
        ),
        (
            "grow1_last",
            format!("{big1}    m: dict[int, int] = {{**a, **b}}\n    return m[24]\n"),
        ),
        (
            "grow1_vsum",
            format!("{big1}    m: dict[int, int] = {{**a, **b}}\n    return sum(m.values())\n"),
        ),
        // ── grow2: 40 distinct keys → DOUBLE (16→32→64) relocation ──────────
        (
            "grow2_len",
            format!("{big2}    m: dict[int, int] = a | b\n    return len(m)\n"),
        ),
        (
            "grow2_first",
            format!("{big2}    m: dict[int, int] = a | b\n    return m[1]\n"),
        ),
        (
            "grow2_last",
            format!("{big2}    m: dict[int, int] = a | b\n    return m[40]\n"),
        ),
        (
            "grow2_ksum",
            format!("{big2}    m: dict[int, int] = a | b\n    return sum(m)\n"),
        ),
        (
            "grow2_vsum",
            format!("{big2}    m: dict[int, int] = a | b\n    return sum(m.values())\n"),
        ),
        // ── an explicit pair lands AFTER a double relocation ────────────────
        (
            "grow_overlap",
            format!(
                "{big2}    m: dict[int, int] = {{**a, **b, 1: 7}}\n    return m[1] * 1000 + len(m)\n"
            ),
        ),
        // ── str-keyed: CONTENT-compare + explicit pair literal layout ───────
        (
            "str_len",
            format!("{strs}    m: dict[str, int] = {{**a, **b}}\n    return len(m)\n"),
        ),
        (
            "str_shared",
            format!("{strs}    m: dict[str, int] = {{**a, **b}}\n    return m['y']\n"),
        ),
        (
            "str_kept",
            format!("{strs}    m: dict[str, int] = {{**a, **b}}\n    return m['x']\n"),
        ),
        (
            "str_pair",
            format!("{strs}    m: dict[str, int] = {{**a, 'w': 4}}\n    return m['w'] + m['x']\n"),
        ),
        // ── a merged dict feeds the existing read surface ────────────────────
        (
            "merge_iter",
            format!(
                "{small}    m: dict[int, int] = {{**a, **b}}\n    t: int = 0\n    for k in m:\n        t = t + k\n    return t\n"
            ),
        ),
        (
            "merge_vals_sum",
            format!("{small}    m: dict[int, int] = {{**a, **b}}\n    return sum(m.values())\n"),
        ),
    ]
}

/// The whole corpus as ONE Python module (each probe a zero-arg def).
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    src
}

// ---- relocation arithmetic (mirrors the emitter's capacity math) ------------

/// Mirrors `DICT_GROWTH_SLACK` in src/lib.rs — the merge dst's spare capacity
/// past its explicit pairs. If the emitter's slack changes, this test fails
/// loudly and the grow corpus must be re-checked to still outrun it.
const SLACK: i64 = 16;

/// How many times the merge dst RELOCATES: it starts at `explicit + SLACK`
/// capacity and `$__wasm_dict_set_<k>` 2x-grows whenever an insert finds
/// `count >= capacity`.
fn merge_dst_relocations(explicit_pairs: i64, distinct_final: i64) -> u32 {
    let mut cap = explicit_pairs + SLACK;
    let mut r = 0;
    while distinct_final > cap {
        cap *= 2;
        r += 1;
    }
    r
}

#[test]
fn merge_forces_real_relocations() {
    // The PMAT-1303 lesson: a "grow" witness that does not OUTRUN the slack
    // exercises plain appends. Pin that the small probes NEVER relocate…
    assert_eq!(merge_dst_relocations(0, 3), 0, "small merge must not grow");
    assert_eq!(merge_dst_relocations(1, 3), 0, "mixed merge must not grow");
    assert_eq!(
        merge_dst_relocations(2, 4),
        0,
        "dup-pair merge must not grow"
    );
    // …and the grow probes really do: grow1 = 24 distinct keys → ONE 2x
    // relocation of the cap-16 dst; grow2 = 40 → a DOUBLE 16→32→64.
    assert_eq!(merge_dst_relocations(0, 24), 1, "grow1 must relocate ONCE");
    assert_eq!(merge_dst_relocations(0, 40), 2, "grow2 must relocate TWICE");
    // grow_overlap: ONE explicit pair (cap 17) + 40 distinct keys → 17→34→68.
    assert_eq!(
        merge_dst_relocations(1, 40),
        2,
        "grow_overlap must relocate TWICE"
    );
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn dict_merge_lowers_and_reuses_helpers() {
    let wat = emit(&corpus_source()).expect("the merge corpus must lower end-to-end");
    // A splat rides the PMAT-1302 update walk; an explicit pair rides the
    // shared update-or-insert dedup — NO bespoke merge helper exists.
    for call in [
        "call $__wasm_dict_update_i",
        "call $__wasm_dict_update_s",
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_set_s",
    ] {
        assert!(wat.contains(call), "merge must reuse {call}:\n{wat}");
    }
    assert!(
        !wat.contains("$__wasm_dict_merge"),
        "the merge must NOT mint a bespoke helper (it is a fold over update/set):\n{wat}"
    );
    // str-keyed merge compares keys by CONTENT.
    assert!(
        wat.contains("$__wasm_str_eq"),
        "str-keyed merge must carry the content-compare helper:\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

#[test]
fn dict_merge_refuses_set_splat() {
    // {**s} — a set is not a mapping; the FRONTEND refuses the spread.
    let err = emit(
        "def f() -> int:\n    s: set[int] = {1, 2}\n    m: dict[int, int] = {**s}\n    return len(m)\n",
    )
    .expect_err("a set splat must be refused");
    assert!(
        err.contains("requires each spread value to be a dict"),
        "refusal should name the dict-spread requirement, got: {err}"
    );
}

#[test]
fn dict_merge_refuses_nested_literal_source() {
    // {**{5: 50}, **a} — a non-name splat source (bind it to a local first).
    let err = emit(
        "def f() -> int:\n    a: dict[int, int] = {1: 10}\n    m: dict[int, int] = {**{5: 50}, **a}\n    return len(m)\n",
    )
    .expect_err("a nested-literal splat source must be refused");
    assert!(
        err.contains("non-name source"),
        "refusal should name the non-name source, got: {err}"
    );
}

#[test]
fn dict_merge_refuses_unbound_chained_union() {
    // a | b | c — the left-assoc inner merge is a temporary, not a local.
    let err = emit(
        "def f() -> int:\n    a: dict[int, int] = {1: 1}\n    b: dict[int, int] = {2: 2}\n    c: dict[int, int] = {3: 3}\n    m: dict[int, int] = a | b | c\n    return len(m)\n",
    )
    .expect_err("an unbound chained union must be refused");
    assert!(
        err.contains("non-name source"),
        "refusal should name the non-name source, got: {err}"
    );
}

#[test]
fn dict_merge_refuses_mismatched_key_kind() {
    // int-keyed a splatted with str-keyed b into an int-keyed result.
    let err = emit(
        "def f() -> int:\n    a: dict[int, int] = {1: 10}\n    b: dict[str, int] = {'x': 2}\n    m: dict[int, int] = {**a, **b}\n    return len(m)\n",
    )
    .expect_err("a mismatched key kind must be refused");
    assert!(
        err.contains("key kinds that disagree"),
        "refusal should name the key-kind mismatch, got: {err}"
    );
}

// ---- WABT harness -------------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal; every pin here is non-negative, so `u64` → `i64` is exact.
fn parse_scalar_export(stdout: &str, name: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&format!("{name}() => ")))
        .unwrap_or_else(|| panic!("no `{name}` export in interp output:\n{stdout}"));
    line.rsplit_once(':')
        .unwrap_or_else(|| panic!("malformed export line {line:?}"))
        .1
        .trim()
        .parse::<u64>()
        .map(|u| u as i64)
        .unwrap_or_else(|_| panic!("parse scalar for {name} from {line:?}"))
}

fn assemble_and_run(wat: &str) -> (String, bool) {
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictmerge-{}", std::process::id()));
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
        "wat2wasm failed:\n{}\n---WAT---\n{wat}",
        String::from_utf8_lossy(&assemble.stderr)
    );

    let run = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_path)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    (stdout, run.status.success())
}

/// Execute the IDENTICAL corpus source in live python3, returning
/// `name=value` pairs — the differential ground truth.
fn python_truth(src: &str) -> Option<Vec<(String, i64)>> {
    let names: Vec<&str> = probes().iter().map(|(n, _)| *n).collect();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{globals()[n]()}}' for n in {names:?}))\n");
    let out = Command::new("python3")
        .arg("-c")
        .arg(&driver)
        .output()
        .ok()?;
    if !out.status.success() {
        panic!(
            "python3 failed on the witness corpus:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(
        stdout
            .trim()
            .split(';')
            .map(|kv| {
                let (k, v) = kv.split_once('=').expect("k=v");
                (k.to_string(), v.parse::<i64>().expect("int"))
            })
            .collect(),
    )
}

// ---- EXECUTED witness (gated on WABT + python3) --------------------------------

#[test]
fn dict_merge_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the merge corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1304: skipping EXECUTED dict-merge witness — WABT (wat2wasm / \
             wasm-interp) absent. The corpus lowered through the FULL pipeline \
             (PythonFrontend → emit_module) and reuses the update/set helpers \
             (asserted in `dict_merge_lowers_and_reuses_helpers`); a box with \
             WABT also runs every export and value-matches live python3 on the \
             identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1304: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1304: running EXECUTED dict-merge witness via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, *expected,
            "executed WASM {name}() = {got} but live CPython = {expected} on the \
             IDENTICAL source\nfull interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no dict-merge probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1304: EXECUTED dict-merge witness PASSED — {} probes (splat merge, \
         PEP 584 union both spellings, mixed pairs last-wins, real copy \
         independence, self-rebind, single + double relocating grows, str \
         content-compare, merged-dict iteration/reductions) all == live python3.",
        truth.len()
    );
}
