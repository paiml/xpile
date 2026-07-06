//! PMAT-1337 — ADVERSARIAL-VERIFY differential witness over the `any()` / `all()`
//! TRUTHINESS-REDUCE belt shipped as PMAT-1332..1336: the SCALAR list folds
//! (`list[int]`/`list[float]`, PMAT-1332), the dict `.values()` folds
//! (int/bool/float, PMAT-1333), the int-keyed dict-KEYS fold (PMAT-1334), the
//! `set[int]` fold (PMAT-1335), and the STRING fold over `set[str]` + str-keyed
//! dict keys (PMAT-1336). Under `C-COMPILE-RUST-TO-WASM` + `C-WASM-HEAP`.
//!
//! This is a SKEPTIC pass, not a feature. The belt grew five slices deep off ONE
//! shared idea — a `!= 0` (int) / `!= 0.0` (float) / `len != 0` (str) fold with an
//! `is_all`-XOR short-circuit — routed through three helpers
//! (`$__wasm_list_int_truthy_reduce`, `$__wasm_list_float_truthy_reduce`,
//! `$__wasm_hash_strkey_truthy_reduce`) fed by four materialisers/direct-folds. A
//! regression hides at the SEAMS the per-slice witnesses do not individually
//! stress: the exact truthiness predicate (nonzero, NOT `> 0`, so a NEGATIVE
//! value is truthy), the IEEE float edge (`-0.0` is FALSEY, a raw i64 `!= 0` on
//! its `0x8000…` bits would read truthy), the classic Python str gotchas (`"0"`,
//! `" "`, `"\t"` are ALL truthy — only `""` is falsey), the short-circuit
//! position (a falsey element at index 0 vs the last of a 40-element list), and
//! the post-mutation live region (a swap-last-into-hole `discard`/`del` and a
//! RELOCATING grow whose moved slot holds the falsey element).
//!
//! The probes below drive exactly those, plus cross-lane compositions
//! (`all(xs) and all(s)`), reductions across a FUNCTION param, and reductions
//! nested inside `if`/`while` bodies (walker parity — the gate must still declare
//! the helper). Every one is value-matched against LIVE python3 on the identical
//! source.
//!
//! ## Result of the sweep at the PMAT-1336 head: NOTHING REFUTED.
//!
//! All 73 executed observables value-match live python3, corroborated by an
//! independent fan-out skeptic pass (152 CPython-vs-WASM comparisons across 82
//! probe files, all matching, zero silent mis-lowerings). One extra trap the
//! fan-out surfaced is baked in permanently ([`i64_truncation_probes`]): a value
//! whose LOW 32 BITS are zero (`2^32`, `2^33`) is TRUTHY, and would misread FALSEY
//! only if a lane truncated the i64 payload to i32 before the `!= 0` test — every
//! int lane compares the full i64, so all four are True. The fan-out also proved
//! that a `list.append`-growth-past-static-slack RUNTIME TRAP (`unreachable`) is
//! REDUCE-INDEPENDENT (a no-`any`/`all` probe traps identically at the same append
//! count) — a pre-existing `list`-mutation capacity limit that fails LOUD (a trap,
//! never a wrong value) and is outside this reduce scope; set/dict relocating grows
//! have no such ceiling (verified to N=51).
//!
//! Two claims are SHARPENED (documented, not defects):
//!   * The IEEE float claim in the `list[float]` / `dict[_, float].values()` doc
//!     ("`NaN` is truthy, `-0.0` is falsy") is only HALF reachable through the
//!     pipeline: `-0.0` IS expressible (a literal) and its FALSEY truthiness is
//!     witnessed here, but `NaN` is NOT — `float("nan")`/`float("inf")` are
//!     refused honestly (a builtin constructor outside the WASM subset), and
//!     `0.0/0.0` RAISES `ZeroDivisionError` in CPython, so no CPython-agreeing
//!     source can feed a `NaN` to the fold. The `f64.ne 0.0` is correct by IEEE;
//!     the `NaN`-truthy leg is simply unreachable, not wrong.
//!   * `len(s)` is a Python CHAR count but the str ABI header is a BYTE count —
//!     for TRUTHINESS these agree exactly (empty iff 0 chars iff 0 bytes), so a
//!     MULTIBYTE nonempty key (`"é"`, `"日"`) folds truthy just like an ASCII one;
//!     witnessed here so a future byte/char confusion in the header read is caught.
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

/// A single observable probe: `def <name>() -> bool: <body>`. The bool return is
/// coerced to `int` (0/1) both sides for the differential.
type Probe = (String, String);

fn p(name: &str, body: &str) -> Probe {
    (name.to_string(), body.to_string())
}

/// `list[int]` / `list[float]` / `list[bool]` folds — the PMAT-1332 scalar lanes.
/// Drives the exact NONZERO predicate (negatives are truthy), the i64 boundaries,
/// every short-circuit position, the empty-list identities, and the IEEE `-0.0`
/// FALSEY edge (a raw i64 `!= 0` would misread its sign-bit-only pattern).
fn list_probes() -> Vec<Probe> {
    vec![
        // list[int]: nonzero truthiness — a NEGATIVE element is truthy (`!= 0`, not `> 0`).
        p(
            "li_neg_all",
            "    xs: list[int] = [-1, -2, -3]\n    return all(xs)",
        ),
        p(
            "li_neg_any",
            "    xs: list[int] = [-1, -2, -3]\n    return any(xs)",
        ),
        p(
            "li_zero_first_all",
            "    xs: list[int] = [0, 1, 2]\n    return all(xs)",
        ),
        p(
            "li_zero_last_all",
            "    xs: list[int] = [1, 2, 0]\n    return all(xs)",
        ),
        p(
            "li_zero_mid_all",
            "    xs: list[int] = [1, 0, 2]\n    return all(xs)",
        ),
        p(
            "li_all_zero_any",
            "    xs: list[int] = [0, 0, 0]\n    return any(xs)",
        ),
        p(
            "li_nonzero_last_any",
            "    xs: list[int] = [0, 0, 5]\n    return any(xs)",
        ),
        // i64 boundaries — both extremes are nonzero → truthy.
        p(
            "li_imax_all",
            "    xs: list[int] = [9223372036854775807]\n    return all(xs)",
        ),
        p(
            "li_imin_any",
            "    xs: list[int] = [-9223372036854775808]\n    return any(xs)",
        ),
        p(
            "li_single_zero_all",
            "    xs: list[int] = [0]\n    return all(xs)",
        ),
        p(
            "li_single_zero_any",
            "    xs: list[int] = [0]\n    return any(xs)",
        ),
        // empty-list identities: all([]) == True, any([]) == False.
        p("li_empty_all", "    xs: list[int] = []\n    return all(xs)"),
        p("li_empty_any", "    xs: list[int] = []\n    return any(xs)"),
        // list[float]: `-0.0` is FALSEY (bool(-0.0) is False) — the IEEE edge.
        p(
            "lf_negzero_any",
            "    xs: list[float] = [-0.0, 0.0]\n    return any(xs)",
        ),
        p(
            "lf_negzero_all",
            "    xs: list[float] = [-0.0, 0.0]\n    return all(xs)",
        ),
        p(
            "lf_neg_all",
            "    xs: list[float] = [-1.5, -2.5]\n    return all(xs)",
        ),
        p(
            "lf_zero_mid_all",
            "    xs: list[float] = [1.5, 0.0, 2.5]\n    return all(xs)",
        ),
        p(
            "lf_tiny_all",
            "    xs: list[float] = [1e-300, 2e-300]\n    return all(xs)",
        ),
        p(
            "lf_zero_any",
            "    xs: list[float] = [0.0, 0.0]\n    return any(xs)",
        ),
        // list[bool]: the original PMAT-1251 i32 (0/1) fold.
        p(
            "lb_false_all",
            "    xs: list[bool] = [True, False]\n    return all(xs)",
        ),
        p(
            "lb_false_any",
            "    xs: list[bool] = [False, False]\n    return any(xs)",
        ),
    ]
}

/// `set[int]` (PMAT-1335) + int-keyed dict-KEYS (PMAT-1334) + dict `.values()`
/// int/bool/float (PMAT-1333) folds. Keys are unique, so an "all-zero" dict is
/// exactly `{0: v}`; the float `.values()` lane must fold as f64 (`-0.0` falsey).
fn set_dict_probes() -> Vec<Probe> {
    vec![
        // set[int]: negatives truthy, zero present, empty identities.
        p(
            "si_neg_all",
            "    s: set[int] = {-1, -2, -3}\n    return all(s)",
        ),
        p(
            "si_zero_all",
            "    s: set[int] = {0, 1, 2}\n    return all(s)",
        ),
        p("si_zero_any", "    s: set[int] = {0}\n    return any(s)"),
        p(
            "si_nonzero_any",
            "    s: set[int] = {0, 7}\n    return any(s)",
        ),
        // int-keyed dict: `any(d)`/`all(d)` iterate the KEYS.
        p(
            "di_neg_all",
            "    d: dict[int, int] = {-1: 5, -2: 6}\n    return all(d)",
        ),
        p(
            "di_zerokey_all",
            "    d: dict[int, int] = {0: 5, 1: 6}\n    return all(d)",
        ),
        p(
            "di_zerokey_any",
            "    d: dict[int, int] = {0: 5}\n    return any(d)",
        ),
        p(
            "di_keys_view_all",
            "    d: dict[int, int] = {0: 5, 1: 6}\n    return all(d.keys())",
        ),
        // dict.values() int — zero present.
        p(
            "dv_int_zero_all",
            "    d: dict[int, int] = {1: 0, 2: 3}\n    return all(d.values())",
        ),
        p(
            "dv_int_zero_any",
            "    d: dict[int, int] = {1: 0, 2: 0}\n    return any(d.values())",
        ),
        // dict.values() bool.
        p(
            "dv_bool_false_all",
            "    d: dict[int, bool] = {1: True, 2: False}\n    return all(d.values())",
        ),
        p(
            "dv_bool_false_any",
            "    d: dict[int, bool] = {1: False, 2: False}\n    return any(d.values())",
        ),
        // dict.values() float — MUST fold as f64: `-0.0` is falsey.
        p(
            "dv_flt_negzero_any",
            "    d: dict[int, float] = {1: -0.0, 2: 0.0}\n    return any(d.values())",
        ),
        p(
            "dv_flt_zero_all",
            "    d: dict[int, float] = {1: 0.0, 2: 1.5}\n    return all(d.values())",
        ),
    ]
}

/// `set[str]` + str-keyed dict folds (PMAT-1336). The classic Python str
/// truthiness gotchas: `"0"`, `" "`, `"\t"`, and MULTIBYTE keys are ALL truthy
/// (len != 0); only `""` is falsey. `len` is a CHAR count but the ABI header is a
/// BYTE count — exact for truthiness (empty iff 0 chars iff 0 bytes).
fn str_probes() -> Vec<Probe> {
    vec![
        p(
            "ss_zero_all",
            "    s: set[str] = {\"0\"}\n    return all(s)",
        ),
        p(
            "ss_space_all",
            "    s: set[str] = {\" \"}\n    return all(s)",
        ),
        p(
            "ss_tab_all",
            "    s: set[str] = {\"\\t\"}\n    return all(s)",
        ),
        // multibyte: a byte-count header must still read nonzero for a nonempty key.
        p(
            "ss_multibyte_all",
            "    s: set[str] = {\"é\", \"日\"}\n    return all(s)",
        ),
        p(
            "ss_empty_present_all",
            "    s: set[str] = {\"a\", \"\"}\n    return all(s)",
        ),
        p(
            "ss_empty_present_any",
            "    s: set[str] = {\"\", \"\"}\n    return any(s)",
        ),
        p(
            "ss_nonempty_any",
            "    s: set[str] = {\"x\"}\n    return any(s)",
        ),
        // str-keyed dict: `any(d)`/`all(d)` iterate the KEYS.
        p(
            "ds_zerokey_all",
            "    d: dict[str, int] = {\"0\": 1, \"false\": 2}\n    return all(d)",
        ),
        p(
            "ds_space_all",
            "    d: dict[str, int] = {\" \": 1}\n    return all(d)",
        ),
        p(
            "ds_empty_all",
            "    d: dict[str, int] = {\"\": 1, \"x\": 2}\n    return all(d)",
        ),
        p(
            "ds_keys_view_any",
            "    d: dict[str, int] = {\"\": 1, \"x\": 2}\n    return any(d.keys())",
        ),
    ]
}

/// Mutation-then-reduce + relocating grows. A `discard`/`del` swaps the last live
/// entry into the hole and decrements the count, so the live region `[0, n)` must
/// still fold correctly; a >16-element literal outruns the slot slack and forces a
/// real relocation whose MOVED slot holds the falsey element (`0` / `""`).
fn mutation_probes() -> Vec<Probe> {
    let mut v = vec![
        p(
            "mut_set_discard_all",
            "    s: set[int] = {1, 2, 3}\n    s.discard(2)\n    return all(s)",
        ),
        p(
            "mut_set_add_zero_all",
            "    s: set[int] = {1, 2}\n    s.add(0)\n    return all(s)",
        ),
        p(
            "mut_dict_del_all",
            "    d: dict[int, int] = {1: 9, 0: 9, 2: 9}\n    del d[0]\n    return all(d)",
        ),
        p(
            "mut_sset_discard_empty_all",
            "    s: set[str] = {\"\", \"a\", \"b\"}\n    s.discard(\"\")\n    return all(s)",
        ),
        p(
            "mut_sdict_del_empty_all",
            "    d: dict[str, int] = {\"\": 1, \"a\": 2}\n    del d[\"a\"]\n    return all(d)",
        ),
    ];
    // A relocating grow (20 elements) whose moved region includes the falsey key.
    let grow_int: String = (1..20)
        .map(|k| k.to_string())
        .chain(std::iter::once("0".to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    v.push(p(
        "grow_set_has_zero_all",
        &format!("    s: set[int] = {{{grow_int}}}\n    return all(s)"),
    ));
    v.push(p(
        "grow_set_has_zero_any",
        &format!("    s: set[int] = {{{grow_int}}}\n    return any(s)"),
    ));
    let grow_dict: String = (1..20)
        .map(|k| format!("{k}: 1"))
        .chain(std::iter::once("0: 1".to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    v.push(p(
        "grow_dict_has_zero_all",
        &format!("    d: dict[int, int] = {{{grow_dict}}}\n    return all(d)"),
    ));
    let grow_str: String = (1..20)
        .map(|k| format!("\"s{k}\""))
        .chain(std::iter::once("\"\"".to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    v.push(p(
        "grow_sset_has_empty_all",
        &format!("    s: set[str] = {{{grow_str}}}\n    return all(s)"),
    ));
    v.push(p(
        "grow_sset_has_empty_any",
        &format!("    s: set[str] = {{{grow_str}}}\n    return any(s)"),
    ));
    v
}

/// Large-N short-circuit position: a 40-element list with the deciding element at
/// index 0, the middle, and the last — the fold must scan far enough (any/all
/// short-circuit) yet still decide correctly regardless of position.
fn large_n_probes() -> Vec<Probe> {
    let li = |zero_at: usize| -> String {
        let mut vals = vec!["1"; 40];
        vals[zero_at] = "0";
        format!("[{}]", vals.join(", "))
    };
    let li_one = |one_at: usize| -> String {
        let mut vals = vec!["0"; 40];
        vals[one_at] = "1";
        format!("[{}]", vals.join(", "))
    };
    vec![
        p(
            "big_zero_first_all",
            &format!("    xs: list[int] = {}\n    return all(xs)", li(0)),
        ),
        p(
            "big_zero_mid_all",
            &format!("    xs: list[int] = {}\n    return all(xs)", li(20)),
        ),
        p(
            "big_zero_last_all",
            &format!("    xs: list[int] = {}\n    return all(xs)", li(39)),
        ),
        p(
            "big_one_first_any",
            &format!("    xs: list[int] = {}\n    return any(xs)", li_one(0)),
        ),
        p(
            "big_one_last_any",
            &format!("    xs: list[int] = {}\n    return any(xs)", li_one(39)),
        ),
    ]
}

/// Cross-lane compositions + reductions nested inside `if`/`while` bodies (walker
/// parity: the gate walker must still declare the helper when the `BoolReduce`
/// sits under a control-flow statement).
fn composition_probes() -> Vec<Probe> {
    vec![
        p("comp_and", "    xs: list[int] = [1, 2]\n    s: set[str] = {\"a\"}\n    return all(xs) and all(s)"),
        p("comp_or", "    xs: list[int] = [0, 0]\n    s: set[str] = {\"\"}\n    return any(xs) or any(s)"),
        p("comp_not_any", "    s: set[str] = {\"\", \"a\"}\n    return not any(s)"),
        p("cf_if_reduce", "    xs: list[int] = [1, 0, 2]\n    r: bool = False\n    if len(xs) > 0:\n        r = all(xs)\n    return r"),
        p("cf_while_reduce", "    xs: list[int] = [0, 1]\n    r: bool = True\n    i: int = 0\n    while i < 1:\n        r = any(xs)\n        i = i + 1\n    return r"),
    ]
}

/// The i64-TRUNCATION trap (surfaced by the independent skeptic fan-out): a value
/// whose LOW 32 BITS are zero — `2^32 == 4294967296`, `2^33 == 8589934592` — is
/// nonzero and therefore TRUTHY, but a fold that truncated the i64 payload to an
/// i32 before the `!= 0` test would read it FALSEY. All four int lanes (list, set,
/// dict-key, dict-value) MUST compare the full i64, so every one of these is True.
/// If any lane ever regresses to an i32-truncated compare, exactly these flip.
fn i64_truncation_probes() -> Vec<Probe> {
    vec![
        p(
            "li_low32zero_all",
            "    xs: list[int] = [4294967296, 8589934592]\n    return all(xs)",
        ),
        p(
            "si_low32zero_all",
            "    s: set[int] = {4294967296}\n    return all(s)",
        ),
        p(
            "di_low32zero_all",
            "    d: dict[int, int] = {4294967296: 1}\n    return all(d)",
        ),
        p(
            "dv_low32zero_all",
            "    d: dict[int, int] = {1: 4294967296}\n    return all(d.values())",
        ),
    ]
}

/// Every observable probe (the ones with a zero-arg `def`).
fn observable_probes() -> Vec<Probe> {
    let mut v = list_probes();
    v.extend(set_dict_probes());
    v.extend(str_probes());
    v.extend(mutation_probes());
    v.extend(large_n_probes());
    v.extend(composition_probes());
    v.extend(i64_truncation_probes());
    v
}

/// Observable names PLUS the three param-boundary callers (whose reduce runs in a
/// callee taking a `list[int]`/`set[str]`/`dict[str, int]` param).
fn observable_names() -> Vec<String> {
    let mut names: Vec<String> = observable_probes().iter().map(|(n, _)| n.clone()).collect();
    names.push("param_li_all".to_string());
    names.push("param_ss_any".to_string());
    names.push("param_ds_all".to_string());
    names
}

/// The corpus: observable exports + the param-boundary helpers/callers.
fn corpus_source() -> String {
    let mut src = String::new();
    for (name, body) in observable_probes() {
        src.push_str(&format!("def {name}() -> bool:\n{body}\n\n"));
    }
    // A list[int] / set[str] / dict[str, int] reduced across a function param.
    src.push_str("def _red_li(xs: list[int]) -> bool:\n    return all(xs)\n");
    src.push_str(
        "def param_li_all() -> bool:\n    xs: list[int] = [3, 0, 5]\n    return _red_li(xs)\n",
    );
    src.push_str("def _red_ss(s: set[str]) -> bool:\n    return any(s)\n");
    src.push_str(
        "def param_ss_any() -> bool:\n    s: set[str] = {\"\", \"\"}\n    return _red_ss(s)\n",
    );
    src.push_str("def _red_ds(d: dict[str, int]) -> bool:\n    return all(d)\n");
    src.push_str("def param_ds_all() -> bool:\n    d: dict[str, int] = {\"a\": 1, \"\": 2}\n    return _red_ds(d)\n");
    src
}

// ---- CONSTRUCT assertions (hold with or without WABT) ------------------------

#[test]
fn truthy_reduce_corpus_lowers_end_to_end() {
    let wat = emit(&corpus_source()).expect("the truthiness-reduce corpus must lower");
    // All three fold helpers are DECLARED (their gates tripped) — else wat2wasm
    // rejects the module for an undefined call.
    for helper in [
        "$__wasm_list_int_truthy_reduce",
        "$__wasm_list_float_truthy_reduce",
        "$__wasm_hash_strkey_truthy_reduce",
    ] {
        assert!(
            wat.contains(&format!("(func {helper}")),
            "the {helper} fold helper must be DECLARED:\n{wat}"
        );
    }
}

// ---- honest refusals (through the FULL pipeline) ------------------------------

/// Forms OUTSIDE the reduce lanes must REFUSE — never silently mis-lowered. A
/// silent mis-lowering (emit succeeds, wasm != cpython) would be the highest-
/// severity defect this whole belt could hide; these guard against it.
#[test]
fn truthy_reduce_out_of_lane_forms_refuse() {
    for (label, src, needle) in [
        // str-VALUED dict `.values()` — the value-slot str fold is deferred (keys-only).
        (
            "any(str-valued d.values())",
            "def f() -> bool:\n    d: dict[int, str] = {1: \"a\", 2: \"\"}\n    return any(d.values())\n",
            "str-valued dict",
        ),
        // the lazy short-circuiting GENERATOR form (a per-element predicate lambda).
        (
            "any(<generator over s>)",
            "def f() -> bool:\n    s: set[str] = {\"\", \"a\"}\n    return any(len(x) > 0 for x in s)\n",
            "<generator>",
        ),
    ] {
        let err = match emit(src) {
            Err(e) => e,
            Ok(wat) => panic!("{label} must be refused but lowered:\n{wat}"),
        };
        assert!(
            err.contains(needle),
            "{label} refusal should mention {needle:?}, got: {err}"
        );
    }
}

// ---- WABT harness -------------------------------------------------------------

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

/// Per-CALL unique work dir (pid + monotonic counter) so parallel libtest threads
/// never race on `prog.wat` (the PMAT-1320 witness gotcha).
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

fn assemble_and_run(wat: &str) -> (String, bool) {
    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("xpile-wasm-adv1337-{}-{}", std::process::id(), seq));
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

/// Execute the IDENTICAL corpus source in live python3 — the differential ground
/// truth. Each bool is coerced to `int` (0/1) to match `wasm-interp`'s i32 print.
fn python_truth(src: &str) -> Option<Vec<(String, i64)>> {
    let names = observable_names();
    let driver =
        format!("{src}\nprint(';'.join(f'{{n}}={{int(globals()[n]())}}' for n in {names:?}))\n");
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
fn truthy_reduce_executes_in_wasm_and_matches_cpython() {
    let src = corpus_source();
    let wat = emit(&src).expect("the adversarial corpus must lower end-to-end");

    if !wasm_runtime_available() {
        eprintln!(
            "PMAT-1337: skipping EXECUTED adversarial re-verify — WABT (wat2wasm / \
             wasm-interp) absent. The corpus lowered through the FULL pipeline \
             (PythonFrontend → emit_module) and the CONSTRUCT + refusal assertions \
             hold; a box with WABT also runs every export and value-matches live \
             python3 on the identical source. Free CI skips execution and stays green."
        );
        return;
    }
    let Some(truth) = python_truth(&src) else {
        eprintln!("PMAT-1337: python3 absent — witness asserted at emit level only");
        return;
    };
    assert_eq!(
        truth.len(),
        observable_names().len(),
        "python3 must produce one value per observable probe"
    );

    let (stdout, ok) = assemble_and_run(&wat);
    assert!(ok, "wasm-interp run failed:\n{stdout}");

    for (name, expected) in &truth {
        let got = parse_scalar_export(&stdout, name);
        assert_eq!(
            got, *expected,
            "truthiness reduce `{name}`: wasm {got} != cpython {expected}\n{stdout}"
        );
    }
    eprintln!(
        "PMAT-1337: {} any/all truthiness observables across list[int/float/bool], \
         set[int/str], int/str-keyed dicts, and dict.values() int/bool/float — \
         negatives, i64 boundaries, IEEE -0.0, the \"0\"/\" \"/\"\\t\"/multibyte str \
         gotchas, empty-region identities, post-mutation + relocating-grow live \
         regions, 40-element short-circuit positions, cross-lane compositions, \
         control-flow-nested reduces, and three param boundaries — all == live \
         python3. NOTHING REFUTED.",
        truth.len()
    );
}
