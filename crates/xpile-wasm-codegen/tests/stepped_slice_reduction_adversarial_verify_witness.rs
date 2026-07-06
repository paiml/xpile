//! PMAT-1331 — ADVERSARIAL-VERIFY differential witness over the MOST RECENT
//! WASM wave: the STEPPED string slice (`s[i:j:k]`, PMAT-1327), the BOOL-valued
//! dict `.values()` reductions (`sum`/`min`/`max`, PMAT-1328/1329), and `len()`
//! of a dict VIEW (`len(d.keys()/values()/items())`, PMAT-1330). Under
//! `C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`.
//!
//! This is a SKEPTIC pass, not a feature. The three slices it re-checks are each
//! the sort of thing that hides an off-by-one or a wrong-int-width: PMAT-1327
//! reimplements CPython's `PySlice_AdjustIndices` per step sign with a two-pass
//! char-exact copy, and PMAT-1328 shipped as a FIX of an invalid-WAT emit (a
//! bool `min`/`max` extremum that was left as an `i64` where an `i32` bool was
//! due). A regression in any of them would surface as a run-time divergence from
//! CPython or an emit-time `wat2wasm` failure. The probes are built to trip
//! exactly those:
//!
//!   * STEPPED SLICE — reverse (`s[::-1]`), positive strides with and without
//!     bounds (`s[::2]`, `s[1::2]`, `s[1:5:2]`, `s[1:9:3]`), negative strides
//!     WITHOUT bounds (`s[::-2]`, `s[::-3]`, huge `s[::-100]`), empty results
//!     (`s[2:2:1]`, `s[7:2:1]`), out-of-range clamps (`s[0:100:2]`,
//!     `s[-100:100:1]`), a stride wider than the string (`s[::100]` → first char
//!     only), and MULTIBYTE UTF-8 (`"héllo wörld"`, `"αβγδεζ"`) where a byte-level
//!     stride would corrupt a code point. Every slice result is fingerprinted by
//!     a rolling hash over its CODE POINTS, so a divergence in LENGTH *or*
//!     ORDER/CONTENT (a reverse that didn't, a stride off by one) shows as one
//!     integer mismatch.
//!   * BOOL REDUCTIONS — `sum(d.values())` (counts the Trues), `min`/`max`
//!     (the i64 extremum WRAPPED back to a proper i32 bool — the PMAT-1328 fix)
//!     over all-True / all-False / mixed / single dicts, str-keyed variants, and
//!     a RELOCATING 20-key grow read back after the move.
//!   * VIEW LEN — `len(d.keys())` / `len(d.values())` / `len(d.items())` equal
//!     `len(d)` across sizes, value kinds, and after `del`/insert mutation.
//!
//! ## The i64-overflow FINGERPRINT DISCIPLINE (a re-learned lesson)
//!
//! The fingerprint MODS by a prime EVERY ITERATION, not once at the end. A
//! naive `acc = acc*131 + ord(c)` accumulated over a 10+ char result overflows
//! the WASM `i64` (`131^10 ≈ 1.6e21 ≫ 2^63`) and WRAPS, while CPython's bignum
//! does not — so a single terminal `% p` diverges and every full-length slice
//! reads as a FALSE mismatch. Per-iteration `% 1000000007` keeps `acc·131 <
//! 1.4e11 ≪ 2^63`, so WASM and CPython fold bit-identically. (Same trap the
//! PMAT-1318 sweep flagged: an i64-overflow fold is a TEST artifact, not a bug.)
//!
//! ## Result of the sweep at the PMAT-1330 head: NOTHING REFUTED.
//!
//! Every executable probe value-matches live python3, and the negative-step-
//! WITH-bounds forms (`s[5:0:-1]`, `s[4::-1]`, `s[:4:-1]`, …) each refuse
//! HONESTLY through the full pipeline (`negative-step slice with bounds` —
//! v0.2.0). The claim "STEPPED slice `s[i:j:k]` incl. `s[::-1]`" is thereby
//! SHARPENED: a negative stride executes only with BOTH bounds omitted; ANY
//! explicit start or stop with a negative step refuses. Positive strides take
//! bounds freely.
//!
//! Every probe is FULL-pipeline (REAL Python → `PythonFrontend` → `emit_module`
//! → `wat2wasm` → `wasm-interp`). Gated on `wasm_runtime_available()` — a clean
//! skip (still asserting emit + the refusals) without WABT.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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

fn emit(src: &str) -> Result<String, String> {
    let module = lower(src)?;
    emit_module(&module).map_err(|e| format!("wasm-codegen: {e}"))
}

// ---- probe construction ------------------------------------------------------

/// The per-iteration-MODDED fingerprint tail (see the module doc): folds a
/// rolling hash over the sliced string's code points WITHOUT overflowing i64,
/// then salts in the length so a shorter/longer result can never alias.
fn fp_tail() -> &'static str {
    "    acc: int = 0\n\
     \x20   i: int = 0\n\
     \x20   n: int = len(r)\n\
     \x20   while i < n:\n\
     \x20       acc = (acc * 131 + ord(r[i]) + 7) % 1000000007\n\
     \x20       i = i + 1\n\
     \x20   return acc + n * 1000000000\n"
}

/// A stepped-slice probe body: bind `s`, take `r = <slice>`, fingerprint `r`.
fn slice_body(s: &str, slice_expr: &str) -> String {
    format!("    s: str = {s}\n    r: str = {slice_expr}\n{}", fp_tail())
}

const ASCII: &str = "\"abcdefghij\"";

/// Executable stepped/plain slices (positive stride any bounds; negative stride
/// ONLY with both bounds omitted) + multibyte cases. `(name, body)`.
fn slice_probes() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    let ascii_cases = [
        // reverse + negative stride WITHOUT bounds
        ("rev", "s[::-1]"),
        ("rev2", "s[::-2]"),
        ("rev3", "s[::-3]"),
        ("rev_huge", "s[::-100]"),
        // positive stride, no bounds
        ("step1", "s[::1]"),
        ("step2", "s[::2]"),
        ("step3", "s[::3]"),
        ("from1_step2", "s[1::2]"),
        ("step_huge", "s[::100]"),
        // positive stride WITH bounds (incl. clamps + empties)
        ("bnd_1_5_2", "s[1:5:2]"),
        ("bnd_2_8_1", "s[2:8:1]"),
        ("bnd_1_9_3", "s[1:9:3]"),
        ("bnd_0_100_2", "s[0:100:2]"),
        ("bnd_neg100_100", "s[-100:100:1]"),
        ("empty_2_2", "s[2:2:1]"),
        ("empty_7_2", "s[7:2:1]"),
        ("bnd_neg8_neg3", "s[-8:-3:1]"),
        ("bnd_3_3_2", "s[3:3:2]"),
        // non-stepped baselines that share the slice path
        ("plain_2_5", "s[2:5]"),
        ("plain_pre3", "s[:3]"),
        ("plain_from3", "s[3:]"),
        ("plain_full", "s[:]"),
        ("plain_negtail", "s[-3:]"),
        ("plain_neghead", "s[:-3]"),
        ("plain_negspan", "s[-5:-2]"),
    ];
    for (name, expr) in ascii_cases {
        v.push((name.to_string(), slice_body(ASCII, expr)));
    }
    // MULTIBYTE: a byte-level stride would shred a code point.
    let mb_strings = [
        ("mba", "\"héllo wörld\""),
        ("mbb", "\"αβγδεζ\""),
        ("mbc", "\"naïve café\""),
    ];
    let mb_exprs = [("rev", "s[::-1]"), ("st2", "s[::2]"), ("sl", "s[1:4]")];
    for (sname, s) in mb_strings {
        for (ename, expr) in mb_exprs {
            v.push((format!("{sname}_{ename}"), slice_body(s, expr)));
        }
    }
    v
}

/// BOOL-dict reductions (PMAT-1328/1329) + dict-VIEW len (PMAT-1330). These
/// return ints directly (no fingerprint needed). `(name, body)`.
fn reduction_probes() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    let bd = |ent: &str, expr: &str, kty: &str| -> String {
        format!("    d: dict[{kty}, bool] = {{{ent}}}\n    return {expr}\n")
    };
    let dicts = [
        ("allt", "1: True, 2: True, 3: True"),
        ("allf", "1: False, 2: False, 3: False"),
        ("mix", "1: True, 2: False, 3: True, 4: False, 5: True"),
        ("one_t", "7: True"),
        ("one_f", "7: False"),
    ];
    for (name, ent) in dicts {
        v.push((format!("sum_{name}"), bd(ent, "sum(d.values())", "int")));
        v.push((
            format!("max_{name}"),
            bd(ent, "1 if max(d.values()) else 0", "int"),
        ));
        v.push((
            format!("min_{name}"),
            bd(ent, "1 if min(d.values()) else 0", "int"),
        ));
        v.push((format!("lenk_{name}"), bd(ent, "len(d.keys())", "int")));
        v.push((format!("lenv_{name}"), bd(ent, "len(d.values())", "int")));
        v.push((format!("leni_{name}"), bd(ent, "len(d.items())", "int")));
    }
    // str-keyed bool dicts
    v.push((
        "s_sum".to_string(),
        bd(
            "\"a\": True, \"b\": False, \"c\": True",
            "sum(d.values())",
            "str",
        ),
    ));
    v.push((
        "s_max".to_string(),
        bd(
            "\"a\": False, \"b\": True",
            "1 if max(d.values()) else 0",
            "str",
        ),
    ));
    v.push((
        "s_lenv".to_string(),
        bd(
            "\"a\": True, \"b\": False, \"cc\": True",
            "len(d.values())",
            "str",
        ),
    ));
    // mutation then reduce
    v.push((
        "mut_sum".to_string(),
        "    d: dict[int, bool] = {1: True, 2: False}\n    d[3] = True\n    d[1] = False\n    return sum(d.values())\n".to_string(),
    ));
    v.push((
        "mut_lenk".to_string(),
        "    d: dict[int, bool] = {1: True, 2: False, 3: True}\n    del d[2]\n    return len(d.keys())\n".to_string(),
    ));
    // RELOCATING 20-key grow (outruns the 16-slot literal slack), then reduce
    let grow: String = (1..=20)
        .map(|k| format!("{k}: {}", if k % 2 == 0 { "True" } else { "False" }))
        .collect::<Vec<_>>()
        .join(", ");
    v.push(("grow_sum".to_string(), bd(&grow, "sum(d.values())", "int")));
    v.push(("grow_lenv".to_string(), bd(&grow, "len(d.values())", "int")));
    v.push((
        "grow_max".to_string(),
        bd(&grow, "1 if max(d.values()) else 0", "int"),
    ));
    // int-valued view len cross-check (non-bool value kind)
    v.push((
        "int_lenk".to_string(),
        "    d: dict[int, int] = {1: 10, 2: 20, 3: 30}\n    return len(d.keys())\n".to_string(),
    ));
    v.push((
        "int_leni".to_string(),
        "    d: dict[str, int] = {\"a\": 1, \"b\": 2}\n    return len(d.items())\n".to_string(),
    ));
    v
}

fn all_probes() -> Vec<(String, String)> {
    let mut v = slice_probes();
    v.extend(reduction_probes());
    v
}

fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in all_probes() {
        src.push_str(&format!("def {name}() -> int:\n{body}\n"));
    }
    src
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn recent_surface_corpus_lowers_end_to_end() {
    let wat = emit(&corpus_source())
        .expect("the stepped-slice + bool-reduction + view-len corpus must lower");
    // PMAT-1327 stepped copy helper is present (the const-step slice runtime).
    assert!(
        wat.contains("$__wasm_str_slice_step"),
        "the stepped-slice corpus must emit the stepped copy helper:\n{wat}"
    );
    // PMAT-1328 bool extremum WRAPS i64→i32 (the fix — an i32 bool, not an i64).
    assert!(
        wat.contains("i32.wrap_i64"),
        "a bool min/max must wrap the i64 extremum back to an i32 bool:\n{wat}"
    );
}

/// The negative-step-WITH-bounds forms each refuse HONESTLY (never silently
/// mis-lowered). Each needle pins the SPECIFIC reason, so a refusal that fired
/// for the wrong cause — or a form that quietly started lowering — is caught.
/// This is the sharpening of the PMAT-1327 claim.
#[test]
fn negative_step_with_bounds_refuses_honestly() {
    let needle = "negative-step slice with bounds";
    for expr in [
        "s[5:0:-1]",   // both bounds
        "s[8:2:-1]",   // both bounds
        "s[9:0:-3]",   // both bounds, stride 3
        "s[4::-1]",    // start only
        "s[:4:-1]",    // stop only
        "s[-4::-1]",   // negative start only
        "s[-3:-8:-1]", // both negative
        "s[100::-1]",  // out-of-range start
        "s[0:0:-1]",   // empty-looking, still bounded
    ] {
        let src = format!(
            "def f() -> int:\n    s: str = {ASCII}\n    r: str = {expr}\n    return len(r)\n"
        );
        let err = match emit(&src) {
            Err(e) => e,
            Ok(wat) => {
                panic!("`{expr}` must be refused (negative step + bounds) but lowered:\n{wat}")
            }
        };
        assert!(
            err.contains(needle),
            "`{expr}` refusal should say {needle:?}, got: {err}"
        );
    }
    // The DUAL: a pure negative stride WITHOUT bounds DOES lower (the reverse
    // family), so the refusal is a bounds predicate, not a blanket neg-step ban.
    for ok_expr in ["s[::-1]", "s[::-2]", "s[::-100]"] {
        let src = format!(
            "def f() -> int:\n    s: str = {ASCII}\n    r: str = {ok_expr}\n    return len(r)\n"
        );
        assert!(
            emit(&src).is_ok(),
            "`{ok_expr}` (negative stride, no bounds) must lower — it is the reverse family"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

/// Parse a `name() => <ty>:<v>` line. `wasm-interp` prints integers as UNSIGNED
/// decimal; every observable here is a non-negative int, so `u64` → `i64` is exact.
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

/// Per-CALL unique work dir (pid + monotonic counter) so parallel libtest
/// threads never race on `prog.wat` (the PMAT-1320 witness gotcha).
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

fn assemble_and_run(wat: &str) -> (String, bool) {
    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-adv1331-{}-{}", std::process::id(), seq));
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

/// Execute the IDENTICAL corpus source in live python3, returning `name=value`
/// pairs — the differential ground truth.
fn python_truth(src: &str) -> Option<Vec<(String, i64)>> {
    let names: Vec<String> = all_probes().iter().map(|(n, _)| n.clone()).collect();
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
fn recent_surface_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the adversarial corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1331: skipping EXECUTED adversarial re-verify — WABT (wat2wasm / \
             wasm-interp) absent. The corpus lowered through the FULL pipeline \
             (PythonFrontend → emit_module) and the CONSTRUCT + refusal assertions \
             hold; a box with WABT also runs every export and value-matches live \
             python3 on the identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1331: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        all_probes().len(),
        "python3 must produce one value per probe"
    );

    eprintln!("PMAT-1331: running EXECUTED adversarial re-verify via WABT");
    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, *expected,
            "executed WASM {name}() = {got} but live CPython = {expected} on the \
             IDENTICAL source — REFUTED\nfull interp output:\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("unreachable executed"),
        "no adversarial probe should trap:\n{stdout}"
    );

    eprintln!(
        "PMAT-1331: EXECUTED adversarial re-verify PASSED — {} probes \
         (stepped/reverse/multibyte string slices with the i64-safe fingerprint, \
         bool-dict sum/min/max reductions incl. a relocating grow, and dict-view \
         len across value kinds + mutation) all == live python3. NOTHING REFUTED.",
        truth.len()
    );
}
