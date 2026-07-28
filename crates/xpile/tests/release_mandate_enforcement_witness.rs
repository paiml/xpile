//! XPILE-RELMANDATE-001 (PMAT-1449) — the release runbook told Thursday's
//! operator to publish a false statement about what CI enforces.
//!
//! THE DEFECT. Two work items mandate the content of the v0.1.618 release body.
//! Both required its "What is NOT merge-blocking" section to state, verbatim:
//!
//! > the live org ruleset requires only [gate, workspace-test]
//!
//! **It requires only `[gate]`.** Measured against the live API, not the
//! snapshot:
//!
//! ```text
//! $ gh api orgs/paiml/rulesets/13878864 | jq '.rules[] | select(.type=="required_status_checks")'
//!   required: ['gate']
//!   strict_required_status_checks_policy: False
//! $ jq … docs/status/ruleset-13878864.json
//!   required: ['gate', 'workspace-test']
//! ```
//!
//! So the mandate reproduced the **committed snapshot** and called it the
//! **live ruleset** — and it did so in the one section whose entire purpose is
//! to tell readers what is *not* enforced. **It overstated enforcement inside
//! the disclosure of non-enforcement**, which is the worst available direction
//! for that particular sentence: a reader is told `workspace-test` gates merges
//! when it does not, and `workspace-test` is the only job that runs the full
//! suite.
//!
//! THE REPO ALREADY CONTRADICTED ITSELF. `docs/RELEASE.md` has the live
//! position right — *"the live ruleset requires only `gate` while the committed
//! snapshot records `gate` + `workspace-test`, an open owner decision"* — so
//! the runbook and the release documentation disagreed, and the runbook is the
//! one that gets copied into the published note.
//!
//! WHY NO GATE SAW IT. `ruleset_drift.rs` covers the three places this list is
//! *machine-checkable*: the live API vs the snapshot (RED, by design — an open
//! owner decision), the `XPILE-ENFORCEMENT REQUIRED-CONTEXTS:` markers vs the
//! snapshot (green), and each context vs a real CI job (green). It does not,
//! and should not, read prose in `queue.yaml`. **A list that is derived in
//! three places and typed in a fourth will go stale in the fourth**, and the
//! fourth is the one with a publication date on it.
//!
//! THE FIX IS THE POINTER DOCTRINE, applied where it matters most. PMAT-1348
//! demoted `docs/status/CURRENT.md` to a pointer file — *"numbers in it must be
//! stated as the command that derives them, never typed inline"* — and gated
//! that for **that one file**. The release runbook is the same shape and had no
//! such rule, so the mandates now say *derive it from
//! `docs/status/ruleset-13878864.json` and name the open drift from
//! `docs/RELEASE.md`* rather than carrying a list. This is PMAT-1440's lesson
//! again: **a rule written against a FILE does not protect a CLAIM.**
//!
//! WHAT THIS FILE PINS.
//!
//! 1. No release-body mandate may hard-code the enforcement set. Keyed on what
//!    the DEFECT spelled — `requires only [` followed by a context list — with
//!    an exemption for a *quoted mention* of the old wording, because both
//!    corrections quote it in order to explain themselves (PMAT-1430: a doc
//!    gate must distinguish USE from MENTION).
//! 2. The destination the mandates now point at must EXIST and must disclose
//!    the drift, naming both sets. **A deferral with no destination is just an
//!    omission** (PMAT-1440).
//! 3. `docs/RELEASE.md`'s disclosure must name the set the SNAPSHOT actually
//!    records, derived from the JSON. That is the behaviour half: if the owner
//!    resolves the drift by re-deriving the snapshot, this reds and forces the
//!    disclosure to move with it — a caveat that outlives its defect is the
//!    same falsehood pointing the other way (PMAT-1411).
//!
//! NOT TOUCHED, deliberately: the `XPILE-ENFORCEMENT REQUIRED-CONTEXTS:`
//! markers in `ci.yml`, `CURRENT.md` and `enforcement-handoff.md` all read
//! `gate, workspace-test` and all **correctly** match the committed snapshot,
//! which `ruleset_drift::enforcement_markers_match_the_committed_snapshot`
//! holds green. Changing them would ratify the live weakening — the exact thing
//! `RELEASE.md` says not to do. The snapshot is the owner's record; only the
//! prose that mislabelled it as *live* was wrong.

use std::path::{Path, PathBuf};

const SNAPSHOT: &str = "docs/status/ruleset-13878864.json";
const RELEASE_MD: &str = "docs/RELEASE.md";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// The required contexts the COMMITTED snapshot records, parsed from the JSON.
/// Derived, never typed — that is the whole point of this file.
fn snapshot_contexts() -> Vec<String> {
    let body = read(SNAPSHOT);
    let v: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{SNAPSHOT} is not valid JSON: {e}"));
    let mut out = Vec::new();
    for rule in v
        .get("rules")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
    {
        if rule.get("type").and_then(|t| t.as_str()) != Some("required_status_checks") {
            continue;
        }
        for c in rule
            .pointer("/parameters/required_status_checks")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(ctx) = c.get("context").and_then(|c| c.as_str()) {
                out.push(ctx.to_string());
            }
        }
    }
    out.sort();
    assert!(
        !out.is_empty(),
        "{SNAPSHOT} records no required status checks — every assertion below would range over \
         nothing"
    );
    out
}

/// Blank-line-delimited paragraphs, flattened, as (start line, text).
fn paragraphs(body: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let (mut start, mut buf) = (1usize, Vec::new());
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            if !buf.is_empty() {
                out.push((start, buf.join(" ")));
                buf.clear();
            }
            start = i + 2;
        } else {
            if buf.is_empty() {
                start = i + 1;
            }
            buf.push(line);
        }
    }
    if !buf.is_empty() {
        out.push((start, buf.join(" ")));
    }
    out
}

#[test]
fn no_release_body_mandate_hard_codes_the_enforcement_set() {
    // What the DEFECT spelled: `requires only [gate, workspace-test]` inside a
    // paragraph telling the operator what the release body MUST state. A quoted
    // mention is exempt — both corrections quote the old wording to explain
    // themselves, and a gate that forbade that would forbid the disclosure
    // (PMAT-1430's use-vs-mention rule, PMAT-1438's exemption discipline).
    let mut offenders = Vec::new();
    for rel in ["docs/roadmaps/queue.yaml", "docs/roadmaps/roadmap.yaml"] {
        for (line, para) in paragraphs(&read(rel)) {
            let p = para.to_ascii_lowercase();
            let is_mandate = p.contains("merge-blocking") && p.contains("must state");
            if !is_mandate || !p.contains("requires only [") {
                continue;
            }
            // A MENTION carries the correction alongside it.
            let mentioned = p.contains("used to say") || p.contains("pmat-1449");
            if !mentioned {
                offenders.push(format!("{rel}:{line}: {}", para.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na release-body mandate hard-codes the required-context set:\n  {}\n\
         The live ruleset and the committed snapshot DISAGREE (an open owner decision), so a \
         typed list is wrong for one of them — and this one was wrong about the LIVE set, in \
         the section whose job is to say what is NOT enforced. Derive it from {SNAPSHOT} and \
         name the drift from {RELEASE_MD}.",
        offenders.join("\n  ")
    );
}

#[test]
fn the_mandates_point_somewhere_and_that_somewhere_discloses_the_drift() {
    // A deferral with no destination is just an omission (PMAT-1440). If the
    // mandates say "derive it", the derivation source and the drift disclosure
    // both have to be real.
    let queue = read("docs/roadmaps/queue.yaml");
    assert!(
        queue.contains("ruleset-13878864.json"),
        "no release mandate names {SNAPSHOT} as the derivation source, so \"derive it\" has no \
         destination"
    );
    assert!(
        workspace_root().join(SNAPSHOT).exists(),
        "{SNAPSHOT} does not exist, but the release mandates now send the operator to it"
    );

    let rel = read(RELEASE_MD);
    let l = rel.to_ascii_lowercase();
    assert!(
        l.contains("live ruleset requires only"),
        "{RELEASE_MD} no longer states what the LIVE ruleset requires. The release mandates defer \
         to it for exactly that, so this is now the only place the live position is written down."
    );
    assert!(
        l.contains("owner decision") || l.contains("ruleset-workspace-test-dropped"),
        "{RELEASE_MD} states the live-vs-snapshot difference but no longer names it as an OPEN \
         OWNER DECISION — without that, a reader cannot tell a detected mutation from an \
         accepted one"
    );
}

#[test]
fn release_md_names_the_set_the_snapshot_actually_records() {
    // THE BEHAVIOUR HALF. The disclosure is pinned to the JSON, not to prose:
    // re-derive the snapshot (which is how someone would "fix" the red
    // ruleset_drift) and this reds, forcing the caveat to move with it. A
    // caveat that outlives its defect is the same falsehood pointing the other
    // way (PMAT-1411 inverted exactly such a test for exactly this reason).
    let contexts = snapshot_contexts();
    let rel = read(RELEASE_MD);
    for ctx in &contexts {
        assert!(
            rel.contains(ctx.as_str()),
            "{RELEASE_MD} does not name `{ctx}`, which {SNAPSHOT} records as a required context. \
             Snapshot now records {contexts:?} — if the snapshot was re-derived, the drift \
             disclosure has to move with it."
        );
    }
    // And the disclosure must still be describing a DIFFERENCE. If the snapshot
    // ever shrinks to the live single context, "requires only `gate` while the
    // committed snapshot records `gate` + `workspace-test`" becomes false.
    if contexts.len() == 1 {
        panic!(
            "{SNAPSHOT} now records a single required context ({contexts:?}). Either the drift \
             was resolved by ratifying the weakening — which {RELEASE_MD} explicitly says not to \
             do — or it was resolved upstream. Either way the disclosure in {RELEASE_MD} and the \
             mandates in queue.yaml describe a difference that no longer exists."
        );
    }
}

#[test]
fn the_scan_actually_reaches_the_release_mandates() {
    // NON-VACUITY by anchor, not by count. If the mandate paragraphs stop
    // matching, rule 1 above passes over an empty set and would go on passing
    // forever (PMAT-1396).
    let mut found = 0usize;
    for rel in ["docs/roadmaps/queue.yaml", "docs/roadmaps/roadmap.yaml"] {
        for (_, para) in paragraphs(&read(rel)) {
            let p = para.to_ascii_lowercase();
            if p.contains("merge-blocking") && p.contains("must state") {
                found += 1;
            }
        }
    }
    assert!(
        found >= 2,
        "the release-body mandate scan matched {found} paragraph(s); PMAT-1355 and PMAT-1373 each \
         carry one, so the phrasing this rule keys on has drifted away from the runbook"
    );
}
