//! XPILE-ENFORCE-PROSE-001 — no document may under-claim what blocks a merge
//! (PMAT-1475).
//!
//! `ruleset_drift.rs` pins the machine-readable half: the committed
//! `docs/status/ruleset-*.json` receipts, the `XPILE-ENFORCEMENT` marker lines,
//! and the live branch-rules endpoint. This file pins the PROSE half, which is
//! what a reader actually reads and which drifted in the opposite direction
//! from every other honesty defect this release.
//!
//! ## Why the prose needs its own gate
//!
//! On 2026-07-27 `workspace-test` was **moved** from org ruleset `13878864`
//! into a new ruleset `19814559`. Effective protection on `main` did not
//! change. But a per-id check reported a weakening, and over the next two days
//! three documents were edited to AGREE with it — including
//! `contracts/README.md`, which is packaged and published to crates.io, and an
//! `[Unreleased]` CHANGELOG entry that was about to ship as release notes.
//!
//! Every one of those edits passed CI. `ruleset_drift.rs` could not see them:
//! its subject is markers and JSON, and none of the three sentences was a
//! marker. So the repo told its readers it enforced less than it did, in files
//! no enforcement gate was quantified over.
//!
//! **The direction is the point.** A sweep that only hunts over-claiming will
//! rewrite a TRUE sentence into a false one and call it a correction.
//!
//! ## What this asserts
//!
//! Over a DISCOVERED corpus — every tracked Markdown file, via `git ls-files`,
//! so a new document is covered the day it lands — no file may state that a
//! context recorded as required is not enforced.
//!
//! Quoted spans are exempt: an entry that corrects itself has to be able to
//! quote what it used to say. That exemption is itself a hole, so it is
//! bounded — inline code and block quotes only — and both the needle and the
//! exemption carry a control that must PASS, because a screen nobody has seen
//! fire is a screen that might match nothing at all.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Sentences that were LIVE and FALSE between 2026-07-27 and 2026-07-29. Each
/// asserts that a required context is not enforced. Matched case-insensitively
/// against text with quoted spans removed.
const UNDER_CLAIMS: &[&str] = &[
    // contracts/README.md — packaged, shipped to crates.io.
    "live org ruleset is currently weaker",
    "weaker than that record",
    // The generic shape: any claim that the required set is `gate` alone.
    "requires only gate",
    "required set is gate alone",
    "only gate blocks",
    "workspace-test was dropped",
    "workspace-test is no longer required",
    "workspace-test no longer blocks",
];

/// How far past a match to look for the word that would make it honest.
const ACQUITTAL_WINDOW: usize = 48;

/// Remove the spans a document is allowed to quote, and ONLY those.
///
/// Disclosed quotation means a **block quote** (`> …`): an entry that corrects
/// itself has to be able to restate what it used to say. Inline code is NOT a
/// quotation — it is formatting, and treating it as one is a hole wide enough
/// to drive the original defect through. The first draft of this gate blanked
/// backticked spans, and `docs/RELEASE.md`'s live-and-false
/// "the live ruleset requires only `gate`" went UNDETECTED because the one word
/// that made it a claim sat inside backticks. So backticks are stripped as
/// *characters* while their contents stay in the haystack. This is the same
/// needle-blindness that let PMAT-1449's gate miss `contracts/README.md`.
fn strip_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.trim_start().starts_with('>') {
            out.push('\n');
            continue;
        }
        // A span inside double quotes is also disclosed quotation — that is how
        // a prose paragraph cites the wording it is correcting when a block
        // quote would break the sentence.
        let mut in_quote = false;
        for ch in line.chars() {
            match ch {
                '`' => {}
                '"' => {
                    in_quote = !in_quote;
                    out.push(' ');
                }
                _ => out.push(if in_quote { ' ' } else { ch }),
            }
        }
        out.push('\n');
    }
    out
}

/// Every tracked Markdown file. Discovered, not listed — the corpus is the
/// blind spot that let the packaged `contracts/README.md` sit outside
/// PMAT-1449's two-file corpus.
fn tracked_markdown() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "*.md"])
        .current_dir(workspace_root())
        .output()
        .expect("`git ls-files` runs");
    assert!(out.status.success(), "`git ls-files` failed");
    let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    assert!(
        files.len() >= 20,
        "discovered only {} tracked .md files — the corpus query is broken, not \
         the repo",
        files.len()
    );
    files
}

/// A needle is an offence only if `workspace-test` does not appear right after
/// it. "requires only gate **and workspace-test**" is an honest sentence that
/// happens to contain the words of a dishonest one, and the first draft of this
/// gate flagged two of them — including a correct line in a shipped release
/// section. A screen that cries wolf gets loosened by the next person to hit it.
fn offences_in(text: &str) -> BTreeSet<String> {
    let hay = strip_quoted(text).to_lowercase();
    let mut found = BTreeSet::new();
    for needle in UNDER_CLAIMS {
        let lower = needle.to_lowercase();
        let mut from = 0;
        while let Some(rel) = hay[from..].find(&lower) {
            let at = from + rel;
            let end = at + lower.len();
            let window = &hay[end..hay.len().min(end + ACQUITTAL_WINDOW)];
            if !window.contains("workspace-test") {
                found.insert((*needle).to_string());
                break;
            }
            from = end;
        }
    }
    found
}

/// No tracked document may claim the repo enforces less than its receipts record.
#[test]
fn no_document_under_claims_the_merge_blocking_set() {
    let root = workspace_root();
    let mut offences = Vec::new();
    for rel in tracked_markdown() {
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for needle in offences_in(&text) {
            offences.push(format!("{rel}: \"{needle}\""));
        }
    }
    assert!(
        offences.is_empty(),
        "document(s) claim a required context is not enforced:\n  {}\n\n\
         Before changing anything, run `gh api repos/paiml/xpile/rules/branches/main`. \
         That is the UNION over every ruleset protecting main, and it is the only \
         thing that answers \"what blocks a merge\". A single ruleset losing a \
         context is not evidence the context stopped being required — on \
         2026-07-27 `workspace-test` MOVED to ruleset 19814559 and three \
         documents were edited to claim less enforcement than the repo has. \
         If the enforcement really did weaken, re-derive the receipts under \
         docs/status/ first; the prose follows the receipts, never the reverse.",
        offences.join("\n  ")
    );
}

/// The needle works. Without this, a typo in `UNDER_CLAIMS` turns the gate above
/// into a test that reads every Markdown file and asserts nothing.
#[test]
fn the_under_claim_screen_actually_fires() {
    let live_and_false =
        "Do not read the merge-blocking set here. The live org ruleset is currently \
         weaker than that record, and the gap is an open owner decision.";
    let found = offences_in(live_and_false);
    assert!(
        found.contains("live org ruleset is currently weaker"),
        "the screen did not fire on the exact sentence that shipped to crates.io \
         in v0.1.617's contracts/README.md: {found:?}"
    );

    // Two-way control: honest prose must NOT trip it.
    let honest = "The merge-blocking set is the union over every ruleset protecting \
                  main: gate and workspace-test.";
    assert!(
        offences_in(honest).is_empty(),
        "the screen fires on honest prose — it would force documents to be wrong"
    );

    // The acquittal window. This exact sentence ships in a released CHANGELOG
    // section and is TRUE; the first draft of this gate flagged it. A screen
    // that cries wolf on correct prose gets deleted by whoever hits it next.
    let true_but_similar = "the live org ruleset requires only `gate` and `workspace-test`";
    assert!(
        offences_in(true_but_similar).is_empty(),
        "the screen flagged a TRUE sentence — \"requires only gate AND \
         workspace-test\" names both required contexts: {:?}",
        offences_in(true_but_similar)
    );
}

/// The quotation exemption works, and is bounded. A CHANGELOG entry has to be
/// able to quote the falsehood it is correcting; it must not be able to launder
/// a live claim by indenting it.
#[test]
fn the_quotation_exemption_is_bounded() {
    // Disclosed quotation — exempt.
    let disclosed = "> The live org ruleset is currently weaker than that record.\n\n\
                     That sentence was false; the context had moved rulesets.";
    assert!(
        offences_in(disclosed).is_empty(),
        "a block-quoted disclosure was flagged — an entry could not describe its \
         own correction"
    );

    // Same words, asserted in the document's own voice — NOT exempt.
    let asserted = "The live org ruleset is currently weaker than that record.";
    assert!(
        !offences_in(asserted).is_empty(),
        "the exemption swallowed an unquoted claim — every offender could evade \
         this gate by never using a quote character"
    );

    // Inline code is formatting, not quotation. This is the exact sentence the
    // first draft of this gate let through in docs/RELEASE.md.
    let backticked = "As of 2026-07-27 it is RED — the live ruleset requires only \
                      `gate` while the committed snapshot records both.";
    assert!(
        !offences_in(backticked).is_empty(),
        "a claim survived by putting the load-bearing word in backticks — the \
         inline-code exemption is a hole, not a feature"
    );
}
