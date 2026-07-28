//! XPILE-RUNBOOK-001 (PMAT-1453) — the release runbook cited a line number, and
//! the text AT that line number was the warning that the runbook was wrong.
//!
//! THE DEFECT, and it is procedural rather than cosmetic. PMAT-1373 — the
//! Thursday tag-cut item — opened with:
//!
//! > Version is single-sourced at `Cargo.toml:43` — one line + `cargo check`
//! > refreshes all 31 Cargo.lock entries.
//!
//! `Cargo.toml:43` is:
//!
//! ```text
//! # RELEASE BUMP TOUCHES 35 LINES, NOT THIS ONE (PMAT-1408). Every
//! # intra-workspace path-dep below repeats this number, because a
//! # `[workspace.dependencies]` entry cannot inherit `[workspace.package].version`
//! ```
//!
//! **The citation pointed at its own refutation.** The `version =` key is on
//! line 53; line 43 is the comment saying the instruction above it is wrong.
//! Measured: the version literal appears on **35 sites** in `Cargo.toml`.
//!
//! WHY THAT IS EXPENSIVE, in this repo's own recorded experience. PMAT-1408's
//! note says that through v0.1.617 the path-deps read `"0.1.12"` while
//! `[workspace.package]` read `"0.1.617"` — *"and nothing noticed: `^0.1.12` is
//! satisfied by 0.1.617, so every dry-run packaged clean while the published
//! manifests let a resolver pair releases 605 apart."* A one-line bump on
//! Thursday re-creates exactly that skew, and it re-creates it in the direction
//! `cargo publish --dry-run` cannot see.
//!
//! SECOND DEFECT, same shape. The same item mandates that the release body list
//! which jobs are ADVISORY, and enumerated *"kani, lake-build, docs, wasi,
//! lean-models, shader-validate"*. Derived from the workflows, the unrequired
//! set also contains **`license-scan`**, **`build`** and **`deploy`** — so the
//! published list understated what is unenforced, in the section whose job is
//! to disclose exactly that. Note the pattern with [[PMAT-1449]]: the
//! `XPILE-ADVISORY` markers are derived and correct (`ruleset_drift` holds them
//! equal to the unrequired set); it was the **typed prose** that was stale.
//! Twice now, in one runbook.
//!
//! WHAT THIS FILE PINS.
//!
//! 1. The runbook may not describe the version bump as one line, and the claim
//!    is checked against a COUNT DERIVED from `Cargo.toml` rather than a number
//!    written down here. If the workspace ever genuinely becomes
//!    single-sourced, this reds and the runbook text must move with it — a
//!    caveat that outlives its defect is the same falsehood pointing the other
//!    way (PMAT-1411).
//! 2. A `Cargo.toml:<N>` citation in the runbook must point at a line that
//!    supports the claim being made about it.
//! 3. The runbook may not type an advisory roster; it must defer to the
//!    derivation, and the derivation must exist.
//!
//! Quoted MENTIONS of the old wording are exempt — both corrections quote
//! themselves to explain what changed, and a rule forbidding that would forbid
//! the disclosure (PMAT-1430's use-vs-mention rule, PMAT-1449's exemption
//! discipline).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const QUEUE: &str = "docs/roadmaps/queue.yaml";
const MANIFEST: &str = "Cargo.toml";

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

/// The workspace version, and every line of `Cargo.toml` that carries it as a
/// `version = "…"` assignment. DERIVED — no count is written down in this file.
fn version_sites() -> (String, Vec<usize>) {
    let body = read(MANIFEST);
    let version = body
        .lines()
        .find_map(|l| {
            let t = l.trim();
            t.strip_prefix("version = \"")
                .and_then(|r| r.split('"').next())
                .map(str::to_string)
        })
        .expect("Cargo.toml declares a workspace version");
    let needle = format!("version = \"{version}\"");
    let sites: Vec<usize> = body
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(&needle) && !l.trim_start().starts_with('#'))
        .map(|(i, _)| i + 1)
        .collect();
    assert!(
        !sites.is_empty(),
        "no line of {MANIFEST} assigns version {version:?} — the derivation below is measuring \
         nothing"
    );
    (version, sites)
}

/// The PMAT-1373 note, whitespace-flattened.
fn runbook() -> String {
    let body = read(QUEUE);
    let doc: serde_yaml::Value =
        serde_yaml::from_str(&body).unwrap_or_else(|e| panic!("{QUEUE} is not valid YAML: {e}"));
    fn find(v: &serde_yaml::Value) -> Option<String> {
        match v {
            serde_yaml::Value::Mapping(m) => {
                if m.get(serde_yaml::Value::from("id"))
                    .and_then(|i| i.as_str())
                    == Some("PMAT-1373")
                {
                    return m
                        .get(serde_yaml::Value::from("notes"))
                        .and_then(|n| n.as_str())
                        .map(str::to_string);
                }
                m.values().find_map(find)
            }
            serde_yaml::Value::Sequence(s) => s.iter().find_map(find),
            _ => None,
        }
    }
    let notes = find(&doc).expect("PMAT-1373 is in queue.yaml with a `notes` field");
    notes.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True if the occurrence at `at` sits INSIDE a double-quoted span — i.e. the
/// runbook is QUOTING the old wording rather than instructing with it.
///
/// A proximity window is not good enough, and the red half proved it: the first
/// draft exempted anything within 240 chars of `used to say` / `PMAT-1453`, and
/// since the correction and its disclosure are adjacent by construction, a
/// re-typed ADVISORY roster landed inside that window and the perturbation
/// stayed GREEN. **An exemption keyed on NEARNESS exempts the neighbourhood.**
/// Quote-enclosure is the property that actually distinguishes a mention from a
/// use (PMAT-1430), and it is what the corrections genuinely do.
fn is_mention(hay: &str, at: usize) -> bool {
    // Global quote PARITY was the second attempt and it was also wrong: the
    // note carries many quoted spans, so one unbalanced quote anywhere flips
    // the verdict for everything after it, and the CONTROL went red. What
    // actually distinguishes a mention here is the shape the corrections use —
    // `used to say "<old wording>"` — so require BOTH the reporting verb and an
    // opening quote to sit immediately before the occurrence.
    let lead = &hay[at.saturating_sub(160)..at];
    let quoted_recently = hay[at.saturating_sub(80)..at].contains('"');
    let reported = lead.contains("used to say") || lead.contains("used to enumerate");
    quoted_recently && reported
}

#[test]
fn the_runbook_does_not_describe_the_version_bump_as_one_line() {
    let (version, sites) = version_sites();
    // THE BEHAVIOUR HALF lives here: the expectation is derived from the
    // manifest, so making the workspace genuinely single-sourced reds this and
    // forces the runbook text to move (PMAT-1411 inverted such a test for
    // exactly this reason).
    assert!(
        sites.len() > 1,
        "{MANIFEST} now assigns version {version:?} on a single line ({sites:?}). The bump really \
         IS one line, so PMAT-1373's corrected text — which says it is NOT — has become the stale \
         claim. Move it back and delete this assertion."
    );

    let n = runbook();
    for pat in ["single-sourced at Cargo.toml", "one line +"] {
        if let Some(at) = n.find(pat) {
            assert!(
                is_mention(&n, at),
                "PMAT-1373 states {pat:?} as an instruction. {MANIFEST} assigns {version:?} on {} \
                 lines ({sites:?}); a one-line bump leaves the intra-workspace path-deps behind, \
                 which is the published-manifest skew PMAT-1408 removed and which \
                 `cargo publish --dry-run` cannot see.",
                sites.len()
            );
        }
    }
}

#[test]
fn a_cargo_toml_line_citation_points_at_what_it_claims() {
    // The original defect in one assertion: the runbook cited `Cargo.toml:43`
    // for the version, and line 43 is the comment refuting the instruction.
    let (_, sites) = version_sites();
    let body = read(MANIFEST);
    let lines: Vec<&str> = body.lines().collect();
    let n = runbook();

    // The offset handed to `is_mention` must be ABSOLUTE. The first draft
    // advanced a `rest` slice and passed the RELATIVE index, so from the second
    // match onward the exemption window was read from the wrong part of the
    // string — and the test failed on this file's own disclosure sentence.
    let mut cited: BTreeSet<usize> = BTreeSet::new();
    let mut base = 0usize;
    while let Some(rel) = n[base..].find("Cargo.toml:") {
        let at = base + rel;
        let tail = &n[at + "Cargo.toml:".len()..];
        let num: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(v) = num.parse::<usize>() {
            if !is_mention(&n, at) {
                cited.insert(v);
            }
        }
        base = at + "Cargo.toml:".len();
    }

    for line_no in cited {
        let text = lines.get(line_no - 1).copied().unwrap_or("");
        assert!(
            sites.contains(&line_no),
            "PMAT-1373 cites {MANIFEST}:{line_no} for the version, but that line is {text:?}. The \
             version is assigned on {sites:?}. A line citation that drifts is worse than none: \
             this one pointed at the comment that says the instruction beside it is wrong."
        );
    }
}

#[test]
fn the_runbook_does_not_type_an_advisory_roster() {
    // Derived: every CI job across the workflows, minus the required contexts.
    // The runbook must defer to that rather than carrying a list — it carried
    // one, and the list omitted three jobs.
    let n = runbook();
    if let Some(at) = n.find("are ADVISORY") {
        assert!(
            is_mention(&n, at),
            "PMAT-1373 enumerates an ADVISORY roster inline. The unrequired set is derived from \
             .github/workflows minus the required contexts, and the typed list omitted \
             `license-scan`, `build` and `deploy` — understating what is unenforced in the \
             section whose job is to disclose it. Defer to the XPILE-ADVISORY markers, which \
             `ruleset_drift` already holds equal to the derived set."
        );
    }
    assert!(
        n.contains("AS DERIVED"),
        "PMAT-1373 no longer tells the operator to derive the advisory set, so nothing sends them \
         to the markers that carry it"
    );
}

#[test]
fn the_runbook_scan_actually_reaches_pmat_1373() {
    // NON-VACUITY by anchor. If the note stops matching, every rule above
    // passes over an empty string and goes on passing (PMAT-1396).
    let n = runbook();
    assert!(
        n.len() > 500,
        "PMAT-1373's notes are {} chars — the scan is not reaching the runbook",
        n.len()
    );
    for anchor in ["THURSDAY 2026-07-30", "Cargo.lock", "ADVISORY"] {
        assert!(
            n.contains(anchor),
            "PMAT-1373's notes no longer mention {anchor:?}; the phrasing these rules key on has \
             drifted away from the runbook"
        );
    }
}
