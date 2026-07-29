//! XPILE-RELMANDATE-001 (PMAT-1449) — the release runbook told Thursday's
//! operator to publish a false statement about what CI enforces.
//!
//! # ⚠️ CORRECTION (PMAT-1475): THE PREMISE BELOW IS FALSE
//!
//! **The mandated sentence was TRUE.** A merge to `main` is blocked by `gate`
//! **and** `workspace-test`, and always was. The measurement below read ONE
//! ruleset by id; on `2026-07-27T13:48:24` the org **moved** `workspace-test`
//! into a second ruleset, `19814559`. The effective required set —
//! `gh api repos/paiml/xpile/rules/branches/main`, the union over every ruleset
//! protecting the branch — never changed.
//!
//! So this gate was built to enforce the disclosure of a drift that does not
//! exist, and two of its four tests **required `docs/RELEASE.md` to keep
//! asserting the falsehood**. Both have been re-aimed at the invariant that was
//! actually wanted: the mandates must not hard-code a context list, and the
//! destination they defer to must name the set the receipts record. The
//! use-vs-mention discipline and the pointer doctrine below are sound and are
//! what the correcting slice reused — only the premise was wrong.
//!
//! One thing this file got exactly right, and it is worth keeping: *"a list
//! that is derived in three places and typed in a fourth will go stale in the
//! fourth."* The list was derived in three places and **all three were reading
//! the wrong ruleset**, which is the failure mode one rung up — derivation does
//! not help when every deriver shares a subject that no longer answers the
//! question.
//!
//! THE DEFECT AS ORIGINALLY DIAGNOSED (read with the correction above in mind).
//! Two work items mandate the content of the v0.1.618 release body.
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
//! 2. The destination the mandates point at must EXIST and must name the
//!    endpoint that answers the question — `repos/paiml/xpile/rules/branches/
//!    main`. **A deferral with no destination is just an omission**
//!    (PMAT-1440); *re-aimed by PMAT-1475, which found this rule demanding that
//!    `RELEASE.md` keep asserting a drift that did not exist.*
//! 3. `docs/RELEASE.md` must name every context the committed RECEIPTS record,
//!    derived from the JSON — the union across `docs/status/ruleset-*.json`,
//!    not one file. That is the behaviour half: re-derive the receipts and this
//!    reds until the prose follows. A caveat that outlives its defect is the
//!    same falsehood pointing the other way (PMAT-1411).
//!
//! NOT TOUCHED, deliberately: the `XPILE-ENFORCEMENT REQUIRED-CONTEXTS:`
//! markers in `ci.yml`, `CURRENT.md` and `enforcement-handoff.md` all read
//! `gate, workspace-test`. They were **correct throughout** — including for the
//! two days everything around them said otherwise — and
//! `ruleset_drift::enforcement_markers_match_the_committed_snapshot` held them
//! green the whole time. *The original text of this paragraph justified leaving
//! them alone on the grounds that changing them would ratify a live weakening.
//! There was no weakening; they were simply right.* That is worth recording:
//! **the markers, the receipts and this repo's own `workspace-test` job were
//! all telling the truth, and the only thing that had gone wrong was a gate
//! reading one ruleset — yet the prose moved toward the gate.**

use std::path::{Path, PathBuf};

const SNAPSHOT: &str = "docs/status/ruleset-*.json";
const SNAPSHOT_DIR: &str = "docs/status";
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

/// The UNION over every committed receipt — what actually blocks a merge.
/// Derived, never typed; that is the whole point of this file.
///
/// Reading a single receipt is the defect PMAT-1475 corrected: enforcement is a
/// property of the BRANCH, and since 2026-07-27 two rulesets supply it.
fn snapshot_contexts() -> Vec<String> {
    let dir = workspace_root().join(SNAPSHOT_DIR);
    let mut out = Vec::new();
    let mut receipts = 0usize;
    for entry in std::fs::read_dir(&dir)
        .expect("docs/status/ exists")
        .flatten()
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("ruleset-") || !name.ends_with(".json") {
            continue;
        }
        receipts += 1;
        let body = std::fs::read_to_string(&path).expect("receipt is readable");
        let v: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"));
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
    }
    assert!(
        receipts > 0,
        "no {SNAPSHOT} receipts found — every assertion below would range over nothing"
    );
    out.sort();
    out.dedup();
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
        queue.contains("ruleset-13878864.json") || queue.contains("ruleset-*.json"),
        "no release mandate names the {SNAPSHOT} receipts as the derivation source, so \
         \"derive it\" has no destination"
    );
    assert!(
        !snapshot_contexts().is_empty(),
        "no {SNAPSHOT} receipt records a required context, but the release mandates send the \
         operator to them"
    );

    // PMAT-1475 re-aimed this. It used to require RELEASE.md to keep asserting
    // "the live ruleset requires only …", which was a disclosure of a drift that
    // never existed — so the gate ENFORCED the falsehood and would have red-ed
    // the fix. What the mandates actually need from RELEASE.md is a live,
    // derivable position: the command that answers the question, not a typed set.
    let rel = read(RELEASE_MD);
    let l = rel.to_ascii_lowercase();
    assert!(
        l.contains("rules/branches/main"),
        "{RELEASE_MD} no longer names `gh api repos/paiml/xpile/rules/branches/main`. The release \
         mandates defer to it for what blocks a merge, and that endpoint — the union over every \
         ruleset protecting the branch — is the only thing that answers it. A per-ruleset read \
         cannot, which is the PMAT-1475 defect."
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
    // PMAT-1475 replaced the old tripwire here. It fired when the receipts
    // recorded a SINGLE context, on the theory that the only way to reach that
    // state was to ratify a weakening. That theory was wrong twice over: the
    // union across receipts is what matters, and there was no weakening. Worse,
    // the tripwire read one receipt, so it would have fired on the CORRECT
    // re-derivation — a gate that reds on the fix is a gate that enforces the
    // defect.
    //
    // What survives is the property that was always wanted: the disclosure is
    // pinned to the RECEIPTS, so re-deriving them forces the prose to move too.
    assert!(
        contexts.len() >= 2,
        "the {SNAPSHOT} receipts record only {contexts:?} as merge-blocking. Before editing any \
         prose, run `gh api repos/paiml/xpile/rules/branches/main` — a context vanishing from one \
         ruleset is not a weakening if it reappears in another, which is exactly what happened on \
         2026-07-27 and cost two days. If enforcement genuinely was reduced, re-derive every \
         receipt first and let the prose follow."
    );
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
