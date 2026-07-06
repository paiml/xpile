//! PMAT-1305 — EXECUTED witness for native-WASM STR-valued dicts
//! (`dict[int, str]` / `dict[str, str]`) — the FIRST non-int dict VALUE kind.
//! It runs on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` +
//! `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! Every dict slice through PMAT-1304 stored `i64` INT values only; the value
//! gate (`dict_value_kind`, né `dict_value_is_supported`) refused everything
//! else. This slice stores a STR value as its `i32` base-pointer ZERO-EXTENDED
//! into the same 8-byte value slot: strings are immutable in Python, so the
//! pointer copy IS reference semantics — `update`/`merge`/`del`/relocation move
//! the slot raw, soundly, with ZERO new runtime helpers. The work is all
//! ROUTING and GATING:
//!
//! * value STORES (`{k: "s"}` literal pairs, `d[k] = s`, merge explicit pairs)
//!   lower the value through `emit_str_expr` + `i64.extend_i32_u`;
//! * value READS (`d[k]`, `d.get(k, default)`) get STRING-position arms in
//!   `emit_str_expr` (`call $__wasm_dict_get_<k>` + `i32.wrap_i64`), so
//!   `len`/`ord`/`==`/`<`/concat/slice/f-string over a read all compose;
//! * the value-INTERPRETING forms REFUSE — `.values()` iteration/reductions
//!   (`min` would be lowest-ADDRESS) and MIXED value-kind `update`/merge (a
//!   raw slot copy the reader would mis-interpret). `.pop`/`.setdefault` were
//!   refused here too until PMAT-1306 wired their str legs (see
//!   `dict_str_pop_setdefault_witness.rs`), and dict `==` until PMAT-1307
//!   wired the content-comparing `$__wasm_dict_eq_sv_<k>` twin (see
//!   `dict_str_eq_witness.rs`).
//!
//! ## The silent-miscompile class this witness pins shut
//!
//! An i32 str base-pointer is INDISTINGUISHABLE from an int in the value slot.
//! Any path that reads the slot in the INT lane "works" — it just computes on
//! addresses. The high-value pins are therefore the CONTENT ones:
//!   * `a[1] == b[2]` across two dicts — must be `$__wasm_str_eq` content
//!     compare (two DISTINCT allocations of equal bytes), never pointer eq.
//!     This exercises the `expr_is_str_valued` / `binop_operand_is_string`
//!     DictGet parity arms; without them the compare lands in the int lane.
//!   * `d[a] < d[b]` ordering — `$__wasm_str_cmp`, never address order.
//!   * a RELOCATING `update` (>16 net-new keys outruns the literal slack —
//!     the PMAT-1303 lesson) then value reads through the final pointer.
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` →
//! `emit_module` → `wat2wasm` → `wasm-interp`), value-matched against LIVE
//! python3 executing the IDENTICAL source. Gated on `wasm_runtime_available()`
//! — a clean skip (still asserting emit + refusals) without WABT.

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

/// 24 str-valued pairs whose value length varies with `k % 3` — the grow
/// probe's source (24 net-new keys OUTRUN the 16-slot literal slack, forcing a
/// real relocation whose moved value slots are then length-read). Only THREE
/// distinct contents (`"g"`/`"gx"`/`"gxx"`) so the static literal region
/// (512 bytes, deduplicated by content) is not exhausted — the 24 value SLOTS
/// still hold 24 stored pointers that all relocate.
fn grow_pairs() -> String {
    let entries: Vec<String> = (1..=24)
        .map(|k| format!("{k}: \"g{}\"", "x".repeat((k % 3) as usize)))
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
fn probes() -> Vec<(&'static str, String)> {
    // The corpus deliberately REUSES a small alphabet of distinct literal
    // contents — the static literal region is 512 bytes and deduplicated by
    // content, so distinct contents (not uses) are the scarce resource.
    let ab = "    a: dict[int, str] = {1: \"x\", 2: \"yy\"}\n    b: dict[int, str] = {2: \"zzz\", 3: \"w\"}\n";
    vec![
        // ── literal store + subscript read + subscript write ────────────────
        (
            "lit_read",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    d[3] = \"qqqq\"\n    return len(d[1]) + len(d[3])\n"
                .to_string(),
        ),
        (
            "store_over",
            "    d: dict[int, str] = {1: \"abc\"}\n    d[1] = \"x\"\n    return len(d[1]) * 10 + len(d)\n"
                .to_string(),
        ),
        // ── CONTENT equality — the pointer-identity trap pinned shut ────────
        (
            "eq_lit",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    if d[1] == \"abc\":\n        return 10\n    return 20\n"
                .to_string(),
        ),
        (
            "eq_cross",
            "    a: dict[int, str] = {1: \"same\"}\n    b: dict[int, str] = {2: \"same\"}\n    if a[1] == b[2]:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "neq_cross",
            "    a: dict[int, str] = {1: \"aa\"}\n    b: dict[int, str] = {2: \"ab\"}\n    if a[1] != b[2]:\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── CONTENT ordering — never address order ──────────────────────────
        (
            "ord_lt",
            "    d: dict[str, str] = {\"k1\": \"hello\"}\n    d[\"k2\"] = \"worldly\"\n    if d[\"k1\"] < d[\"k2\"]:\n        return len(d[\"k1\"]) * 10 + len(d[\"k2\"])\n    return 0\n"
                .to_string(),
        ),
        // ── the read composes with the whole string surface ─────────────────
        (
            "concat_len",
            "    d: dict[str, str] = {\"k1\": \"hello\", \"k2\": \"worldly\"}\n    s: str = d[\"k1\"] + \" \" + d[\"k2\"]\n    return len(s)\n"
                .to_string(),
        ),
        (
            "slice_val",
            "    d: dict[int, str] = {1: \"abcdef\"}\n    s: str = d[1][1:3]\n    return len(s)\n"
                .to_string(),
        ),
        (
            "fstr_val",
            "    d: dict[int, str] = {1: \"bob\"}\n    s: str = f\"hi {d[1]}!\"\n    return len(s)\n"
                .to_string(),
        ),
        (
            "aug_concat",
            "    d: dict[int, str] = {1: \"ab\"}\n    d[1] += \"cde\"\n    return len(d[1])\n"
                .to_string(),
        ),
        // ── the TOTAL read: get(k, default), both branches ───────────────────
        (
            "get_hit_miss",
            "    d: dict[int, str] = {1: \"abc\"}\n    return len(d.get(1, \"zz\")) * 10 + len(d.get(9, \"zz\"))\n"
                .to_string(),
        ),
        // ── removal + membership stay key-based (value slots move raw) ──────
        (
            "del_read",
            "    d: dict[int, str] = {1: \"x\", 2: \"yy\"}\n    del d[1]\n    return len(d) * 10 + len(d[2])\n"
                .to_string(),
        ),
        (
            "contains_gate",
            "    d: dict[int, str] = {1: \"abc\"}\n    if 1 in d:\n        return len(d[1])\n    return 0\n"
                .to_string(),
        ),
        // ── dict-to-dict: update / PEP 584 in-place / merge, values raw ─────
        (
            "upd",
            format!("{ab}    a.update(b)\n    return len(a) * 10 + len(a[2])\n"),
        ),
        (
            "ior",
            format!("{ab}    a |= b\n    return len(a) * 10 + len(a[2])\n"),
        ),
        (
            "merge_mixed",
            format!(
                "{ab}    m: dict[int, str] = {{**a, **b, 4: \"qqqq\"}}\n    return len(m) * 100 + len(m[2]) * 10 + len(m[4])\n"
            ),
        ),
        (
            "merge_copy_indep",
            "    a: dict[int, str] = {1: \"orig\"}\n    m: dict[int, str] = {**a}\n    m[1] = \"zz\"\n    return len(a[1]) * 10 + len(m[1])\n"
                .to_string(),
        ),
        // ── a RELOCATING update: 24 net-new keys outrun the 16-slot slack ───
        (
            "grow_reloc",
            format!(
                "    a: dict[int, str] = {{1: \"base\", 2: \"keep\"}}\n    b: dict[int, str] = {}\n    a.update(b)\n    return len(a) * 1000 + len(a[2]) * 100 + len(a[24]) * 10 + len(a[1])\n",
                grow_pairs()
            ),
        ),
        // ── KEY-based forms keep working over a str-VALUED dict ─────────────
        (
            "key_reduce",
            "    d: dict[int, str] = {3: \"x\", 7: \"yy\"}\n    return sum(d) + max(d)\n"
                .to_string(),
        ),
        (
            "sorted_keys",
            "    d: dict[int, str] = {9: \"x\", 4: \"yy\"}\n    xs: list[int] = sorted(d)\n    return xs[0]\n"
                .to_string(),
        ),
        (
            "key_iter_fold",
            "    d: dict[int, str] = {3: \"ab\", 7: \"cde\"}\n    n: int = 0\n    for k in d:\n        n = n + k\n    return n\n"
                .to_string(),
        ),
    ]
}

fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    src
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn dict_str_values_lower_and_reuse_helpers() {
    let wat = emit(&corpus_source()).expect("the str-value corpus must lower end-to-end");
    // A str value rides the EXISTING keyed helpers — the store is the shared
    // update-or-insert, the read is the shared get + i32.wrap_i64; content
    // compares over reads ride the string helpers. NO bespoke str-value helper.
    for call in [
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_get_i",
        "call $__wasm_dict_set_s",
        "call $__wasm_dict_get_s",
        "call $__wasm_dict_update_i",
        "call $__wasm_str_eq",
        "call $__wasm_str_cmp",
    ] {
        assert!(
            wat.contains(call),
            "str-value dict must reuse {call}:\n{wat}"
        );
    }
    // The store zero-extends the pointer into the slot; the read wraps it back.
    assert!(
        wat.contains("i64.extend_i32_u"),
        "a str-value store must zero-extend the i32 base-pointer:\n{wat}"
    );
    assert!(
        wat.contains("i32.wrap_i64"),
        "a str-value read must wrap the slot back to an i32 pointer:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_dict_strval"),
        "no bespoke str-value helper may exist (routing only):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The value-INTERPRETING forms refuse for a str-valued dict — each would
/// compute on POINTERS (identity/address order), not string content.
#[test]
fn dict_str_values_refuse_value_interpreting_forms() {
    let d = "    d: dict[int, str] = {1: \"x\", 2: \"yy\"}\n";
    for (label, src, needle) in [
        // dict `==`/`!=` over a str-valued dict LOWERS since PMAT-1307 (the
        // content-comparing `$__wasm_dict_eq_sv_<k>` twin — see
        // `dict_str_eq_witness.rs`); the value-ITERATING forms below remain
        // refused.
        (
            "for v in d.values()",
            format!(
                "def f() -> int:\n{d}    n: int = 0\n    for v in d.values():\n        n = n + 1\n    return n\n"
            ),
            "str-value iteration is not in the WASM subset yet",
        ),
        (
            "for k, v in d.items()",
            format!(
                "def f() -> int:\n{d}    n: int = 0\n    for k, v in d.items():\n        n = n + k\n    return n\n"
            ),
            "str-value iteration is not in the WASM subset yet",
        ),
        // pop/setdefault over a str-valued dict LOWER since PMAT-1306 (their
        // str legs are wired — see `dict_str_pop_setdefault_witness.rs`); the
        // int-lane guards inside `emit_dict_pop`/`emit_dict_set_default`
        // remain as defense in depth for an INT-position use, pinned there.
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!(
                "{label} must be refused for a str-valued dict but lowered:\n{wat}"
            ),
        };
        assert!(
            err.contains(needle),
            "{label} refusal should say {needle:?}, got: {err}"
        );
    }
}

/// A raw slot copy between dicts of DIFFERENT value kinds would store slots the
/// reader mis-interprets — both merge directions refuse.
#[test]
fn dict_str_values_refuse_mixed_value_kinds() {
    let base = "    a: dict[int, str] = {1: \"x\"}\n    b: dict[int, int] = {2: 5}\n";
    let upd = format!("def f() -> int:\n{base}    a.update(b)\n    return len(a)\n");
    let err = emit(&upd).expect_err("mixed-value-kind update must be refused");
    assert!(
        err.contains("VALUE kinds differ"),
        "update refusal should name the value-kind mismatch, got: {err}"
    );
    let mrg =
        format!("def f() -> int:\n{base}    m: dict[int, str] = {{**a, **b}}\n    return len(m)\n");
    let err = emit(&mrg).expect_err("mixed-value-kind merge must be refused");
    assert!(
        err.contains("VALUE kinds that disagree"),
        "merge refusal should name the value-kind mismatch, got: {err}"
    );
}

/// The value-kind gate still refuses the UNMODELLED value kinds. Since this
/// witness landed, int→{int, str} widened, then bool (PMAT-1320) and float
/// (PMAT-1322) joined the int-slot lane — so the pin is now on a NESTED value
/// (`dict[int, dict[int, int]]`), still outside the WASM dict value subset.
#[test]
fn dict_value_gate_still_refuses_nested() {
    let err = emit(
        "def f() -> int:\n    d: dict[int, dict[int, int]] = {}\n    return len(d)\n",
    )
    .expect_err("a nested-dict-valued dict must still be refused");
    assert!(
        err.contains("dict value type"),
        "nested-value refusal should come from the value gate, got: {err}"
    );
}

/// An int-valued dict read in a STRING position refuses (the mirror of the
/// str-valued-dict-in-int-position guard) — never a silent pointer fabrication.
#[test]
fn dict_int_values_refuse_string_position() {
    let err = emit(
        "def f() -> int:\n    d: dict[int, int] = {1: 5}\n    s: str = \"a\" + d[1]\n    return len(s)\n",
    )
    .expect_err("an int value in a string position must be refused");
    assert!(
        err.contains("int-valued dict") || err.contains("Concat"),
        "refusal should name the int-value/str-position mismatch, got: {err}"
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictstrval-{}", std::process::id()));
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
fn dict_str_values_execute_in_wasm_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the str-value corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1305: skipping EXECUTED dict-str-values witness — WABT \
             (wat2wasm / wasm-interp) absent. The corpus lowered through the \
             FULL pipeline (PythonFrontend → emit_module) and reuses the keyed \
             get/set/update + str eq/cmp helpers (asserted in \
             `dict_str_values_lower_and_reuse_helpers`); a box with WABT also \
             runs every export and value-matches live python3 on the identical \
             source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1305: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1305: running EXECUTED dict-str-values witness via WABT");
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
        "no dict-str-values probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1305: EXECUTED dict-str-values witness PASSED — {} probes \
         (literal/store/read, cross-dict CONTENT eq + ordering, concat/slice/\
         f-string/aug-concat composition, get-with-default both branches, \
         del/contains, update + PEP 584 |= + merge with a value-kind parity \
         gate, copy independence, a RELOCATING 24-key grow, and the key-based \
         reductions/iteration surviving over str values) all == live python3.",
        truth.len()
    );
}
