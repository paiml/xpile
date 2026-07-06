//! PMAT-1306 — EXECUTED witness for native-WASM str-value `d.pop(k)` /
//! `d.pop(k, default)` / `d.setdefault(k, default)` over a `dict[K, str]` —
//! the two value-kinded forms PMAT-1305 shipped REFUSED ("str legs unwired").
//! Runs on the bump-heap dict runtime (`C-COMPILE-RUST-TO-WASM` +
//! `C-WASM-HEAP`).
//!
//! ## Why this exists
//!
//! PMAT-1305 stored a str value as its i32 base-pointer zero-extended into the
//! 8-byte value slot and wired the READ forms (`d[k]`, `d.get(k, default)`)
//! into `emit_str_expr`. `.pop` and `.setdefault` refused there — not because
//! the runtime lacked anything (the keyed `pop`/`set`/`get` helpers are
//! value-kind-agnostic slot movers) but because their STRING-position arms
//! didn't exist. This slice is pure ROUTING and CLASSIFIER PARITY:
//!
//! * `d.pop(k)` in a string position = the SAME `$__wasm_dict_pop_<k>` call
//!   (swap-last-into-hole + count--, in place, no write-back) wrapped
//!   `i32.wrap_i64`; the 2-arg form gates the MUTATING pop on membership and
//!   falls to the default STRING without mutating — exactly CPython.
//! * `d.setdefault(k, default)` = insert-if-absent through the shared
//!   `$__wasm_dict_set_<k>` (the miss path may GROW + relocate; the returned
//!   base is written back) with the default stored via the PMAT-1305 routing
//!   (`emit_str_expr` + `i64.extend_i32_u`), then a `get` + wrap read-back.
//! * ★ BOTH string classifiers (`binop_operand_is_string` emit-time,
//!   `expr_is_str_valued` gate-time) gained DictPop/DictSetDefault parity arms
//!   — without them `a.pop(1) == b.pop(2)` lands in the int lane and compares
//!   ADDRESSES (the PMAT-1305 silent-miscompile class).
//!
//! ## The pointer-identity trap, pinned honestly
//!
//! The static literal region DEDUPLICATES by content, so two dict literals
//! holding `"same"` store the SAME pointer — a literal-vs-literal compare
//! passes under pointer identity too and pins nothing. The content pins here
//! therefore put a HEAP-materialised string (a `Concat`) on one side: equal
//! content at a DIFFERENT address (eq pin), and smaller content at a LARGER
//! address (ordering pin) — each fails under address compare.
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

/// Every probe: a zero-arg `def <name>() -> int` (the export) built from REAL
/// Python — the same text is executed by live python3 for the expected value.
/// The corpus reuses a SMALL alphabet of distinct literal contents (the static
/// literal region is 512 bytes, deduplicated by content).
fn probes() -> Vec<(&'static str, String)> {
    vec![
        // ── pop: hit reads AND removes ───────────────────────────────────────
        (
            "pop_hit",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    s: str = d.pop(1)\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        (
            "pop_removes_membership",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    s: str = d.pop(1)\n    if 1 in d:\n        return 99\n    return len(d)\n"
                .to_string(),
        ),
        // ── pop with default: miss does NOT mutate, hit removes ─────────────
        (
            "pop_default_miss",
            "    d: dict[int, str] = {1: \"abc\"}\n    s: str = d.pop(9, \"zz\")\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        (
            "pop_default_hit",
            "    d: dict[int, str] = {1: \"abc\"}\n    s: str = d.pop(1, \"zz\")\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        // ── ★ CONTENT equality — heap Concat vs literal: equal bytes at a
        //     DIFFERENT address; pointer identity returns 0, content 1 ───────
        (
            "pop_content_eq",
            "    a: dict[int, str] = {1: \"same\"}\n    b: dict[int, str] = {2: \"w\"}\n    b[2] = \"sa\" + \"me\"\n    if a.pop(1) == b.pop(2):\n        return 1\n    return 0\n"
                .to_string(),
        ),
        (
            "setdefault_content_eq",
            "    a: dict[int, str] = {1: \"same\"}\n    b: dict[int, str] = {2: \"w\"}\n    b[2] = \"sa\" + \"me\"\n    if a.setdefault(1, \"w\") == b.setdefault(2, \"w\"):\n        return 1\n    return 0\n"
                .to_string(),
        ),
        // ── ★ CONTENT ordering — smaller content at a LARGER (heap) address:
        //     address order returns 8, content order 7 ─────────────────────────
        (
            "pop_content_cmp",
            "    d: dict[int, str] = {1: \"w\"}\n    d[1] = \"ab\" + \"c\"\n    e: dict[int, str] = {2: \"abd\"}\n    if d.pop(1) < e.pop(2):\n        return 7\n    return 8\n"
                .to_string(),
        ),
        // ── setdefault: hit keeps the existing value (no overwrite) ─────────
        (
            "setdefault_hit",
            "    d: dict[int, str] = {1: \"abc\"}\n    s: str = d.setdefault(1, \"zz\")\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        // ── setdefault: miss INSERTS the default and returns it ─────────────
        (
            "setdefault_miss_inserts",
            "    d: dict[int, str] = {1: \"abc\"}\n    s: str = d.setdefault(3, \"qq\")\n    if d[3] == \"qq\":\n        return len(s) * 10 + len(d)\n    return 99\n"
                .to_string(),
        ),
        // ── setdefault: a materialising (Concat) default on the miss path ───
        (
            "setdefault_concat_default",
            "    d: dict[int, str] = {1: \"ab\"}\n    s: str = d.setdefault(2, \"ab\" + \"c\")\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        // ── str-KEYED str-valued dict (the `_s` helper suffix lane) ─────────
        (
            "pop_str_key",
            "    d: dict[str, str] = {\"a\": \"xy\", \"b\": \"w\"}\n    s: str = d.pop(\"a\")\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        // ── composition: the read strings feed the existing str surface.
        //    A pop CANNOT sit inside a concat/f-string (multi-eval — refused,
        //    pinned below); bind-then-use composes, and the idempotent
        //    setdefault composes directly. ─────────────────────────────────────
        (
            "pop_bind_then_concat",
            "    d: dict[int, str] = {1: \"ab\"}\n    p: str = d.pop(1)\n    s: str = \"w\" + p\n    return len(s)\n"
                .to_string(),
        ),
        (
            "pop_len_direct",
            "    d: dict[int, str] = {1: \"abc\", 2: \"w\"}\n    return len(d.pop(1)) * 10 + len(d)\n"
                .to_string(),
        ),
        (
            "setdefault_concat_compose",
            "    d: dict[int, str] = {1: \"ab\"}\n    s: str = \"w\" + d.setdefault(2, \"zz\")\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        (
            "setdefault_fstring_compose",
            "    d: dict[int, str] = {1: \"ab\"}\n    s: str = f\"<{d.setdefault(1, 'zz')}>\"\n    return len(s) * 10 + len(d)\n"
                .to_string(),
        ),
        (
            "pop_ord_compose",
            "    d: dict[int, str] = {1: \"q\"}\n    s: str = d.pop(1)\n    return ord(s)\n"
                .to_string(),
        ),
        // ── statement position: removal / ensure-key are the point ──────────
        (
            "pop_stmt",
            "    d: dict[int, str] = {1: \"ab\", 2: \"w\"}\n    d.pop(1)\n    return len(d)\n"
                .to_string(),
        ),
        (
            "setdefault_stmt",
            "    d: dict[int, str] = {1: \"ab\"}\n    d.setdefault(2, \"w\")\n    d.setdefault(1, \"zz\")\n    return len(d) * 10 + len(d[1])\n"
                .to_string(),
        ),
        // ── ★ RELOCATING grow through setdefault's miss path (the PMAT-1303
        //     lesson: OUTRUN the 16-slot literal slack — 23 net-new keys into
        //     cap 17 forces a real 17→34 relocation mid-loop; the write-back
        //     threads the moved base). d[0] pins the no-overwrite hit too. ────
        (
            "setdefault_grow_relocates",
            "    d: dict[int, str] = {0: \"g\"}\n    for i in range(24):\n        d.setdefault(i, \"gx\")\n    return len(d) * 100 + len(d[0]) * 10 + len(d[23])\n"
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
fn dict_str_pop_setdefault_lower_and_reuse_helpers() {
    let wat = emit(&corpus_source()).expect("the pop/setdefault corpus must lower end-to-end");
    // Both forms ride the EXISTING keyed helpers — pop is the shared remover,
    // setdefault the shared has/set/get triple; the str value is wrapped back
    // from the i64 slot. Content compares ride the string helpers (the
    // classifier parity arms). NO bespoke str-value helper exists.
    for call in [
        "call $__wasm_dict_pop_i",
        "call $__wasm_dict_pop_s",
        "call $__wasm_dict_set_i",
        "call $__wasm_dict_get_i",
        "call $__wasm_dict_has_i",
        "call $__wasm_str_eq",
        "call $__wasm_str_cmp",
    ] {
        assert!(
            wat.contains(call),
            "str-value pop/setdefault must reuse {call}:\n{wat}"
        );
    }
    assert!(
        wat.contains("i32.wrap_i64"),
        "a str-value pop/setdefault read must wrap the slot back to i32:\n{wat}"
    );
    assert!(
        wat.contains("i64.extend_i32_u"),
        "the setdefault miss path must zero-extend the default's pointer:\n{wat}"
    );
    assert!(
        !wat.contains("$__wasm_dict_strval"),
        "no bespoke str-value helper may exist (routing only):\n{wat}"
    );
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// The value-kind mismatches refuse in BOTH directions — an int-valued dict's
/// pop/setdefault in a string position, and an int default handed to a
/// str-valued dict's 2-arg pop / setdefault.
#[test]
fn dict_str_pop_setdefault_refuse_kind_mismatches() {
    for (label, src, needle) in [
        (
            "int-valued pop in str position",
            "def f() -> int:\n    d: dict[int, int] = {1: 5}\n    s: str = d.pop(1)\n    return len(s)\n"
                .to_string(),
            "in a string position over an int-valued dict",
        ),
        (
            "int-valued setdefault in str position",
            "def f() -> int:\n    d: dict[int, int] = {1: 5}\n    s: str = d.setdefault(2, 9)\n    return len(s)\n"
                .to_string(),
            "in a string position over an int-valued dict",
        ),
        (
            "int default in a str-valued 2-arg pop",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    s: str = d.pop(9, 5)\n    return len(s)\n"
                .to_string(),
            "in a string position",
        ),
        (
            "int default in a str-valued setdefault",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    s: str = d.setdefault(2, 5)\n    return len(s)\n"
                .to_string(),
            "in a string position",
        ),
        // ★ MULTI-EVAL defense (the find this witness's own first run made):
        // concat re-evaluates each operand per pass, so a MUTATING pop inside
        // a concat / f-string would remove twice — trap (bare) or silently
        // fall to its default (2-arg). Refused; bind to a name first.
        (
            "pop inside a concat",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    s: str = \"w\" + d.pop(1)\n    return len(s)\n"
                .to_string(),
            "re-evaluates each operand",
        ),
        (
            "pop inside an f-string",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    s: str = f\"<{d.pop(1)}>\"\n    return len(s)\n"
                .to_string(),
            "re-evaluates each operand",
        ),
        // ★ LAZY-DEFAULT defense: the default of get/pop/setdefault lowers in
        // the miss branch only, but CPython evaluates the ARGUMENT eagerly —
        // a nested pop would skip its removal on a hit. Both lanes refuse.
        (
            "pop inside a 2-arg pop default (str lane)",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    e: dict[int, str] = {2: \"w\"}\n    s: str = d.pop(1, e.pop(2))\n    return len(s)\n"
                .to_string(),
            "inside the default",
        ),
        (
            "pop inside a get default (int lane)",
            "def f() -> int:\n    d: dict[int, int] = {1: 5}\n    e: dict[int, int] = {2: 7}\n    return d.get(1, e.pop(2))\n"
                .to_string(),
            "inside the default",
        ),
        (
            "pop inside a setdefault default (int lane)",
            "def f() -> int:\n    d: dict[int, int] = {1: 5}\n    e: dict[int, int] = {2: 7}\n    return d.setdefault(3, e.pop(2))\n"
                .to_string(),
            "inside the default",
        ),
    ] {
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains(needle),
            "{label} refusal should say {needle:?}, got: {err}"
        );
    }
}

/// The INT-lane guards inside `emit_dict_pop` / `emit_dict_set_default` stay
/// as defense in depth: a str-valued dict's pop/setdefault forced into an
/// integer position refuses rather than handing a pointer to arithmetic.
#[test]
fn dict_str_pop_setdefault_refuse_int_position() {
    for (label, src) in [
        (
            "pop in arithmetic",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    n: int = 0\n    n = n + d.pop(1)\n    return n\n"
                .to_string(),
        ),
        (
            "setdefault in arithmetic",
            "def f() -> int:\n    d: dict[int, str] = {1: \"ab\"}\n    n: int = 0\n    n = n + d.setdefault(2, \"w\")\n    return n\n"
                .to_string(),
        ),
    ] {
        assert!(
            emit(&src).is_err(),
            "{label} over a str-valued dict must refuse (never a pointer-as-int)"
        );
    }
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
    let dir = std::env::temp_dir().join(format!("xpile-wasm-dictstrpop-{}", std::process::id()));
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
fn dict_str_pop_setdefault_execute_and_match_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the pop/setdefault corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1306: skipping EXECUTED str-value pop/setdefault witness — \
             WABT (wat2wasm / wasm-interp) absent. The corpus lowered through \
             the FULL pipeline (PythonFrontend → emit_module) and reuses the \
             keyed pop/set/get + str eq/cmp helpers (asserted in \
             `dict_str_pop_setdefault_lower_and_reuse_helpers`); a box with \
             WABT also runs every export and value-matches live python3 on the \
             identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1306: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1306: running EXECUTED str-value pop/setdefault witness via WABT");
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
        "no pop/setdefault probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1306: EXECUTED str-value pop/setdefault witness PASSED — {} probes \
         (pop hit/removes/2-arg both branches, heap-vs-literal CONTENT eq + \
         ordering [the pointer-identity pins], setdefault hit-keeps/\
         miss-inserts/Concat default, str-keyed lane, concat/len/f-string/ord \
         composition, statement position, and a RELOCATING 24-key setdefault \
         grow) all == live python3.",
        truth.len()
    );
}
