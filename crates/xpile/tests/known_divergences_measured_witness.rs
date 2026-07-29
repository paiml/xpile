//! XPILE-DIVERGENCE-001 (PMAT-1474) — the "Known divergences" list is a set of
//! falsifiable behavioural claims, so a gate can RUN them.
//!
//! THE DEFECT. `CHANGELOG.md`'s `[Unreleased]` "Known divergences" section is
//! the passage a reader consults to decide what this project cannot be trusted
//! with. [[PMAT-1473]] found its first stale entry — item 5, reporting a Lean
//! defect PMAT-1405 had fixed — and named the class. This slice swept the other
//! eight. **Three had drifted, and the worst of them was item 1**, the most
//! severe claim in the document:
//!
//! > **1. i64 arithmetic overflow is still a SILENT WRONG ANSWER in the WASM
//! > lane.** … the lane maps them to `i64` and wraps without a word.
//! > `9223372036854775807 + 1` returns `-9223372036854775808`;
//! > `3037000500 * 3037000500` returns `-9223372036709301616` …
//!
//! **PMAT-1402, an arc 1,900 lines above it in the same `[Unreleased]`
//! section, had already made every one of those return values
//! unreproducible.** `+`, `-`, `*` and unary `-` route through
//! `$__wasm_add_i64` / `$__wasm_sub_i64` / `$__wasm_mul_i64` and **trap**.
//! Exactly one shape survives, and the entry named it correctly:
//! `abs(i64::MIN)` returns `i64::MIN`, because `$__wasm_abs_i64` computes
//! `0 - x` with a bare `i64.sub` and `NumBuiltin` never reaches `emit_binop`,
//! which is where PMAT-1402 landed.
//!
//! WHY THIS DIRECTION IS THE EXPENSIVE ONE. Every prior slice in this sweep
//! hunted OVER-claiming. This entry UNDER-claimed: it told a reader evaluating
//! the native-WASM lane that every i64 add silently wraps, when the emitter
//! traps — and it aimed the 0.1.619 lane at *"overflow-checked add/sub/mul/neg
//! across every i64 operation, L/XL, on the emitter's hot path"*, work that had
//! shipped. The surviving residual is S and off the hot path. A stale
//! divergence list does not just misinform; it misdirects the roadmap.
//!
//! AND NOTHING WAS HIDING IT.
//! `crates/xpile-wasm-codegen/tests/i64_overflow_witness.rs` has asserted the
//! trap behaviour, green, since 2026-07-27, and its module doc names
//! `abs(i64::MIN)` as the survivor. The executing witness and the release note
//! disagreed for two days and only the note was wrong. **A green witness does
//! not propagate to the prose that describes it** — which is the whole reason
//! this file exists in the same repository as that one.
//!
//! WHAT THIS FILE PINS:
//!
//! 1. **The behaviour is MEASURED here** (rules A–C), through the shipped CLI,
//!    with in-range CONTROLS — without them a uniformly trapping module is
//!    indistinguishable from one that no longer runs at all.
//! 2. **The prose is held to the measurement** (rule D), **bidirectionally**: if
//!    `abs` is ever routed through the checked helper, the disclosure this slice
//!    wrote becomes the stale claim and rule B reds. A one-directional gate
//!    would silently bless the fix into a new falsehood.
//! 3. **The two METRIC entries are held to the pointer doctrine** (rules E–F,
//!    PMAT-1449 / PMAT-1470 / PMAT-1471). Item 6's roster size drifted 18 → 14
//!    when PMAT-1432 tightened the stratum; item 7 typed nine live witness
//!    counts, four of which drifted in two days — **directly above its own
//!    sentence forbidding exactly that**. Rule E re-derives item 6 by running
//!    `xpile quorum`; rule F requires item 7 to publish its command instead of
//!    its result.
//! 4. **Both prose detectors carry a positive control** (rule G) that feeds them
//!    the PRE-FIX text and asserts they fire. A detector that matches nothing is
//!    a green test that checks nothing.
//!
//! SHIPPED RELEASE SECTIONS ARE OUT OF SCOPE, DELIBERATELY, on PMAT-1473's
//! reasoning: `## [0.1.617]` carries the old wording and it is CORRECT there —
//! it describes what 0.1.617 shipped. The subject is `[Unreleased]`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn xpile_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xpile")
}

fn tool_present(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// WABT gates the EXECUTING half only. Every structural rule below runs
/// unconditionally, so a runner without WABT cannot make this file vacuous.
fn wabt_present() -> bool {
    tool_present("wat2wasm") && tool_present("wasm-interp")
}

/// PER-CALL directory. Tests in one binary run in PARALLEL and several call
/// this; a pid-only path let one test's cleanup delete another's input mid-run
/// ([[PMAT-1436]]'s shared-state lesson, re-learned by this file's red half).
fn scratch(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("xpile_divergence_{tag}_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

// ── The probe module ────────────────────────────────────────────────────────
//
// Every value is built from an in-range literal and an in-range operation, so
// the module contains no out-of-i64 LITERAL — that shape is refused by the WASM
// contract surface (PMAT-1350) and would abort the emit rather than exercise
// the arithmetic this file is about.

const PROBE_PY: &str = r#"
def ovf_add() -> int:
    a: int = 9223372036854775807
    return a + 1

def ovf_sub() -> int:
    a: int = -9223372036854775807
    return a - 2

def ovf_mul() -> int:
    b: int = 3037000500
    return b * b

def ovf_neg() -> int:
    a: int = -9223372036854775807
    return -(a - 1)

def abs_min_is_negative() -> bool:
    a: int = -9223372036854775807
    return abs(a - 1) < 0

def ctrl_add() -> int:
    a: int = 5
    return a + 1

def ctrl_mul() -> int:
    b: int = 1000
    return b * b

def ctrl_abs_is_negative() -> bool:
    a: int = -5
    return abs(a) < 0
"#;

/// Emit the probe module through the SHIPPED CLI under DEFAULT flags.
fn emit_probe_wat() -> String {
    let dir = scratch("emit");
    let src = dir.join("probe.py");
    std::fs::write(&src, PROBE_PY).expect("write probe.py");
    let out_file = dir.join("probe.wat");
    let out = Command::new(xpile_bin())
        .arg("transpile")
        .arg(&src)
        .args(["--target", "wasm", "--out"])
        .arg(&out_file)
        .output()
        .expect("spawn xpile");
    assert!(
        out.status.success(),
        "`xpile transpile --target wasm` failed on the overflow probe module:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let wat = std::fs::read_to_string(&out_file).expect("read emitted WAT");
    let _ = std::fs::remove_dir_all(&dir);
    wat
}

/// The body of one `(func $name …)` in the emitted WAT, up to the next `(func`.
fn func_body<'a>(wat: &'a str, name: &str) -> &'a str {
    let needle = format!("(func ${name} ");
    let start = wat
        .find(&needle)
        .unwrap_or_else(|| panic!("emitted WAT has no `{needle}…`:\n{wat}"));
    let rest = &wat[start..];
    let end = rest[1..]
        .find("(func $")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `name => Ok(value)` for an i64/i32 export, or `Err(())` when it trapped.
fn run_all_exports(wat: &str) -> Vec<(String, Result<i64, ()>)> {
    let dir = scratch("run");
    let wat_p = dir.join("m.wat");
    let wasm_p = dir.join("m.wasm");
    std::fs::write(&wat_p, wat).expect("write wat");
    let conv = Command::new("wat2wasm")
        .arg(&wat_p)
        .arg("-o")
        .arg(&wasm_p)
        .output()
        .expect("spawn wat2wasm");
    assert!(
        conv.status.success(),
        "`wat2wasm` REJECTED the emitted probe module:\n{}",
        String::from_utf8_lossy(&conv.stderr)
    );
    let out = Command::new("wasm-interp")
        .arg("--run-all-exports")
        .arg(&wasm_p)
        .output()
        .expect("spawn wasm-interp");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let _ = std::fs::remove_dir_all(&dir);

    let mut rows = Vec::new();
    for line in stdout.lines() {
        let Some((lhs, rhs)) = line.split_once("=>") else {
            continue;
        };
        let name = lhs.trim().trim_end_matches("()").trim().to_string();
        let rhs = rhs.trim();
        if rhs.starts_with("error") {
            rows.push((name, Err(())));
        } else if let Some((_ty, v)) = rhs.rsplit_once(':') {
            // wasm-interp prints integer exports UNSIGNED. Reinterpret at the
            // declared width, or `abs(i64::MIN)` reads as +9223372036854775808
            // and the divergence disappears into a plausible-looking number.
            let parsed = v
                .trim()
                .parse::<u64>()
                .map(|u| u as i64)
                .or_else(|_| v.trim().parse::<i64>());
            if let Ok(n) = parsed {
                rows.push((name, Ok(n)));
            }
        }
    }
    rows
}

fn value_of(rows: &[(String, Result<i64, ()>)], name: &str) -> Result<i64, ()> {
    rows.iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("`wasm-interp` did not report an export `{name}`"))
        .1
}

// ── Rule A: the checked helpers are what `+`/`-`/`*`/neg lower to ───────────

#[test]
fn wasm_arithmetic_routes_through_the_checked_overflow_helpers() {
    let wat = emit_probe_wat();
    for (func, helper) in [
        ("ovf_add", "call $__wasm_add_i64"),
        ("ovf_sub", "call $__wasm_sub_i64"),
        ("ovf_mul", "call $__wasm_mul_i64"),
    ] {
        let body = func_body(&wat, func);
        assert!(
            body.contains(helper),
            "`{func}` no longer routes through `{helper}`. PMAT-1402 has REGRESSED, and the \
             Known-divergences item 1 that PMAT-1474 rewrote to say these operators TRAP is now \
             itself false — revert it.\n--- body ---\n{body}"
        );
    }
    // Unary `-` lowers to the `x * -1` form, so it inherits the checked multiply.
    let neg = func_body(&wat, "ovf_neg");
    assert!(
        neg.contains("call $__wasm_mul_i64") || neg.contains("call $__wasm_sub_i64"),
        "unary negation no longer routes through a checked helper:\n{neg}"
    );
}

// ── Rule B: `abs` is still the documented unchecked residual ────────────────

#[test]
fn the_abs_helper_is_still_an_unchecked_subtraction() {
    let wat = emit_probe_wat();
    let helper = func_body(&wat, "__wasm_abs_i64");
    assert!(
        helper.contains("i64.sub"),
        "`$__wasm_abs_i64` no longer subtracts at all; its shape changed and the \
         Known-divergences item 1 disclosure must be re-derived:\n{helper}"
    );
    assert!(
        !helper.contains("call $__wasm_sub_i64"),
        "`$__wasm_abs_i64` now routes through the CHECKED subtract — `abs(i64::MIN)` presumably \
         traps, which means the surviving residual PMAT-1474 disclosed in CHANGELOG.md \
         `[Unreleased]` Known-divergences item 1 is FIXED and that entry is now the stale claim. \
         Delete the residual paragraph and move the 0.1.619 lane item to done.\n{helper}"
    );
}

// ── Rule C: run it ─────────────────────────────────────────────────────────

#[test]
fn overflow_traps_while_abs_of_i64_min_silently_wraps() {
    if !wabt_present() {
        eprintln!(
            "warning: wat2wasm/wasm-interp not on PATH; skipping the EXECUTING half of \
             XPILE-DIVERGENCE-001. The structural halves (rules A, B, D–G) still ran."
        );
        return;
    }
    let rows = run_all_exports(&emit_probe_wat());

    // CONTROLS FIRST. A module that traps on everything — because it failed to
    // build, or because a runtime changed — would pass the trap assertions
    // below while proving nothing at all.
    assert_eq!(
        value_of(&rows, "ctrl_add"),
        Ok(6),
        "the in-range control `5 + 1` did not answer 6; the trap assertions below would be vacuous"
    );
    assert_eq!(
        value_of(&rows, "ctrl_mul"),
        Ok(1_000_000),
        "the in-range control `1000 * 1000` did not answer 1000000"
    );
    assert_eq!(
        value_of(&rows, "ctrl_abs_is_negative"),
        Ok(0),
        "the control `abs(-5) < 0` is not false; the `abs` differential below would be vacuous"
    );

    for probe in ["ovf_add", "ovf_sub", "ovf_mul", "ovf_neg"] {
        assert_eq!(
            value_of(&rows, probe),
            Err(()),
            "`{probe}` returned a VALUE where it must trap. i64 overflow is silently wrapping \
             again (PMAT-1402 regression), and Known-divergences item 1 — which PMAT-1474 \
             rewrote to say these trap — has become false in the other direction."
        );
    }

    // THE SURVIVOR, measured by a route that does not depend on wasm-interp's
    // unsigned printing: `abs(i64::MIN) < 0` is TRUE in the module and False in
    // CPython. This is the one claim item 1 still legitimately makes.
    assert_eq!(
        value_of(&rows, "abs_min_is_negative"),
        Ok(1),
        "`abs(i64::MIN) < 0` is no longer true in the emitted module — the silent wrap has been \
         fixed, so the residual disclosed in Known-divergences item 1 is stale. See rule B."
    );
}

// ── The CHANGELOG subject ──────────────────────────────────────────────────

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// `[Unreleased]` only — shipped sections describe what they shipped.
fn unreleased_section() -> String {
    let body = read("CHANGELOG.md");
    let a = body
        .find("## [Unreleased]")
        .expect("CHANGELOG.md has an [Unreleased] section");
    let rest = &body[a + "## [Unreleased]".len()..];
    let b = rest.find("\n## [").map(|i| i + 1).unwrap_or(rest.len());
    rest[..b].to_string()
}

/// The `### Known divergences` block inside `[Unreleased]`.
fn known_divergences() -> String {
    let un = unreleased_section();
    let a = un
        .find("### Known divergences")
        .expect("[Unreleased] has a `### Known divergences` section");
    let rest = &un[a..];
    let b = rest[1..]
        .find("\n### ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    rest[..b].to_string()
}

/// One numbered entry, `**N. …**` up to the next `**M. ` or the section end.
fn divergence_item(n: usize) -> String {
    let sec = known_divergences();
    let head = format!("**{n}. ");
    let a = sec
        .find(&head)
        .unwrap_or_else(|| panic!("Known divergences has no item {n}"));
    let rest = &sec[a..];
    let next = format!("**{}. ", n + 1);
    let b = rest.find(&next).unwrap_or(rest.len());
    rest[..b].to_string()
}

/// Prose is line-wrapped, so every needle below is matched against a
/// whitespace-collapsed form. Matching raw text is how a banned sentence
/// survives a gate simply by falling across a line break.
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Rule D: item 1 agrees with rules A–C ───────────────────────────────────

/// Assertions that PMAT-1402 made unreproducible. Each is banned because the
/// measurement above contradicts it — not because of how it is worded.
const RETIRED_ITEM_1_CLAIMS: &[&str] = &[
    "wraps without a word",
    "-9223372036709301616",
    "i64 arithmetic overflow is still a SILENT WRONG ANSWER",
    "the remaining fix is overflow-checked",
];

fn retired_claims_present(text: &str) -> Vec<&'static str> {
    let f = flat(text);
    RETIRED_ITEM_1_CLAIMS
        .iter()
        .copied()
        .filter(|c| f.contains(&flat(c)))
        .collect()
}

#[test]
fn known_divergences_item_1_matches_the_measurement() {
    let item = divergence_item(1);
    let stale = retired_claims_present(&item);
    assert!(
        stale.is_empty(),
        "CHANGELOG.md `[Unreleased]` Known-divergences item 1 still asserts what this file \
         MEASURES to be false — `+`/`-`/`*`/neg trap (rules A and C). Stale claims: {stale:?}"
    );
    let f = flat(&item);
    assert!(
        f.contains("trap") || f.contains("TRAP"),
        "item 1 no longer says the arithmetic operators trap, which is what they do:\n{f}"
    );
    assert!(
        f.contains("abs("),
        "item 1 no longer discloses the surviving `abs(i64::MIN)` residual that rules B and C \
         measure. That residual is live; dropping the disclosure makes the section OMIT a real \
         divergence:\n{f}"
    );
}

// ── Rule E: item 6 re-derived by running `xpile quorum` ────────────────────

/// `(contract, runtime_votes)` from the live `xpile quorum` table.
fn quorum_runtime_votes() -> Vec<(String, u32)> {
    let out = Command::new(xpile_bin())
        .arg("quorum")
        .current_dir(workspace_root())
        .output()
        .expect("spawn xpile quorum");
    assert!(
        out.status.success(),
        "`xpile quorum` failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut rows = Vec::new();
    for line in stdout.lines() {
        let t = line.trim();
        if !t.starts_with("C-") {
            continue;
        }
        let cols: Vec<&str> = t.split_whitespace().collect();
        // contract Sem Sym Run Ext status
        if cols.len() >= 5 {
            if let Ok(run) = cols[3].parse::<u32>() {
                rows.push((cols[0].to_string(), run));
            }
        }
    }
    assert!(
        rows.len() > 20,
        "parsed only {} rows out of `xpile quorum`; the table format changed and this rule is \
         measuring nothing",
        rows.len()
    );
    rows
}

/// The two contracts item 6 names as backed by EXECUTING witnesses.
const EXECUTING_CONTRACTS: &[&str] = &["C-COMPILE-RUST-TO-WASM", "C-WASM-HEAP"];

#[test]
fn known_divergences_item_6_agrees_with_a_live_quorum_run() {
    let rows = quorum_runtime_votes();
    let mut heavy: Vec<&str> = rows
        .iter()
        .filter(|(_, run)| *run >= 50)
        .map(|(c, _)| c.as_str())
        .collect();
    heavy.sort_unstable();
    let mut expected: Vec<&str> = EXECUTING_CONTRACTS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        heavy, expected,
        "item 6 says exactly two contracts are backed by executing witnesses, an order of \
         magnitude above the rest. The live `xpile quorum` no longer agrees — update the entry \
         (and note it names the contracts, not a roster size, deliberately)."
    );

    let max_other = rows
        .iter()
        .filter(|(c, _)| !EXECUTING_CONTRACTS.contains(&c.as_str()))
        .map(|(_, run)| *run)
        .max()
        .unwrap_or(0);
    assert!(
        max_other < 20,
        "a third contract now carries {max_other} Runtime votes, so \"an order of magnitude above \
         every other row\" is no longer the honest reading of item 6"
    );

    let item = flat(&divergence_item(6));
    for c in EXECUTING_CONTRACTS {
        assert!(
            item.contains(c),
            "item 6 no longer names `{c}` as one of the two executing-witness contracts"
        );
    }
    assert!(
        item.contains("xpile quorum"),
        "item 6 must publish the command that derives its roster, not the roster size — the \
         typed size drifted 18 -> 14 when PMAT-1432 tightened the stratum:\n{item}"
    );
    assert!(
        roster_size_sites(&item).is_empty(),
        "item 6 types a roster size again: {:?}",
        roster_size_sites(&item)
    );
}

/// Drop `"…"` spans. A **disclosed quotation** of the wrong number is how an
/// entry explains its own correction, and banning it would forbid the repair
/// this slice is making — the exemption PMAT-1470 established, as a POSITIVE
/// marker (inside quotes) rather than a negation screen. The first run of rule E
/// flagged the corrected item 6 for the `the other 16` it quotes from its own
/// pre-fix text; that is the gate's false positive, not the prose's defect.
fn strip_quoted(flat_text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in flat_text.chars() {
        if c == '"' {
            inside = !inside;
            continue;
        }
        if !inside {
            out.push(c);
        }
    }
    out
}

/// A typed roster size — `NN contracts` or `the other NN` — which is exactly
/// the shape that drifted.
fn roster_size_sites(flat_text: &str) -> Vec<String> {
    let unquoted = strip_quoted(flat_text);
    let mut hits = Vec::new();
    let words: Vec<&str> = unquoted.split(' ').collect();
    for w in words.windows(2) {
        let (a, b) = (w[0], w[1]);
        let num = a.trim_matches(|c: char| !c.is_ascii_digit());
        if num.len() >= 2 && num.chars().all(|c| c.is_ascii_digit()) {
            let bl = b
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            if bl == "contracts" {
                hits.push(format!("{a} {b}"));
            }
        }
        if a.eq_ignore_ascii_case("other")
            && b.trim_matches(|c: char| !c.is_ascii_digit()).len() >= 2
        {
            hits.push(format!("{a} {b}"));
        }
    }
    hits
}

// ── Rule F: item 7 publishes its command, not its result ───────────────────

/// A lane roster is a run of `N (M)` pairs — `shell 10 (7), rust-differential
/// 47 (44), hybrid 7 (3)`. Three or more on one entry is a typed manifest.
fn lane_roster_sites(text: &str) -> Vec<String> {
    let f = flat(text);
    let bytes: Vec<char> = f.chars().collect();
    let mut hits = Vec::new();
    for (i, c) in bytes.iter().enumerate() {
        if *c != '(' || i == 0 {
            continue;
        }
        // digit, space, '('
        let before = bytes[..i].iter().rev().find(|c| **c != ' ').copied();
        if !matches!(before, Some(d) if d.is_ascii_digit()) {
            continue;
        }
        let inner: String = bytes[i + 1..]
            .iter()
            .take_while(|c| **c != ')')
            .collect::<String>();
        if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
            hits.push(format!("… {inner} )"));
        }
    }
    hits
}

#[test]
fn known_divergences_item_7_publishes_the_command_not_the_counts() {
    let item = divergence_item(7);
    assert!(
        flat(&item).contains("cargo test -p xpile --test witness_floor"),
        "item 7 must publish the command that derives the witness-floor manifest:\n{item}"
    );
    let roster = lane_roster_sites(&item);
    assert!(
        roster.len() < 3,
        "item 7 types a live witness-count roster again ({} `N (M)` sites: {roster:?}). Those \
         counts drift with every slice that adds a witness — four of the nine typed on 2026-07-27 \
         were wrong two days later — and this entry's own closing sentence forbids quoting \
         snapshot counts for that exact reason. Publish the command instead.",
        roster.len()
    );
}

// ── Rule G: the detectors are not decorative ───────────────────────────────

/// The PRE-FIX text of both entries, verbatim. A detector that does not fire on
/// the defect it was written for is a green test that checks nothing.
#[test]
fn the_detectors_fire_on_the_pre_fix_text() {
    let pre_fix_item_1 = "**1. i64 arithmetic overflow is still a SILENT WRONG ANSWER in the WASM \
         lane.** Python integers are arbitrary precision; the lane maps them to `i64` and wraps\n\
         without a word. `9223372036854775807 + 1` returns `-9223372036854775808`;\n\
         `3037000500 * 3037000500` returns `-9223372036709301616` … the remaining fix is \
         overflow-checked `add`/`sub`/`mul`/`neg` across every i64 operation.";
    let fired = retired_claims_present(pre_fix_item_1);
    assert_eq!(
        fired.len(),
        RETIRED_ITEM_1_CLAIMS.len(),
        "rule D's detector missed part of the pre-fix item 1. Fired on {fired:?}, expected all of \
         {RETIRED_ITEM_1_CLAIMS:?} — note `wraps without a word` falls across a LINE BREAK in the \
         real file, which is why matching is whitespace-collapsed."
    );

    let pre_fix_item_7 = "wasm 842 (floor 770) of which 333 are runtime-gated (floor 300, 39% vs \
         a 36% floor), shell 10 (7), rust-differential 47 (44), hybrid 7 (3), wasi 1 (1), ruchy \
         7 (7), forjar 4 (4), lean 6 (6).";
    assert!(
        lane_roster_sites(pre_fix_item_7).len() >= 3,
        "rule F's roster detector does not fire on the pre-fix item 7 roster it was written for: \
         {:?}",
        lane_roster_sites(pre_fix_item_7)
    );

    let pre_fix_item_6 = "so of the 18 contracts carrying a Runtime vote, exactly two are backed \
         by executing witnesses; the other 16 carry between 1 and 11 votes";
    assert!(
        roster_size_sites(pre_fix_item_6).len() >= 2,
        "rule E's roster-size detector does not fire on the pre-fix item 6 text: {:?}",
        roster_size_sites(pre_fix_item_6)
    );

    // AND THE FALSE-POSITIVE CONTROL. The corrected entries must NOT trip their
    // own detectors — otherwise the rules above are unsatisfiable and the next
    // author deletes them rather than the defect.
    assert!(
        lane_roster_sites(&divergence_item(7)).len() < 3,
        "the corrected item 7 trips rule F"
    );
    assert!(
        roster_size_sites(&flat(&divergence_item(6))).is_empty(),
        "the corrected item 6 trips rule E: {:?}",
        roster_size_sites(&flat(&divergence_item(6)))
    );

    // THE QUOTATION EXEMPTION, pinned in BOTH directions — an exemption that
    // swallows the rule is worse than no rule. The same sentence fires when it
    // ASSERTS the count and is exempt when it QUOTES it.
    let asserted = "so of the 18 contracts carrying a Runtime vote … the other 16 carry mentions";
    let quoted = "this entry typed \"the 18 contracts carrying a Runtime vote … the other 16\" \
                  on 2026-07-27 and the live figure was 14";
    assert!(
        !roster_size_sites(asserted).is_empty(),
        "the quotation exemption swallowed an ASSERTED roster size"
    );
    assert!(
        roster_size_sites(quoted).is_empty(),
        "a disclosed quotation is still being flagged: {:?}",
        roster_size_sites(quoted)
    );
}

// ── Rule H: the section header describes what it actually covers ──────────

#[test]
fn the_section_header_distinguishes_behavioural_from_metric_entries() {
    let sec = known_divergences();
    let head = flat(sec.split("**1.").next().unwrap_or(""));
    assert!(
        !head.contains("Each item below was reproduced against live `python3`"),
        "the section header claims every entry was reproduced through a `python3` differential. \
         Items 6 and 7 are METRIC claims that no `python3` run can check — and they are precisely \
         the two nobody re-measured for two days. Say which entries are behavioural.\n{head}"
    );
    assert!(
        head.to_ascii_lowercase().contains("metric"),
        "the header must name the metric entries as a distinct kind:\n{head}"
    );
}
