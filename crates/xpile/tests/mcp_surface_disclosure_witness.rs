//! XPILE-MCPDOC-001 — the MCP sub-spec may not publish a surface the crate does
//! not have, and may not defer to a milestone that has already closed
//! (PMAT-1499).
//!
//! ## What was wrong
//!
//! `docs/specifications/sub/mcp.md` published, in the present indicative: seven
//! MCP tools "exposed (v1)", a PMCP transport with stdio and TCP modes, three
//! `xpile mcp` invocations, a five-step server lifecycle, JSON-Schema argument
//! validation, a telemetry passthrough with an end-to-end session id, and a
//! **security guarantee** — *"only operates on files within the project root
//! … Arbitrary path arguments are rejected"*.
//!
//! Measured 2026-07-30 against tag `v0.1.618`: `crates/xpile-mcp/src/lib.rs` is
//! **19 lines** — one struct, one `String` field, one constructor. No tools, no
//! transport, no argument type, no path handling. `xpile` ships seven
//! subcommands and none is `mcp`; all three published invocations exit 2. The
//! page's one true sentence was its **last line**.
//!
//! ## Why every existing gate missed it
//!
//! `claims_drift.rs` walks all of `docs/specifications/` — this page was in the
//! strictest corpus the whole time. But its assertions hunt **derived
//! cardinalities** against the code, and this page's falsehoods were mostly
//! **prose in the present tense with no numeral to check**. Third axis of
//! PMAT-1495's *in scope ≠ covered*, after completeness and cadence.
//!
//! `cli_docs_drift.rs` executes the binary and could have caught `xpile mcp` —
//! but its corpus is `book/src/reference/cli.md`, one file, and PMAT-1498 had
//! just widened the same class onto `sub/cli.md` without reaching either the
//! **parent** `xpile-spec.md` §14 block (measured 1-of-8 true) or this page.
//!
//! ## The two shapes this file exists to stop
//!
//! **1. A stub whose doc outruns it.** The assertions below are keyed to the
//! CRATE, not to prose, so they flip direction when the implementation lands:
//! the day `xpile mcp` becomes a registered subcommand, or a planned tool name
//! appears in `crates/xpile-mcp/`, or a `pmcp` dependency is declared, this
//! gate REDS and demands the "does not run today" framing be retired. A
//! disclosure that cannot expire becomes the next stale claim.
//!
//! **2. A deferral pointing at a milestone that already closed.** The page
//! ended *"Real MCP wiring lands in Phase 4."* Phase 4 is the **Kani** phase,
//! and `sub/phased-rollout.md` records it as shipped. The deferral had expired
//! and nothing read it. A deferral to a named milestone is a claim with an
//! expiry date, so `no_expired_phase_deferral` re-derives the phase's status
//! from the rollout page rather than trusting either document's prose.
//!
//! Every needle here carries a CONTROL that must fire, because a screen nobody
//! has seen match is a screen that might match nothing at all.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

const MCP_PAGE: &str = "docs/specifications/sub/mcp.md";
const MCP_LIB: &str = "crates/xpile-mcp/src/lib.rs";
const MCP_MANIFEST: &str = "crates/xpile-mcp/Cargo.toml";
const ROLLOUT: &str = "docs/specifications/sub/phased-rollout.md";
const PARENT_SPEC: &str = "docs/specifications/xpile-spec.md";

/// The seven tools the sub-spec publishes as PLANNED. Kept here so that a tool
/// appearing in the crate reds `planned_tools_are_absent_from_the_crate`.
const PLANNED_TOOLS: &[&str] = &[
    "transpile_file",
    "transpile_hybrid",
    "inspect_meta_hir",
    "inspect_ffi_manifest",
    "lint_contracts",
    "score_contracts",
    "query_contracts",
];

/// Lines that are QUOTED prose — a page that corrects itself has to be able to
/// quote what it used to say. Bounded to block quotes and fenced blocks, the
/// same exemption doctrine as `enforcement_prose_witness.rs`.
fn unquoted_lines(body: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (i, line) in body.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || t.starts_with('>') {
            continue;
        }
        out.push((i + 1, line));
    }
    out
}

/// Strip inline-code spans. A page that records a corrected numeral has to be
/// able to name it; PMAT-1498's lesson is that an honesty fix writes new claims,
/// so the historical figure goes in backticks and this strips it before the
/// cardinality screen runs. Anything OUTSIDE backticks is the page's own voice.
fn strip_inline_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_code = false;
    for c in s.chars() {
        if c == '`' {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            out.push(c);
        }
    }
    out
}

/// Subcommands the shipped binary actually registers, parsed from `--help`.
fn registered_subcommands() -> BTreeSet<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .arg("--help")
        .output()
        .expect("run xpile --help");
    assert!(out.status.success(), "xpile --help must exit 0");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut in_commands = false;
    let mut set = BTreeSet::new();
    for line in text.lines() {
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            if line.starts_with("Options:") {
                break;
            }
            // clap indents each command by exactly two spaces; wrapped help text
            // is indented further.
            if line.len() - line.trim_start().len() == 2 {
                if let Some(name) = line.split_whitespace().next() {
                    set.insert(name.to_string());
                }
            }
        }
    }
    assert!(
        set.contains("transpile") && set.contains("hybrid"),
        "parse of `xpile --help` Commands: block failed — got {set:?}. This gate \
         cannot pass vacuously."
    );
    set
}

/// PROPERTY 1 — the crate has no path confinement, so the page may not state
/// confinement as a property. Keyed to the CRATE: if path handling lands, the
/// needle is allowed again.
#[test]
fn no_unqualified_path_confinement_guarantee() {
    let lib = read(MCP_LIB);
    let has_path_handling = ["Path", "canonicalize", "strip_prefix", "starts_with"]
        .iter()
        .any(|n| lib.contains(n));
    assert!(
        !has_path_handling,
        "{MCP_LIB} now contains path-handling identifiers. The security posture in \
         {MCP_PAGE} may be upgraded from a REQUIREMENT to a property — but only \
         with its own falsification test. Update this gate deliberately."
    );

    // With no path handling in the crate, these sentences may not appear as the
    // page's own voice.
    const CONFINEMENT_CLAIMS: &[&str] = &[
        "arbitrary path arguments are rejected",
        "only operates on files within the project root",
    ];
    let body = read(MCP_PAGE);
    let lowered_lines: Vec<(usize, String)> = unquoted_lines(&body)
        .into_iter()
        .map(|(n, l)| (n, l.to_lowercase()))
        .collect();
    for needle in CONFINEMENT_CLAIMS {
        for (n, line) in &lowered_lines {
            assert!(
                !line.contains(needle),
                "{MCP_PAGE}:{n} states path confinement as a property — \"{needle}\" \
                 — but {MCP_LIB} has no path handling at all. State it as a \
                 REQUIREMENT on the implementation, or quote it as a past claim.\n\
                 line: {line}"
            );
        }
    }

    // CONTROL: the needles must match the quoted historical text, or they have
    // drifted from the sentences they exist to screen.
    let quoted = body.to_lowercase();
    for needle in CONFINEMENT_CLAIMS {
        assert!(
            quoted.contains(needle),
            "CONTROL FAILED: needle \"{needle}\" matches nothing in {MCP_PAGE}, not \
             even the block-quoted record of the old claim. The screen is dead."
        );
    }
}

/// PROPERTY 2 — `mcp` is not a registered subcommand, so the page must say so;
/// and the day it IS registered, the "does not run today" framing must go.
#[test]
fn mcp_subcommand_status_matches_the_binary() {
    let registered = registered_subcommands();
    let body = read(MCP_PAGE);
    let planned_markers = [
        "not a subcommand",
        "does not run today",
        "unrecognized subcommand 'mcp'",
    ];
    let present: Vec<&str> = planned_markers
        .iter()
        .copied()
        .filter(|m| body.to_lowercase().contains(&m.to_lowercase()))
        .collect();

    if registered.contains("mcp") {
        assert!(
            present.is_empty(),
            "`xpile mcp` is NOW a registered subcommand, but {MCP_PAGE} still \
             publishes the not-implemented framing {present:?}. The disclosure has \
             expired — retire it and document the shipped surface."
        );
    } else {
        assert!(
            !present.is_empty(),
            "`xpile mcp` is not registered (live subcommands: {registered:?}) and \
             {MCP_PAGE} carries none of the planned-surface markers \
             {planned_markers:?}. A reader is told to run a command that exits 2."
        );
    }
}

/// PROPERTY 3 — the planned tool roster is absent from the crate. When a tool
/// lands, the page must stop calling it planned.
#[test]
fn planned_tools_are_absent_from_the_crate() {
    let lib = read(MCP_LIB);
    let manifest = read(MCP_MANIFEST);
    let landed: Vec<&str> = PLANNED_TOOLS
        .iter()
        .copied()
        .filter(|t| lib.contains(t))
        .collect();
    assert!(
        landed.is_empty(),
        "{MCP_LIB} now names planned MCP tool(s) {landed:?}. {MCP_PAGE} lists them \
         under \"Planned surface — does not run today\" and must be corrected."
    );
    assert!(
        !manifest.contains("pmcp"),
        "{MCP_MANIFEST} now declares a `pmcp` dependency. {MCP_PAGE} says the \
         transport is \"Not wired\" and that the crate declares no `pmcp` \
         dependency — both statements have expired."
    );

    // CONTROL: the roster must still be the page's roster. If a name here stops
    // appearing in the doc, the two have drifted and the screen is partly dead.
    let page = read(MCP_PAGE);
    for t in PLANNED_TOOLS {
        assert!(
            page.contains(t),
            "CONTROL FAILED: planned tool `{t}` is screened by this gate but no \
             longer appears in {MCP_PAGE}. Roster and page have drifted."
        );
    }
}

/// PROPERTY 4 — the parent/child numeral cross-check. No gate in this repo
/// compared a parent section against its own sub-spec, and the two published
/// six tools and seven tools respectively.
#[test]
fn parent_section_does_not_contradict_the_sub_spec_tool_count() {
    let page = read(MCP_PAGE);
    // Count the rows of the planned-tool table: each row names one tool.
    let rows = PLANNED_TOOLS
        .iter()
        .filter(|t| page.contains(&format!("`{t}(")))
        .count();
    assert_eq!(
        rows,
        PLANNED_TOOLS.len(),
        "{MCP_PAGE} publishes {rows} planned-tool table rows; this gate's roster \
         has {}. Reconcile them.",
        PLANNED_TOOLS.len()
    );

    const WORDS: &[(&str, usize)] = &[
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
    ];
    let parent = read(PARENT_SPEC);
    let section = parent
        .split("## 15. MCP Server")
        .nth(1)
        .expect("xpile-spec.md must have a `## 15. MCP Server` section")
        .split("\n## ")
        .next()
        .expect("section body");
    let lowered = strip_inline_code(section).to_lowercase();
    for (w, n) in WORDS {
        for pat in [format!("{w} initial tools"), format!("{w} tools")] {
            if lowered.contains(&pat) {
                assert_eq!(
                    *n, rows,
                    "xpile-spec.md §15 publishes \"{pat}\" while {MCP_PAGE} lists \
                     {rows} planned tools. A parent section and its own sub-spec \
                     disagree on a cardinality, in the corpus every claim gate \
                     walks."
                );
            }
        }
    }
    for d in 0..=9usize {
        let pat = format!("{d} tools");
        if lowered.contains(&pat) {
            assert_eq!(
                d, rows,
                "xpile-spec.md §15 publishes \"{pat}\" against {rows} planned tools \
                 in {MCP_PAGE}."
            );
        }
    }
}

/// PROPERTY 5 — the generalizable shape: a deferral to a NAMED phase must not
/// point at a phase the rollout page records as shipped.
#[test]
fn no_expired_phase_deferral() {
    let rollout = read(ROLLOUT);

    // A phase the rollout page reports an `Actual:` outcome for has HAPPENED.
    let mut shipped: BTreeSet<usize> = BTreeSet::new();
    for line in rollout.lines() {
        let t = line.trim();
        if !t.contains("Actual:") {
            continue;
        }
        if let Some(rest) = t.split("Phase ").nth(1) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<usize>() {
                shipped.insert(n);
            }
        }
    }
    // CONTROL: Phase 4 (Kani) is the instance that motivated this gate. If the
    // parse stops finding it, the screen is dead.
    assert!(
        shipped.contains(&4),
        "CONTROL FAILED: could not re-derive Phase 4 as shipped from {ROLLOUT} \
         (parsed shipped phases: {shipped:?}). This gate cannot pass vacuously."
    );

    // POSITIVE CONTROL — run the RED half. The live page's only mention of the
    // retired deferral is inline-code-exempt, so without this the screen could
    // match nothing and still report green.
    let synthetic = "Real MCP wiring lands in Phase 4.";
    assert_eq!(
        expired_deferral(synthetic, &shipped),
        Some(4),
        "CONTROL FAILED: the detector does not fire on the exact sentence this \
         gate exists to catch — {synthetic:?}. The screen is dead."
    );
    // NEGATIVE CONTROL — a deferral to a phase with no `Actual:` row is legal.
    let unshipped = (1..=20usize).find(|n| !shipped.contains(n));
    if let Some(u) = unshipped {
        let ok = format!("MCP wiring lands in Phase {u}.");
        assert_eq!(
            expired_deferral(&ok, &shipped),
            None,
            "CONTROL FAILED: the detector fires on a deferral to Phase {u}, which \
             {ROLLOUT} does NOT record as shipped. It would forbid honest planning."
        );
    }

    let page = read(MCP_PAGE);
    for (n, line) in unquoted_lines(&page) {
        if let Some(phase) = expired_deferral(line, &shipped) {
            panic!(
                "{MCP_PAGE}:{n} defers work to Phase {phase}, which {ROLLOUT} \
                 records as SHIPPED (`Actual:`). The deferral has expired — it \
                 points at a date in its own past.\nline: {line}"
            );
        }
    }
}

/// Does `line` defer work, in the page's own voice, to a phase that has already
/// shipped? Returns the offending phase number.
///
/// Inline code is the page QUOTING a retired deferral, not making one. Without
/// that exemption the sentence recording the fix reds as the defect —
/// PMAT-1495's trap, where correcting reader-facing prose revoked the exemption
/// token and the gate read the repair as the regression.
fn expired_deferral(line: &str, shipped: &BTreeSet<usize>) -> Option<usize> {
    let lowered = strip_inline_code(line).to_lowercase();
    let defers = ["lands in phase", "will land in phase", "ships in phase"]
        .iter()
        .any(|p| lowered.contains(p));
    if !defers {
        return None;
    }
    shipped
        .iter()
        .copied()
        .find(|p| lowered.contains(&format!("phase {p}")))
}
