//! XPILE-RELPLAN-001 (PMAT-1467) — the sprint plan told Thursday's operator to
//! create and push a tag that ALREADY EXISTS.
//!
//! THE DEFECT. `docs/specifications/sub/sprint-6day-2026-07-26.md` §5 "Release
//! plan" is an entire release behind. Its tag line read:
//!
//! > **TAG: `v0.1.617`, created and pushed THURSDAY 2026-07-30** on a pinned SHA
//!
//! `v0.1.617` shipped on **2026-07-26**. It is in `git tag --list` and it is
//! `max_version` on crates.io. The queue's sprint block says
//! `version: 0.1.618` and PMAT-1373 is titled *"RELEASE COMMIT v0.1.618"*. So
//! the plan instructed the operator to `git tag v0.1.617 <SHA> && git push` — a
//! command that fails outright, or, with `-f`, **moves a published tag**. The
//! plan's own A3 abort clause already names `v0.1.618` as the fallback, so the
//! document contradicted itself.
//!
//! FIVE STALE LITERALS IN ONE SECTION, all measured:
//!
//! | site | said | measured |
//! |---|---|---|
//! | VERSION | `0.1.617` | queue sprint block says `0.1.618` |
//! | VERSION | "single-sourced at `Cargo.toml:43` … One line" | 35 assignment sites; line 43 is the comment refuting it |
//! | TAG | `v0.1.617` | already in `git tag --list` and on crates.io |
//! | CRATES.IO BATCH | "live and unyanked at 0.1.616" | crates.io `max_version` is 0.1.617 |
//! | honest-claim | "**795** WASM witness tests" | `witness_floor.rs` derives **857** |
//!
//! THIS IS PMAT-1453 LANDING ONE SURFACE SHORT OF ITS OWN CLASS, and the author
//! was me. PMAT-1453 fixed the one-line-bump falsehood in the runbook and gated
//! it — over `docs/roadmaps/queue.yaml` and `docs/roadmaps/roadmap.yaml`. The
//! **sprint plan** is a release-planning document too, carries the identical
//! sentence, and was outside the gate's corpus. [[PMAT-1438]]'s lesson, applied
//! to a gate written the day before: **scope the gate to the CLAIM, then ask
//! what the widest set of documents is that could carry it.** The corpus here is
//! therefore *every* release-planning document, discovered rather than listed.
//!
//! WHAT IS HONEST AND MUST NOT BE "CORRECTED", measured: *"all 31 members on
//! `version.workspace = true`"* is **true** — 31 of 31 member manifests carry
//! it. The members really do inherit; it is the `[workspace.dependencies]`
//! path-deps that cannot. A fix that flattened that sentence would have replaced
//! a true statement with a false one, which is the [[PMAT-1446]] trap.
//!
//! WHAT THIS FILE PINS.
//!
//! 1. **A release-planning document may not name, as the version to be created,
//!    a version that is already a shipped git tag.** This is the rule that
//!    catches the expensive defect, and it is derived from `git tag --list` — no
//!    version number is written down here.
//! 2. The declared release version agrees with the queue's `sprint.version`.
//! 3. The one-line-bump falsehood may not reappear in ANY release-planning
//!    document — PMAT-1453's rule, with the corpus widened to the class.
//!
//! Quoted MENTIONS are exempt, by the shape PMAT-1453 arrived at after two
//! wrong attempts: a reporting verb plus an opening quote immediately before the
//! occurrence. A proximity window exempts the neighbourhood; global quote parity
//! is too fragile over a long document.

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Release-planning documents — DISCOVERED, not listed. Any doc under
/// `docs/specifications/sub/` whose name marks it a sprint/release plan, plus
/// the release runbook itself.
fn planning_docs() -> Vec<String> {
    let root = workspace_root();
    let mut out = vec!["docs/RELEASE.md".to_string()];
    let dir = root.join("docs/specifications/sub");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".md") && (n.contains("sprint") || n.contains("release")))
            .collect();
        names.sort();
        out.extend(
            names
                .into_iter()
                .map(|n| format!("docs/specifications/sub/{n}")),
        );
    }
    assert!(
        out.len() > 1,
        "no sprint/release planning document was discovered under docs/specifications/sub/ — the \
         corpus is empty and every rule below would range over nothing (PMAT-1396)"
    );
    out
}

/// Every tag already in the repository. DERIVED — no version is typed here.
fn shipped_tags() -> Vec<String> {
    let out = Command::new("git")
        .args(["tag", "--list", "v*"])
        .current_dir(workspace_root())
        .output()
        .expect("spawn git tag");
    let tags: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    assert!(
        !tags.is_empty(),
        "`git tag --list v*` returned nothing — this checkout has no tags, so the \
         already-shipped rule below would pass over an empty set"
    );
    tags
}

/// The version the queue says this sprint is releasing.
fn sprint_version() -> String {
    let body = read("docs/roadmaps/queue.yaml");
    let doc: serde_yaml::Value = serde_yaml::from_str(&body).expect("queue.yaml is valid YAML");
    doc.get("sprint")
        .and_then(|s| s.get("version"))
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_f64().map(|f| f.to_string()))
        })
        .expect("queue.yaml declares sprint.version")
}

/// A reporting verb plus an opening quote immediately before the occurrence —
/// the shape PMAT-1453 settled on after proximity and quote-parity both failed.
fn is_mention(hay: &str, at: usize) -> bool {
    let lead = &hay[at.saturating_sub(200)..at];
    let quoted = hay[at.saturating_sub(90)..at].contains('"')
        || hay[at.saturating_sub(90)..at].contains('`');
    let reported = lead.contains("Through v0.1.")
        || lead.contains("used to say")
        || lead.contains("it read")
        || lead.contains("this line said");
    quoted && reported
}

#[test]
fn no_plan_says_it_will_create_a_tag_that_already_exists() {
    // THE RULE THAT CATCHES THE EXPENSIVE DEFECT. `git tag v0.1.617` fails; with
    // `-f` it MOVES A PUBLISHED TAG. Derived from `git tag --list`, so it
    // sharpens by itself every time a release ships.
    let tags = shipped_tags();
    let mut offenders = Vec::new();
    for rel in planning_docs() {
        let body = read(&rel);
        for tag in &tags {
            let needle = format!("TAG: `{tag}`");
            let mut base = 0usize;
            while let Some(r) = body[base..].find(&needle) {
                let at = base + r;
                if !is_mention(&body, at) {
                    let line = body[..at].matches('\n').count() + 1;
                    offenders.push(format!(
                        "{rel}:{line}: declares `TAG: {tag}` — already shipped"
                    ));
                }
                base = at + needle.len();
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na release-planning document declares a tag that ALREADY EXISTS:\n  {}\n\
         `git tag <existing>` fails outright; `git tag -f` MOVES A PUBLISHED TAG. Shipped tags: \
         {tags:?}. The version this sprint is releasing is `{}` (queue.yaml sprint.version).",
        offenders.join("\n  "),
        sprint_version()
    );
}

#[test]
fn the_plan_declares_the_version_the_queue_is_releasing() {
    let want = sprint_version();
    let plans: Vec<String> = planning_docs()
        .into_iter()
        .filter(|p| p.contains("sprint"))
        .collect();
    assert!(!plans.is_empty(), "no sprint plan discovered");
    for rel in plans {
        let body = read(&rel);
        if !body.contains("**VERSION:") {
            continue;
        }
        let at = body.find("**VERSION:").expect("checked");
        let line_end = body[at..].find('\n').map(|e| at + e).unwrap_or(body.len());
        let line = &body[at..line_end];
        assert!(
            line.contains(&want),
            "{rel} declares {line:?} but queue.yaml's sprint.version is {want:?}. A plan naming \
             the PREVIOUS release is how the tag line came to name an already-shipped tag."
        );
    }
}

#[test]
fn no_planning_document_calls_the_version_bump_one_line() {
    // PMAT-1453's rule, with the corpus widened from two ledger files to the
    // whole class of release-planning documents. That slice fixed the runbook
    // and gated it over `queue.yaml`/`roadmap.yaml`; the sprint plan carried the
    // identical sentence and was outside the corpus.
    let manifest = read("Cargo.toml");
    let version = manifest
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("version = \"")
                .and_then(|r| r.split('"').next())
        })
        .expect("Cargo.toml declares a version");
    let needle = format!("version = \"{version}\"");
    let sites = manifest
        .lines()
        .filter(|l| l.contains(&needle) && !l.trim_start().starts_with('#'))
        .count();
    assert!(
        sites > 1,
        "Cargo.toml now assigns {version:?} on one line; the corrected plan text, which says the \
         bump is NOT one line, has become the stale claim"
    );

    let mut offenders = Vec::new();
    for rel in planning_docs() {
        let body = read(&rel);
        for pat in ["single-sourced at `Cargo.toml:", "One line + `cargo check`"] {
            let mut base = 0usize;
            while let Some(r) = body[base..].find(pat) {
                let at = base + r;
                if !is_mention(&body, at) {
                    let line = body[..at].matches('\n').count() + 1;
                    offenders.push(format!("{rel}:{line}: states {pat:?}"));
                }
                base = at + pat.len();
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "\na release-planning document describes the version bump as one line:\n  {}\n\
         Cargo.toml assigns the version on {sites} lines; a one-line bump leaves the \
         intra-workspace path-deps behind, which `cargo publish --dry-run` cannot see \
         (PMAT-1408, PMAT-1453).",
        offenders.join("\n  ")
    );
}

#[test]
fn the_corpus_reaches_the_sprint_plan() {
    // NON-VACUITY by anchor. If the discovery predicate stops matching, all
    // three rules pass over an empty corpus and go on passing (PMAT-1396).
    let docs = planning_docs();
    assert!(
        docs.iter().any(|d| d.contains("sprint-6day")),
        "the planning-document discovery found {docs:?} and did not reach the sprint plan"
    );
    assert!(
        docs.iter().any(|d| d == "docs/RELEASE.md"),
        "the planning-document corpus no longer includes docs/RELEASE.md: {docs:?}"
    );
}
