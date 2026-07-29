//! XPILE-EUCLIDCLAIM-001 (PMAT-1468): the lowering this project REMOVED for
//! being wrong, still published as the lowering it ships.
//!
//! PMAT-538 (v0.1.237) replaced `checked_div_euclid` / `checked_rem_euclid` in
//! the Python `//` / `%` lowering with the TRUNCATING `checked_div` /
//! `checked_rem` plus a floor correction. The reason is arithmetic: Euclidean
//! division keeps a NON-NEGATIVE remainder, while Python's `%` takes the
//! DIVISOR's sign. They disagree on half the sign combinations —
//!
//! | expression | CPython | Euclidean | emitted |
//! |---|---|---|---|
//! | `7 % 3`    | `1`  | `1`  | `1`  |
//! | `7 % -3`   | `-2` | `1`  | `-2` |
//! | `-7 % 3`   | `2`  | `2`  | `2`  |
//! | `-7 % -3`  | `-1` | `2`  | `-1` |
//! | `-7 // -2` | `3`  | `4`  | `3`  |
//!
//! — and 20 live sites still named the Euclidean form as what xpile emits.
//! Among them: the `examples/README.md` that `cargo package -p xpile` UPLOADS
//! TO CRATES.IO, the example BINARY that prints the claim on stdout, the
//! published book's "Rust backend — what's emitted" page, `C-PY-INT-ARITH`'s
//! own `invariants:` and `postconditions:`, and BOTH Kani harnesses for the
//! two governing equations.
//!
//! ★ THE HARNESSES ARE THE SHARPEST OF THE TWENTY. `modulo_floor_semantics`
//! said *"Python `%` is FLOOR mod (sign matches divisor). Rust `rem_euclid`
//! matches"* and then proved `a.rem_euclid(b) >= 0`, calling that "the
//! load-bearing property". Those two sentences contradict each other — a value
//! that is always `>= 0` cannot take the sign of a negative divisor — so the
//! Symbolic-stratum evidence for the equation was a proof of the property that
//! FALSIFIES it, about a construct the emitter does not emit. A green harness
//! is not a discharged equation; check what the harness is quantified over.
//!
//! ★ THE REFUTATION WAS ALREADY IN THE REPO. `transpile_e2e.rs` has asserted
//! `!rust.contains("rem_euclid")` since PMAT-538. A test proving the claim
//! false ran on every CI cycle beside twenty sites making it.
//!
//! ★ AGED, NOT NEVER-TRUE — and that is why it survived. Every one of the 20
//! was TRUE when written; PMAT-538 changed the emitter and swept the three
//! comment blocks nearest the code it edited. This gate exists because prose
//! at a distance from a fix does not get swept by the fix.
//!
//! WHAT THIS GATE IS: three arms and their controls.
//!
//!  1. BEHAVIOUR — run the emitter. `//` and `%` over `int` must lower to
//!     `checked_div` / `checked_rem` plus a floor correction, on BOTH the Rust
//!     and Ruchy lanes, and must contain no Euclidean op. This is what makes
//!     the prose ban a DERIVED fact rather than a spelling list; if a future
//!     slice legitimately restores `div_euclid`, this arm reds FIRST and the
//!     prose rule is re-decided rather than silently enforced against truth.
//!
//!  2. SEMANTIC — the reason the ban exists, not just the ban. The emitted
//!     formula agrees with CPython on 4 of 4 sign combinations and the
//!     Euclidean formula on 2 of 4. Pinned CPython values, measured with
//!     `python3` on 2026-07-29.
//!
//!  3. PROSE — no live site may name the Euclidean form as the Python
//!     `//` / `%` lowering unless its own block cites `PMAT-538`, the slice
//!     that retired it. A positive, machine-checked marker (PMAT-1459's rule)
//!     rather than a stop-list of claim verbs: the live corpus contains
//!     sentences like *"`//` → `checked_div_euclid` (Python-floor, **not**
//!     C-truncating)"*, so a negation heuristic reads an offender as honest.
//!
//! SCOPE IS THE BLOCK, NOT THE LINE AND NOT THE PARAGRAPH. Measured both
//! wrong ways while writing this. LINE scoping fabricates offenders out of
//! wrapped doc comments whose citation sits one line up (PMAT-1466 hit the
//! same shape). PARAGRAPH scoping — a contiguous non-blank run — LAUNDERS:
//! Rust code has no blank line between a doc comment and the function body,
//! so `rust-codegen`'s false claim at the top of `emit_binop` was excused by a
//! `// PMAT-538:` comment forty lines below inside the body. The block is the
//! contiguous comment run / list item / table row, which is the unit these
//! claims are actually written in.
//!
//! OUT OF SUBJECT, DELIBERATELY, AND WHY:
//!
//!   * `CHANGELOG.md` and `docs/roadmaps/` — the historical ledger, which MUST
//!     quote the retired claim to record it. Same exemption PMAT-1458/1462 set.
//!   * `round(x, -n)`'s banker's-rounding helper genuinely emits `rem_euclid`
//!     on an i128 with a POSITIVE scale, where Euclidean and floor coincide.
//!     That is a correct live use, which is why the needle requires a Python
//!     `//` / `%` anchor in the same clause and does not ban the token.
//!   * Whether each repaired sentence is now the BEST description of the emit
//!     is a judgement no gate can make. This one checks the decidable half:
//!     that no sentence names a construct the emitter provably does not emit.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_backend::{BackendConfig, Profile, Target};
use xpile_frontend::Frontend;

const ENFORCEMENT: &str = "XPILE-EUCLIDCLAIM-001";
const MARKER: &str = "PMAT-538";

/// A Python module exercising both governed operators over `int`.
const FLOORDIV_MOD_PY: &str = "def q(a: int, b: int) -> int:\n    return a // b\n\
                               \ndef m(a: int, b: int) -> int:\n    return a % b\n";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

// ─────────────────────────────────────────────────────────────────────────
// Arm 1 — BEHAVIOUR: what the emitter actually emits.
// ─────────────────────────────────────────────────────────────────────────

/// Lower `FLOORDIV_MOD_PY` through `target` and return the emitted source.
fn emit(target: Target) -> String {
    let dir = std::env::temp_dir().join(format!("xpile-euclid-{:?}", target));
    std::fs::create_dir_all(&dir).expect("probe dir");
    let src = dir.join("probe.py");
    std::fs::write(&src, FLOORDIV_MOD_PY).expect("write probe");

    let module = PythonFrontend
        .parse_and_lower(&src, FLOORDIV_MOD_PY)
        .expect("probe lowers to meta-HIR");

    let session = xpile_core::default_session();
    let backend = session
        .backends
        .iter()
        .find(|b| b.targets().contains(&target))
        .unwrap_or_else(|| panic!("no backend for {target:?}"));
    let cfg = BackendConfig {
        emit_contracts: true,
        target,
        profile: Profile::RustOut,
        hardware: None,
    };
    backend
        .lower(&module, &cfg)
        .unwrap_or_else(|e| panic!("{target:?} must lower `//` and `%`: {e}"))
        .primary
}

/// The claim under audit, decided by running the emitter rather than by
/// reading anything. Both lanes, because both lanes' prose made the claim.
#[test]
fn the_python_floordiv_and_mod_emit_no_euclidean_op() {
    for target in [Target::Rust, Target::Ruchy] {
        let out = emit(target);

        for banned in ["div_euclid", "rem_euclid"] {
            assert!(
                !out.contains(banned),
                "{ENFORCEMENT}: {target:?} emitted `{banned}` for Python `//`/`%`. \
                 PMAT-538 removed it in v0.1.237 because Euclidean division keeps a \
                 NON-NEGATIVE remainder while Python's takes the DIVISOR's sign. If \
                 this is a deliberate restoration, revisit this gate and the prose \
                 rule below it BEFORE landing — do not silence this arm.\n{out}"
            );
        }

        // The positive half. Without it, a backend that stopped emitting the
        // operators at all — or a refusal — would satisfy the assertion above
        // for entirely the wrong reason (PMAT-1385's vacuity shape).
        for required in ["checked_div", "checked_rem"] {
            assert!(
                out.contains(required),
                "{ENFORCEMENT}: {target:?} emitted neither `{required}` nor an \
                 Euclidean op — the probe is measuring nothing.\n{out}"
            );
        }
        assert!(
            out.contains("__q - 1") && out.contains("__r + __fb"),
            "{ENFORCEMENT}: {target:?} emitted no floor correction. The truncating \
             op ALONE diverges from Python exactly as `rem_euclid` does; the \
             correction is the whole reason this lowering is right.\n{out}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Arm 2 — SEMANTIC: why the ban exists.
// ─────────────────────────────────────────────────────────────────────────

/// `(a, b, a // b, a % b)` under CPython. Measured with `python3` on
/// 2026-07-29, not derived from any formula in this repo.
const CPYTHON: [(i64, i64, i64, i64); 4] = [
    (7, 3, 2, 1),
    (7, -3, -3, -2),
    (-7, 3, -3, 2),
    (-7, -3, 2, -1),
];

/// The lowering the emitter produces, transcribed from `emit_floor_div` /
/// `emit_floor_mod`.
fn emitted_floordiv_mod(a: i64, b: i64) -> (i64, i64) {
    let q0 = a / b;
    let r0 = a % b;
    let corrected = r0 != 0 && (r0 < 0) != (b < 0);
    (
        if corrected { q0 - 1 } else { q0 },
        if corrected { r0 + b } else { r0 },
    )
}

/// The gate carries the REASON, not only the rule. A future reader who wants
/// to know why `rem_euclid` is banned gets the counterexample, not a citation.
#[test]
fn the_emitted_lowering_agrees_with_cpython_where_euclidean_does_not() {
    let mut euclid_disagreements = Vec::new();

    for (a, b, py_q, py_r) in CPYTHON {
        let (q, r) = emitted_floordiv_mod(a, b);
        assert_eq!(
            (q, r),
            (py_q, py_r),
            "{ENFORCEMENT}: the emitted lowering disagrees with CPython on \
             {a} // {b} and {a} % {b}"
        );
        if (a.div_euclid(b), a.rem_euclid(b)) != (py_q, py_r) {
            euclid_disagreements.push((a, b));
        }
    }

    assert_eq!(
        euclid_disagreements.len(),
        2,
        "{ENFORCEMENT}: expected the Euclidean form to disagree with CPython on \
         exactly the two NEGATIVE-divisor rows, got {euclid_disagreements:?}. If \
         this changed, the premise of the prose ban changed with it."
    );
    for (_, b) in &euclid_disagreements {
        assert!(
            *b < 0,
            "{ENFORCEMENT}: Euclidean disagreement at a POSITIVE divisor ({b}) — \
             the documented reason for the ban is wrong, not just the prose."
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Arm 3 — PROSE: the derived corpus, the block scope, the needle.
// ─────────────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Blank,
    /// A run of `///` / `//!` doc lines, or of `//` / `#` comment lines.
    Comment,
    /// A markdown/YAML list item, a table row, or a markdown heading.
    Item,
    Text,
}

/// `#` is a HEADING in markdown but a COMMENT-BLOCK line in YAML and Rust.
/// Getting this backwards left four of this slice's own repairs reported as
/// offenders, because each `#` line became its own scope and the citation
/// three lines up was invisible.
fn line_kind(line: &str, markdown: bool) -> Kind {
    let t = line.trim_start();
    if t.trim_end().is_empty() {
        Kind::Blank
    } else if t.starts_with("//") {
        Kind::Comment
    } else if t.starts_with('#') {
        if markdown { Kind::Item } else { Kind::Comment }
    } else if t.starts_with("- ") || t.starts_with("* ") || t.starts_with('|') {
        Kind::Item
    } else {
        Kind::Text
    }
}

/// The block containing line `i`: a contiguous comment run, a list item or
/// table row with its continuation lines, or a plain-text paragraph.
fn block_at(lines: &[&str], i: usize, markdown: bool) -> String {
    let kind = |j: usize| line_kind(lines[j], markdown);
    let (mut lo, mut hi) = (i, i);
    match kind(i) {
        Kind::Comment => {
            while lo > 0 && kind(lo - 1) == Kind::Comment {
                lo -= 1;
            }
            while hi + 1 < lines.len() && kind(hi + 1) == Kind::Comment {
                hi += 1;
            }
        }
        Kind::Item => {
            while hi + 1 < lines.len() && kind(hi + 1) == Kind::Text {
                hi += 1;
            }
        }
        _ => {
            while lo > 0 && kind(lo - 1) == Kind::Text {
                lo -= 1;
            }
            while hi + 1 < lines.len() && kind(hi + 1) == Kind::Text {
                hi += 1;
            }
            if lo > 0 && kind(lo - 1) == Kind::Item {
                lo -= 1;
            }
        }
    }
    lines[lo..=hi].join("\n")
}

fn names_euclidean_op(s: &str) -> bool {
    s.contains("div_euclid") || s.contains("rem_euclid")
}

/// The subject anchor: the clause must be about the PYTHON floor-div / mod
/// lowering. Without it, `C-C-FLOAT-ARITH`'s honest *"never `wrapping_*` or
/// `div_euclid`"* — a denial about a different lane, matched only via the `%`
/// in its operator list — is reported as an offender.
fn is_about_python_floordiv_or_mod(clause: &str) -> bool {
    let lower = clause.to_ascii_lowercase();
    ["python", "floor", "//", "floordiv", "modulo"]
        .iter()
        .any(|n| lower.contains(n))
        || clause.contains("Mod")
}

fn clauses(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        match rest
            .char_indices()
            .find(|(i, c)| {
                (*c == '.' || *c == ';')
                    && rest[i + c.len_utf8()..].starts_with(char::is_whitespace)
            })
            .map(|(i, c)| i + c.len_utf8())
        {
            Some(cut) => {
                out.push(&rest[..cut]);
                rest = &rest[cut..];
            }
            None => {
                out.push(rest);
                return out;
            }
        }
    }
}

/// Every tracked file that publishes normative prose about the emitter.
/// Derived from `git ls-files` so a new spec page or contract is in subject
/// by EXISTING, not by being remembered here.
fn corpus() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files"])
        .current_dir(workspace_root())
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "{ENFORCEMENT}: git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .filter(|f| {
            // The historical ledger MUST be able to quote the retired claim.
            !f.starts_with("docs/roadmaps/")
                && f != "CHANGELOG.md"
                // This gate quotes it too, in the header above.
                && !f.ends_with("euclid_lowering_claim_witness.rs")
                && ((f.starts_with("crates/") && f.ends_with(".rs"))
                    || f.starts_with("contracts/")
                    || f.starts_with("book/src/")
                    || f.starts_with("docs/specifications/")
                    || f == "crates/xpile/examples/README.md")
        })
        .collect()
}

/// `(file, line, clause)` for every in-subject clause whose BLOCK does not
/// cite the slice that retired the construct.
fn scan(files: &[String]) -> (Vec<(String, usize, String)>, usize) {
    let root = workspace_root();
    let (mut offenders, mut cited) = (Vec::new(), 0usize);
    for f in files {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        if !names_euclidean_op(&text) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        let markdown = f.ends_with(".md");
        for (i, line) in lines.iter().enumerate() {
            let hit = clauses(line)
                .into_iter()
                .find(|c| names_euclidean_op(c) && is_about_python_floordiv_or_mod(c));
            let Some(clause) = hit else { continue };
            if block_at(&lines, i, markdown).contains(MARKER) {
                cited += 1;
            } else {
                offenders.push((f.clone(), i + 1, clause.trim().to_owned()));
            }
        }
    }
    (offenders, cited)
}

#[test]
fn no_live_site_claims_the_euclidean_lowering_of_python_floordiv_or_mod() {
    let files = corpus();
    let (offenders, cited) = scan(&files);

    assert!(
        offenders.is_empty(),
        "{ENFORCEMENT}: {} site(s) name `div_euclid`/`rem_euclid` as the Python \
         `//`/`%` lowering. The emitter has not used it since PMAT-538 \
         (v0.1.237) — `the_python_floordiv_and_mod_emit_no_euclidean_op` above \
         re-derives that on this tree. Either state the lowering that ships \
         (`checked_div`/`checked_rem` + a floor correction), or, if the sentence \
         exists to say the Euclidean form is WRONG, cite `{MARKER}` in the same \
         comment block / list item / table row.\n{}",
        offenders.len(),
        offenders
            .iter()
            .map(|(f, l, c)| format!("  {f}:{l}: {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // Anti-vacuity: the corpus really does discuss the construct, so a green
    // result means "no false claims", not "found nothing to read".
    assert!(
        cited >= 30,
        "{ENFORCEMENT}: only {cited} correctly-cited in-subject clause(s) — live \
         was 41 when this gate landed. A collapse means the needle or the corpus \
         stopped reaching the sites, not that the prose improved."
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Controls. Each pins one way this gate could be wrong.
// ─────────────────────────────────────────────────────────────────────────

/// SUBJECT control (PASSES). A gate's subject and its needle are two
/// independent blind spots; this one pins the subject. Every surface this
/// slice repaired must be reachable, including the file `cargo package -p
/// xpile` uploads to crates.io — which no README gate reads, because it is
/// not the crate's `readme =` front page (PMAT-1466).
#[test]
fn the_subject_reaches_the_surfaces_this_slice_repaired() {
    let files: BTreeSet<String> = corpus().into_iter().collect();
    for required in [
        "crates/xpile/examples/README.md",
        "crates/xpile/examples/03_python_to_ruchy.rs",
        "book/src/reference/backends.md",
        "contracts/py-int-arith-v1.yaml",
        "contracts/kani/py_int_arith.rs",
        "crates/xpile-rust-codegen/src/lib.rs",
        "crates/xpile-ruchy-codegen/src/lib.rs",
        "docs/specifications/sub/rust-codegen.md",
    ] {
        assert!(
            files.contains(required),
            "{ENFORCEMENT}: `{required}` is outside the corpus — the gate cannot \
             see a surface this slice found a false claim on."
        );
    }
}

/// NEEDLE control (PASSES). The exact sentence the book published, and the
/// exact table cell crates.io served. Both carry the word "not", which is why
/// this gate requires a POSITIVE marker instead of screening for negation.
#[test]
fn the_needle_reports_the_claims_this_slice_removed() {
    for published in [
        "  - `//` → `checked_div_euclid` (Python-floor, not C-truncating)",
        "| 03 | Python → Ruchy | run | GCD via `%` lowers to `.checked_rem_euclid()` \
         (Python-floor, not C-truncating) |",
        "//! and floor-div / modulo still go through Euclidean semantics (`div_euclid`).",
    ] {
        let hit = clauses(published)
            .into_iter()
            .any(|c| names_euclidean_op(c) && is_about_python_floordiv_or_mod(c));
        assert!(
            hit,
            "{ENFORCEMENT}: the needle does not see a claim this slice removed: \
             {published}"
        );
    }
}

/// NEEDLE control (PASSES, negative direction). `round(x, -n)` genuinely emits
/// `rem_euclid` on a POSITIVE i128 scale, where Euclidean and floor coincide.
/// A needle that banned the token would red a correct emitter.
#[test]
fn the_needle_ignores_a_euclid_mention_with_no_python_operator_anchor() {
    for honest in [
        "let __rm = __rv.rem_euclid(__rp);",
        "- \"emitted Rust uses f32 and plain IEEE infix `+ - * /` — never `div_euclid`\"",
    ] {
        let hit = clauses(honest)
            .into_iter()
            .any(|c| names_euclidean_op(c) && is_about_python_floordiv_or_mod(c));
        assert!(
            !hit,
            "{ENFORCEMENT}: the needle reported a clause with no Python \
             floor-div/mod anchor: {honest}"
        );
    }
}

/// SCOPE control (PASSES). The two ways I measured this wrong before settling
/// on the block, each pinned by a CONSTRUCTED arrangement — the corpus no
/// longer holds either up, since the repair removed every offender.
#[test]
fn the_marker_is_read_from_the_block_and_not_from_the_file_or_the_paragraph() {
    // (a) LINE scoping would fabricate an offender here: the citation is one
    //     line above, inside the same wrapped doc comment.
    let wrapped = ["/// PMAT-538: the emitter does not use", "/// `rem_euclid` for `%`."];
    assert!(
        block_at(&wrapped, 1, false).contains(MARKER),
        "{ENFORCEMENT}: a wrapped comment block must carry its own citation"
    );

    // (b) PARAGRAPH scoping (a contiguous non-blank run) LAUNDERS: in Rust
    //     there is no blank line between a doc comment and the body, so a
    //     citation deep inside the function excused a false claim in the doc.
    let laundered = [
        "/// FloorDiv preserves Python-floor semantics via `checked_div_euclid`.",
        "fn emit_binop() {",
        "    // PMAT-538: floor correction, not div_euclid.",
        "}",
    ];
    assert!(
        !block_at(&laundered, 0, false).contains(MARKER),
        "{ENFORCEMENT}: a citation in the function BODY must not excuse a false \
         claim in the doc comment above it"
    );

    // (c) Same file, different block, is not a citation either.
    let elsewhere = ["# PMAT-538 is discussed here.", "", "- \"`%` uses rem_euclid\""];
    assert!(
        !block_at(&elsewhere, 2, false).contains(MARKER),
        "{ENFORCEMENT}: a marker elsewhere in the file must not launder a claim"
    );
}

/// SCOPE control (PASSES). `#` is a heading in markdown and a comment in YAML.
/// Conflating them split every YAML comment block into single lines and
/// reported four of this slice's own repairs as offenders.
#[test]
fn a_hash_line_is_a_markdown_heading_but_a_yaml_comment_block() {
    let yaml = ["# PMAT-538 removed it:", "# `%` no longer uses rem_euclid."];
    assert!(
        block_at(&yaml, 1, false).contains(MARKER),
        "{ENFORCEMENT}: consecutive YAML `#` lines are ONE comment block"
    );

    let md = ["# PMAT-538", "## `%` and rem_euclid"];
    assert!(
        !block_at(&md, 1, true).contains(MARKER),
        "{ENFORCEMENT}: a markdown heading is its own scope, not a continuation \
         of the heading above it"
    );
}
