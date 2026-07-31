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
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
/// The same marker `ruleset_drift.rs` reads. Kept as one literal rather than a
/// second roster: PMAT-1488's duplicated-normative-set rule applies to gates too.
const ADVISORY_MARKER: &str = "XPILE-ENFORCEMENT ADVISORY-CONTEXTS:";

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
    // PMAT-1506 — this loop used to run `if let Some(at) = n.find(pat)` with no
    // floor. `"one line +"` is ABSENT from the corrected note (measured: -1),
    // so that half of the loop had never executed its assertion, and nothing
    // said so; the test survived on the sibling pattern, which is exactly why
    // the shape reads as covered. The floor below makes the survivor explicit:
    // if the phrasing drifts so that NEITHER pattern is found, this reds
    // instead of passing over an empty scan (PMAT-1396).
    let mut matched = 0usize;
    for pat in ["single-sourced at Cargo.toml", "one line +"] {
        if let Some(at) = n.find(pat) {
            matched += 1;
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
    assert!(
        matched > 0,
        "neither one-line spelling occurs in PMAT-1373's notes any more, so this rule scanned \
         nothing and passed. Re-key it on the wording the runbook actually uses, or retire it."
    );
}

/// Every `Cargo.toml:<N>` this note CITES — mentions excluded.
///
/// The offset handed to `is_mention` must be ABSOLUTE. The first draft advanced
/// a `rest` slice and passed the RELATIVE index, so from the second match
/// onward the exemption window was read from the wrong part of the string — and
/// the test failed on this file's own disclosure sentence.
///
/// Factored out of the test by PMAT-1505 so it can be driven by a control. On
/// the live runbook it returns EMPTY — both occurrences are quoted mentions
/// inside PMAT-1453's correction — which means the loop below asserts nothing
/// today. That is correct for a negative detector, and it is also exactly how a
/// detector dies unnoticed: widen `is_mention` by accident and the set stays
/// empty for the wrong reason, with nothing to say so.
fn cited_manifest_lines(note: &str) -> BTreeSet<usize> {
    let mut cited: BTreeSet<usize> = BTreeSet::new();
    let mut base = 0usize;
    while let Some(rel) = note[base..].find("Cargo.toml:") {
        let at = base + rel;
        let tail = &note[at + "Cargo.toml:".len()..];
        let num: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(v) = num.parse::<usize>() {
            if !is_mention(note, at) {
                cited.insert(v);
            }
        }
        base = at + "Cargo.toml:".len();
    }
    cited
}

/// PMAT-1505 — the CONTROL the loop below has never had. The detector's
/// emptiness on the live corpus is only good news if the detector can still
/// detect; without this, `is_mention` widening to exempt everything is
/// indistinguishable from a clean runbook.
#[test]
fn the_citation_detector_can_still_fire() {
    let live_use = "The bump touches Cargo.toml:43 — go edit it.";
    assert_eq!(
        cited_manifest_lines(live_use),
        BTreeSet::from([43]),
        "an un-quoted line citation must be COLLECTED, or this detector is dead \
         and the test it feeds is permanently vacuous"
    );

    let reported = "PMAT-1453: this line used to say \"Version is single-sourced at \
                    Cargo.toml:43 — one line\", and it was wrong.";
    assert!(
        cited_manifest_lines(reported).is_empty(),
        "a citation quoted inside a `used to say \"…\"` correction is a MENTION and \
         must stay exempt — this is the shape the live runbook uses"
    );

    // And the emptiness asserted below is the LIVE reading, recorded here so a
    // future change that starts citing shows up as a change to this file too.
    assert!(
        cited_manifest_lines(&runbook()).is_empty(),
        "the runbook now carries a live `Cargo.toml:<N>` citation. That is not a \
         failure — it means the loop in the next test finally has a subject. \
         Check the citation, then update this expectation."
    );
}

#[test]
fn a_cargo_toml_line_citation_points_at_what_it_claims() {
    // The original defect in one assertion: the runbook cited `Cargo.toml:43`
    // for the version, and line 43 is the comment refuting the instruction.
    let (_, sites) = version_sites();
    let body = read(MANIFEST);
    let lines: Vec<&str> = body.lines().collect();
    let n = runbook();

    // EMPTY on the live corpus — `the_citation_detector_can_still_fire` is what
    // keeps that from being a silent pass (PMAT-1505).
    let cited = cited_manifest_lines(&n);

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

/// The ADVISORY context set, DERIVED from the marker `ruleset_drift.rs` already
/// holds equal to *every CI job minus the required contexts*. No roster is
/// typed here either — the marker is the single source, and if it moves, this
/// moves with it.
fn advisory_contexts() -> Vec<String> {
    let ci = read(CI_WORKFLOW);
    let line = ci
        .lines()
        .find_map(|l| l.split_once(ADVISORY_MARKER).map(|(_, r)| r))
        .unwrap_or_else(|| {
            panic!(
                "no `{ADVISORY_MARKER}` marker in {CI_WORKFLOW} — rule 3's derivation does not \
                 exist, so nothing the runbook defers TO is checked"
            )
        });
    line.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Advisory job names TYPED into the note as instruction, i.e. occurring where
/// `is_mention` does not exempt them.
///
/// PMAT-1506 — factored out of the test so a CONTROL can drive it. Two
/// screens, both measured against the live note rather than guessed: a name
/// followed by `/` is a PATH SEGMENT (`docs/RELEASE.md`, `docs/status/…` — three
/// live occurrences, none of them a roster entry), and a name that is part of a
/// longer identifier is not that name.
fn typed_advisory_names(note: &str, advisory: &[String]) -> BTreeSet<String> {
    let bytes = note.as_bytes();
    let boundary = |c: u8| !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    let mut typed = BTreeSet::new();
    for name in advisory {
        let mut base = 0usize;
        while let Some(rel) = note[base..].find(name.as_str()) {
            let at = base + rel;
            let end = at + name.len();
            let before_ok = at == 0 || boundary(bytes[at - 1]);
            let after = bytes.get(end).copied();
            let after_ok = after.is_none_or(boundary);
            let path_segment = after == Some(b'/');
            if before_ok && after_ok && !path_segment && !is_mention(note, at) {
                typed.insert(name.clone());
            }
            base = end;
        }
    }
    typed
}

#[test]
fn the_runbook_does_not_type_an_advisory_roster() {
    // PMAT-1506 — this rule used to key on the literal `"are ADVISORY"`, which
    // is ABSENT from the note (measured: -1). The corrected runbook spells it
    // "state the ADVISORY set AS DERIVED", so the antecedent was FALSE and the
    // assertion inside it had never executed; the test passed on the
    // `AS DERIVED` sibling below, which is why it read as covered. Worse, the
    // needle could only ever have caught ONE phrasing of a roster — the defect
    // it was written for was spelled `"kani, lake-build, docs, wasi,
    // lean-models, shader-validate"`, which does not contain it. The rule is
    // now keyed on the NAMES, derived from the marker (PMAT-1501's shape: a
    // gate whose needle cannot represent the claim it forbids).
    let advisory = advisory_contexts();
    assert!(
        advisory.len() > 1,
        "the derived advisory set is {advisory:?}. Rule 3 says the runbook must defer to a \
         derivation AND that the derivation must exist; with fewer than two contexts there is \
         nothing to defer to and the deferral is decoration."
    );

    let n = runbook();
    let typed = typed_advisory_names(&n, &advisory);
    assert!(
        typed.is_empty(),
        "PMAT-1373 types the advisory job name(s) {typed:?} as instruction. The unrequired set is \
         derived from .github/workflows minus the required contexts, and the last typed list \
         omitted `license-scan`, `build` and `deploy` — understating what is unenforced in the \
         section whose job is to disclose it. Defer to the `{ADVISORY_MARKER}` marker, which \
         `ruleset_drift` already holds equal to the derived set."
    );
    assert!(
        n.contains("AS DERIVED"),
        "PMAT-1373 no longer tells the operator to derive the advisory set, so nothing sends them \
         to the markers that carry it"
    );
}

/// PMAT-1506 — the CONTROL this rule has never had. Its live reading is EMPTY,
/// and an empty negative detector is indistinguishable from a dead one without
/// a constructed positive (PMAT-1505's lesson, one rule along in the same file).
#[test]
fn the_advisory_roster_detector_can_still_fire() {
    let advisory = advisory_contexts();

    // POSITIVE: a typed roster is collected.
    let typed = "The release body must state that kani and lake-build are advisory.";
    assert_eq!(
        typed_advisory_names(typed, &advisory),
        BTreeSet::from(["kani".to_string(), "lake-build".to_string()]),
        "a roster typed as instruction must be COLLECTED, or this detector is dead and the rule \
         it feeds is permanently vacuous"
    );

    // NEGATIVE 1: the shape the live runbook uses — a quoted retired wording.
    let quoted = "Then state the ADVISORY set AS DERIVED rather than typed. PMAT-1453: this line \
                  used to enumerate \"kani, lake-build, docs, wasi, lean-models, shader-validate\" \
                  and OMITTED three jobs.";
    assert!(
        typed_advisory_names(quoted, &advisory).is_empty(),
        "a roster quoted inside a `used to enumerate \"…\"` correction is a MENTION and must stay \
         exempt — forbidding it would forbid the disclosure"
    );

    // NEGATIVE 2: the path screen. `docs` is both an advisory context and the
    // first segment of every doc path in the note; without this screen the rule
    // reports three offences on a clean runbook and gets disabled.
    let paths = "read it from docs/status/ruleset-13878864.json and the drift in docs/RELEASE.md";
    assert!(
        typed_advisory_names(paths, &advisory).is_empty(),
        "a context name followed by `/` is a path segment, not a roster entry"
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
