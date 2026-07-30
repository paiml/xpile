//! XPILE-RELMEMBER-001 (PMAT-1496) — the released CHANGELOG section credited
//! v0.1.618 with fifteen slices that are not in the v0.1.618 tag.
//!
//! THE DEFECT. The release commit rolled `## [Unreleased]` into
//! `## [0.1.618] - 2026-07-31` and **did not open a replacement**. Work kept
//! merging — the tag was cut at `00:32` and slices landed all morning — and the
//! per-slice CHANGELOG convention kept writing an entry per arc. With no
//! `[Unreleased]` heading, every one of them landed under the only heading
//! available: the section describing a tag that does not contain them.
//!
//! Measured before the fix: `[0.1.618]` on `main` was **8,702 lines** against the
//! tagged section's **7,701**, and **15 of its 96 arc headings** (PMAT-1480
//! through PMAT-1494) cite ids unreachable from `v0.1.618^{commit}`.
//!
//! WHAT WAS *NOT* THE CAUSE, checked before it was asserted: `changelog_freshness`
//! assertion 3 requires an entry for every id whose commit touched **shipped
//! source** (`crates/*/src/**`, `contracts/**.yaml`). **Zero** of the fifteen
//! touched shipped source — they are docs/tests slices under the release freeze —
//! so that gate neither required these entries nor was violated by them. It
//! passes either way. The first draft of this file blamed it; measuring the
//! fifteen commits' touched paths refuted that. **A plausible mechanism is not a
//! cause until the mechanism is measured.**
//!
//! WHY IT MATTERS EVEN THOUGH THE PUBLISHED ARTIFACT IS SAFE. The publish runs
//! from a worktree checked out **at the tag**, so what reaches crates.io is the
//! tagged CHANGELOG and is correct. The damage is on `main` and permanent: the
//! release notes a reader sees on the default branch credit 0.1.618 with work it
//! never contained, and fifteen slices are filed under the wrong release forever
//! once the next roll happens. A release section is the one part of a CHANGELOG
//! that must never move again.
//!
//! THE INVARIANT IS HISTORICALLY CLEAN, WHICH IS WHY IT IS THE RIGHT ONE. Run over
//! every `## [x.y.z]` section back to `[0.1.587]` — roughly thirty sections — the
//! rule finds **zero** violations anywhere except `[0.1.618]`. It is not a new
//! convention being imposed; it is a property the project has always held and
//! only just broke.
//!
//! WHAT THIS FILE PINS.
//!
//! 1. **Every arc heading in a RELEASED section cites an id reachable from that
//!    section's own tag.** Fully derived — no version and no id is written down.
//!    It sharpens by itself: every release adds a section to the corpus.
//! 2. **If anything has merged since the newest tag, an `[Unreleased]` heading
//!    exists.** This is the poka-yoke for the root cause: forgetting to open the
//!    replacement section reds on the *next* merge instead of after fifteen.
//! 3. **`[Unreleased]`, when present, is the leading heading** and holds only
//!    post-tag work — so the fix cannot be undone by filing tagged work there.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xpile has a workspace root two levels up")
        .to_path_buf()
}

fn changelog() -> String {
    let p = workspace_root().join("CHANGELOG.md");
    std::fs::read_to_string(&p).expect("CHANGELOG.md is readable")
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace_root())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `(heading name, section body)` in file order.
fn sections(body: &str) -> Vec<(String, String)> {
    let marks: Vec<(usize, String)> = body
        .match_indices("\n## [")
        .filter_map(|(i, _)| {
            let rest = &body[i + 5..];
            rest.find(']').map(|e| (i + 1, rest[..e].to_string()))
        })
        .collect();
    let mut out = Vec::new();
    for (n, (start, name)) in marks.iter().enumerate() {
        let end = marks.get(n + 1).map(|(s, _)| *s).unwrap_or(body.len());
        out.push((name.clone(), body[*start..end].to_string()));
    }
    assert!(
        out.len() > 20,
        "only {} `## [...]` sections parsed out of CHANGELOG.md; the heading scanner has gone \
         blind and every assertion below would range over almost nothing",
        out.len()
    );
    out
}

/// The id an arc CLAIMS as its own: the trailing `(PMAT-N)` of a `###` heading.
/// Prose mentions of other ids are references, not membership claims.
fn arc_ids(section_body: &str) -> Vec<String> {
    section_body
        .lines()
        .filter(|l| l.starts_with("### "))
        .filter_map(|l| {
            let t = l.trim_end();
            let close = t.strip_suffix(')')?;
            let open = close.rfind("(PMAT-")?;
            let id = &close[open + 1..];
            id.strip_prefix("PMAT-")
                .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                .map(|_| id.to_string())
        })
        .collect()
}

/// Every PMAT id reachable from a tag's commit.
fn ids_in_tag(tag: &str) -> Option<BTreeSet<String>> {
    let log = git(&["log", "--format=%s%n%b", &format!("{tag}^{{commit}}")])?;
    Some(
        log.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .filter(|t| {
                t.starts_with("PMAT-") && t.len() > 5 && t[5..].bytes().all(|b| b.is_ascii_digit())
            })
            .map(str::to_string)
            .collect(),
    )
}

fn tag_exists(tag: &str) -> bool {
    git(&["rev-parse", "-q", "--verify", &format!("{tag}^{{commit}}")]).is_some()
}

#[test]
fn no_released_section_claims_an_arc_that_is_not_in_its_tag() {
    // THE RULE. Derived end to end: no version literal, no id, no count.
    let body = changelog();
    let mut offences: Vec<String> = Vec::new();
    let mut checked_sections = 0usize;
    let mut checked_arcs = 0usize;
    let mut skipped: Vec<String> = Vec::new();

    for (name, sec) in sections(&body) {
        if name == "Unreleased" {
            continue;
        }
        let tag = format!("v{name}");
        if !tag_exists(&tag) {
            skipped.push(tag);
            continue;
        }
        let reach = match ids_in_tag(&tag) {
            Some(r) => r,
            None => {
                skipped.push(tag);
                continue;
            }
        };
        checked_sections += 1;
        for id in arc_ids(&sec) {
            checked_arcs += 1;
            if !reach.contains(&id) {
                offences.push(format!("[{name}] claims {id}, which is not in {tag}"));
            }
        }
    }

    // NON-VACUITY. Without history (an extracted `.crate`) there is nothing to
    // check; with it, the corpus must be substantial or this rule has gone blind.
    if checked_sections == 0 {
        eprintln!(
            "warning: no release tag is present in this checkout ({} skipped); \
             XPILE-RELMEMBER-001's membership half did not run.",
            skipped.len()
        );
        return;
    }
    assert!(
        checked_sections >= 5 && checked_arcs >= 30,
        "the membership rule ranged over only {checked_sections} section(s) and {checked_arcs} \
         arc heading(s); the `### … (PMAT-N)` convention or the tag naming has changed and this \
         gate has stopped seeing the corpus"
    );

    assert!(
        offences.is_empty(),
        "\na RELEASED CHANGELOG section claims arcs that its own tag does not contain:\n  {}\n\n\
         Checked {checked_arcs} arc headings across {checked_sections} released sections. A \
         released section must never move again, so work merged AFTER a tag belongs under \
         `## [Unreleased]`, not under the section naming that tag. If the release commit rolled \
         `[Unreleased]` away without opening a replacement, open one and re-file these entries — \
         do not delete them.",
        offences.join("\n  ")
    );
}

#[test]
fn an_unreleased_heading_exists_whenever_work_has_merged_since_the_newest_tag() {
    // THE POKA-YOKE FOR THE ROOT CAUSE. The release commit is allowed to leave no
    // `[Unreleased]` — for exactly as long as nothing else has merged. The first
    // commit after the tag makes the section mandatory, so forgetting it reds
    // immediately rather than after fifteen slices.
    let Some(newest) = git(&["describe", "--tags", "--abbrev=0"]).map(|s| s.trim().to_string())
    else {
        eprintln!("warning: no tags in this checkout; the [Unreleased]-presence half did not run.");
        return;
    };
    let Some(since) = git(&["log", "--oneline", &format!("{newest}^{{commit}}..HEAD")]) else {
        eprintln!("warning: `git log` unavailable; the [Unreleased]-presence half did not run.");
        return;
    };
    let n = since.lines().filter(|l| !l.trim().is_empty()).count();
    let has = changelog().contains("\n## [Unreleased]");
    assert!(
        n == 0 || has,
        "{n} commit(s) have merged since {newest} and CHANGELOG.md has no `## [Unreleased]` \
         heading, so every new entry must land under a RELEASED section — which is how \
         `[0.1.618]` came to claim 15 arcs it does not contain (PMAT-1496). Open an \
         `## [Unreleased]` section above the newest release heading."
    );
}

#[test]
fn unreleased_is_the_leading_section_and_holds_only_post_tag_work() {
    let body = changelog();
    let secs = sections(&body);
    let Some(pos) = secs.iter().position(|(n, _)| n == "Unreleased") else {
        return; // legitimately absent immediately after a roll
    };
    assert_eq!(
        pos,
        0,
        "`## [Unreleased]` is section {} of the CHANGELOG; it must lead, or a reader takes the \
         release above it as current",
        pos + 1
    );

    // Its arcs must NOT be in the newest tag — otherwise released work is being
    // re-filed as unreleased, which is this fix run backwards.
    let Some(newest) = git(&["describe", "--tags", "--abbrev=0"]).map(|s| s.trim().to_string())
    else {
        return;
    };
    let Some(reach) = ids_in_tag(&newest) else {
        return;
    };
    let leaked: Vec<String> = arc_ids(&secs[0].1)
        .into_iter()
        .filter(|id| reach.contains(id))
        .collect();
    assert!(
        leaked.is_empty(),
        "`## [Unreleased]` claims {} arc(s) that ARE in {newest}: {leaked:?}. Released work must \
         stay in its release section.",
        leaked.len()
    );
}

#[test]
fn the_rule_detects_a_constructed_violation() {
    // RED-HALF-IN-GATE. The assertions above pass over a repaired CHANGELOG, which
    // proves nothing about whether they can fail. This runs the detection logic
    // against a synthetic section so the rule can never be silently satisfied by
    // a shape it cannot parse.
    let synthetic = "\n## [9.9.9] - 2099-01-01\n\n\
        ### A real arc that is in the tag (PMAT-0001)\n\nbody\n\n\
        ### An arc merged after the tag (PMAT-9999)\n\nbody\n";
    let ids = arc_ids(synthetic);
    assert_eq!(
        ids,
        vec!["PMAT-0001".to_string(), "PMAT-9999".to_string()],
        "the arc-heading parser did not extract both ids from a well-formed section; the real \
         rule is therefore blind to the same shape"
    );
    let reach: BTreeSet<String> = ["PMAT-0001".to_string()].into_iter().collect();
    let offending: Vec<&String> = ids.iter().filter(|i| !reach.contains(*i)).collect();
    assert_eq!(
        offending.len(),
        1,
        "the membership check did not flag the arc absent from the tag"
    );

    // And a heading with no trailing citation is not a membership claim — the
    // false-positive half. `### Fixed` and `### Added` must not be treated as arcs.
    assert!(
        arc_ids("### Fixed\n### Added\n### Known divergences\n").is_empty(),
        "bare `###` headings are being read as arc membership claims"
    );
}
