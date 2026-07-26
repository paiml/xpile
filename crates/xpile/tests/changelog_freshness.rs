//! XPILE-CHANGELOG-001 — the CHANGELOG-FRESHNESS gate (PMAT-1356).
//!
//! ## What this exists to catch
//!
//! `v0.1.617` came within hours of shipping with `## [Unreleased]` **empty**
//! against **108** landed commits. Nothing in `crates/` or `.github/workflows/`
//! read `CHANGELOG.md`, so an empty release note was not a failure — it was
//! silence. The `0.1.616` entry is 404 lines, but its release commit
//! contributed only `+46`: the other ~358 lines came from ~10 incremental
//! commits *during* the cycle. The 0.1.617 cycle wrote **0 of 102**. The
//! standing constraint for this window is "write the CHANGELOG incrementally",
//! and this test is what makes that constraint *enforced* rather than
//! remembered.
//!
//! ## The "active region", and why it is not literally `[Unreleased]`
//!
//! A gate anchored on the literal string `## [Unreleased]` would red the one
//! commit that most needs to pass: the release commit promotes
//! `## [Unreleased]` to `## [0.1.618] - 2026-07-31`, at which point no
//! `[Unreleased]` heading exists at all. So the history assertion instead
//! anchors on the **last released version** — the `v*` git tag — and treats
//! everything above that version's heading as the region that must describe
//! work landed since it. That region is:
//!
//! - the `[Unreleased]` section during a cycle,
//! - the promoted `[0.1.618]` section in the release commit (same content,
//!   renamed heading — still above `## [0.1.617]`, still passes),
//! - and, once `v0.1.618` is tagged, the *new* `[Unreleased]` section that the
//!   next cycle's first slice is thereby forced to create.
//!
//! Self-maintaining, and it never punishes the release.
//!
//! ## The three assertions
//!
//! 1. `unreleased_section_is_never_an_empty_heading` — pure text, no git, no
//!    runtime, cannot skip: if a `## [Unreleased]` heading is present its body
//!    must carry at least one `###` subheading, one top-level bullet and one
//!    `PMAT-` citation. This is the *literal* v0.1.617 near-miss shape, and it
//!    still runs in a shallow checkout or an extracted `.crate` with no history.
//! 2. `leading_heading_tracks_the_workspace_version` — also pure text: the
//!    first heading must be either `[Unreleased]` or `[<workspace version>]`,
//!    and when it is `[Unreleased]` the workspace version must already appear
//!    as a released heading below. This couples `Cargo.toml` to `CHANGELOG.md`
//!    in both directions, which is exactly the pair the release commit edits
//!    together: bumping the version without promoting the heading reds, and
//!    promoting to a version `Cargo.toml` does not carry reds.
//! 3. `every_id_shipped_since_the_release_tag_is_described` — the assertion
//!    with teeth. Every `PMAT-NNNN` cited in a commit SUBJECT since the last
//!    `v*` tag whose commit touched **shipped source** must be named in the
//!    active region. `workspace-test` already has the `fetch-depth: 0` checkout
//!    PMAT-1345 added for XPILE-LEDGER-001, so this runs for real in CI rather
//!    than skipping green.
//!
//! ## Scope of assertion 3, stated honestly
//!
//! "Shipped source" is `crates/*/src/**` and `contracts/**.yaml` — the surface
//! a consumer of the published crates actually receives. Commits that touch
//! only tests, CI, or docs are **not** required to appear. That is a deliberate
//! lower bound, not an oversight: requiring an entry for every test-only slice
//! would make the gate noisy enough to get loosened, and a loosened gate is
//! worth less than a narrow one that holds. Entries for test/CI/docs slices are
//! welcome and several exist — the gate simply does not compel them.
//!
//! The first run of this gate found `PMAT-1350` — the WASM-contract
//! reconciliation, a `contracts/*.yaml` change that landed 15 minutes *after*
//! the `v0.1.617` tag — with no CHANGELOG entry anywhere in the file. That
//! entry ships in the same commit as this test.
//!
//! Like `claims_drift.rs`, `roadmap_registration.rs` and `witness_floor.rs`
//! this is `std::fs` plus one `git` subprocess — no new dependency and no
//! runtime linkage.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// A `## [Unreleased]` body must clear all three of these. They are a
/// "not empty" bar, NOT a size ratchet: the first slice of a new cycle
/// legitimately adds exactly one bullet, and a floor of 3 would red it. The
/// strength of this gate scales through assertion 3, which grows with the work
/// actually landed, not through padding these numbers.
const MIN_SUBHEADINGS: usize = 1;
const MIN_BULLETS: usize = 1;
const MIN_CITATIONS: usize = 1;

/// Below this the heading scanner has gone blind and every other assertion in
/// the file is vacuous. Live count is in the hundreds.
const MIN_HEADINGS: usize = 5;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `## [label]` heading as `(line index, label)`, skipping any that sit
/// inside a fenced code block — `CHANGELOG.md` carries ~100 fences and a
/// documented example of a changelog heading would otherwise be read as a real
/// section boundary and silently truncate the active region.
fn headings(md: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, line) in md.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(rest) = line.strip_prefix("## [") {
            if let Some(end) = rest.find(']') {
                out.push((i, rest[..end].trim().to_string()));
            }
        }
    }
    out
}

/// The body of the heading at `idx`, i.e. every line after it up to the next
/// heading (or EOF).
fn section_body(md: &str, hs: &[(usize, String)], idx: usize) -> String {
    let start = hs[idx].0 + 1;
    let end = hs.get(idx + 1).map(|(l, _)| *l).unwrap_or(usize::MAX);
    md.lines()
        .enumerate()
        .filter(|(i, _)| *i >= start && *i < end)
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `version = "…"` from the `[workspace.package]` table of the root manifest.
fn workspace_version() -> String {
    let manifest = read("Cargo.toml");
    let mut in_table = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_table = t == "[workspace.package]";
            continue;
        }
        if in_table {
            if let Some(rest) = t.strip_prefix("version") {
                if let Some(v) = rest.split('"').nth(1) {
                    return v.to_string();
                }
            }
        }
    }
    panic!("Cargo.toml has no [workspace.package] version — the manifest layout changed");
}

/// Pull every `PMAT-NNNN` occurrence out of `text`, in order.
fn pmat_ids_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = text[i..].find("PMAT-") {
        let start = i + rel;
        let mut end = start + "PMAT-".len();
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > start + "PMAT-".len() {
            out.push(text[start..end].to_string());
        }
        i = end.max(start + 1);
    }
    out
}

/// A path a consumer of the published crates actually receives.
fn is_shipped_source(path: &str) -> bool {
    let crate_src = path.starts_with("crates/")
        && path
            .strip_prefix("crates/")
            .and_then(|r| r.split_once('/'))
            .is_some_and(|(_, rest)| rest.starts_with("src/"));
    let contract = path.starts_with("contracts/") && path.ends_with(".yaml");
    crate_src || contract
}

/// Run `git` in the workspace root; `None` when git is missing or the command
/// fails (no repository, shallow history, unknown revision, …).
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace_root())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Skip the history half loudly, and REFUSE to skip when the caller has
/// declared that history must be there. Mirrors `XPILE_REQUIRE_RULESET_CHECK`
/// (PMAT-1347) and `XPILE_REQUIRE_KANI` (RULESET-002): a skip that cannot be
/// turned into a failure is indistinguishable from a pass.
fn skip_history(reason: &str) {
    if std::env::var("XPILE_REQUIRE_CHANGELOG_HISTORY").as_deref() == Ok("1") {
        panic!(
            "XPILE_REQUIRE_CHANGELOG_HISTORY=1 but the history half of \
             XPILE-CHANGELOG-001 cannot run: {reason}"
        );
    }
    eprintln!(
        "warning: skipping the history half of XPILE-CHANGELOG-001: {reason}\n\
         (the two static halves still ran; set XPILE_REQUIRE_CHANGELOG_HISTORY=1 \
         to make this a hard failure)"
    );
}

// ── 1. Static: an `[Unreleased]` heading may never stand empty ──────────────

#[test]
fn unreleased_section_is_never_an_empty_heading() {
    let md = read("CHANGELOG.md");
    let hs = headings(&md);
    assert!(
        hs.len() >= MIN_HEADINGS,
        "CHANGELOG.md parsed to only {} `## [...]` headings (floor {MIN_HEADINGS}) — \
         the heading format changed and XPILE-CHANGELOG-001 has gone blind",
        hs.len()
    );

    let Some(idx) = hs.iter().position(|(_, label)| label == "Unreleased") else {
        // Legitimate only in the release commit, between promoting the heading
        // and the next cycle's first entry. Assertion 3 is what forces the
        // section back into existence once new work lands.
        eprintln!(
            "note: CHANGELOG.md has no `## [Unreleased]` heading — expected only \
             immediately after a release commit."
        );
        return;
    };

    let body = section_body(&md, &hs, idx);
    let subheadings = body.lines().filter(|l| l.starts_with("### ")).count();
    let bullets = body.lines().filter(|l| l.starts_with("- ")).count();
    let citations = pmat_ids_in(&body).len();

    assert!(
        subheadings >= MIN_SUBHEADINGS && bullets >= MIN_BULLETS && citations >= MIN_CITATIONS,
        "`## [Unreleased]` in CHANGELOG.md is not substantive: {subheadings} `###` \
         subheading(s) (need {MIN_SUBHEADINGS}), {bullets} top-level bullet(s) \
         (need {MIN_BULLETS}), {citations} `PMAT-` citation(s) (need \
         {MIN_CITATIONS}).\n\n\
         This is the exact shape that nearly shipped v0.1.617: an `[Unreleased]` \
         heading with NOTHING under it against 108 landed commits, in a repo \
         where nothing else read the file. Write the entry in the SAME change \
         that ships the work — the 0.1.616 note reached 404 lines through ~10 \
         incremental commits, not one release-day sprint."
    );
}

// ── 2. Static: the leading heading and the workspace version must agree ─────

#[test]
fn leading_heading_tracks_the_workspace_version() {
    let md = read("CHANGELOG.md");
    let hs = headings(&md);
    assert!(
        hs.len() >= MIN_HEADINGS,
        "heading scanner blind ({})",
        hs.len()
    );
    let version = workspace_version();
    let (_, leading) = &hs[0];

    assert!(
        leading == "Unreleased" || *leading == version,
        "CHANGELOG.md's first heading is `## [{leading}]` but Cargo.toml \
         [workspace.package] version is `{version}`.\n\n\
         The first heading must be either `[Unreleased]` (mid-cycle) or \
         `[{version}]` (the release commit, which bumps the version and promotes \
         the heading TOGETHER). A mismatch means one of the two edits was made \
         without the other."
    );

    if leading == "Unreleased" {
        assert!(
            hs.iter().skip(1).any(|(_, l)| *l == version),
            "CHANGELOG.md leads with `## [Unreleased]` and Cargo.toml carries \
             version `{version}`, but no `## [{version}]` heading exists below \
             it.\n\n\
             Either the version was bumped without promoting the previous \
             `[Unreleased]` section, or the released version's section was \
             deleted. Both leave shipped work undocumented."
        );
    }
}

// ── 3. History: everything shipped since the tag must be described ──────────

#[test]
fn every_id_shipped_since_the_release_tag_is_described() {
    if git(&["rev-parse", "--git-dir"]).is_none() {
        skip_history("no git repository (or no `git` on PATH)");
        return;
    }
    if git(&["rev-parse", "--is-shallow-repository"]).as_deref() == Some("true") {
        skip_history(
            "shallow clone — give the job a `fetch-depth: 0` checkout \
             (.github/workflows/ci.yml, job `workspace-test`)",
        );
        return;
    }
    let Some(tag) = git(&["describe", "--tags", "--abbrev=0", "--match", "v*"]) else {
        skip_history("no `v*` tag reachable from HEAD (a `fetch-depth: 0` checkout fetches tags)");
        return;
    };
    // `git log <tag>..HEAD --format=%x01%s --name-only`: one `\x01`-prefixed
    // subject per commit, followed by that commit's paths.
    let Some(log) = git(&[
        "log",
        &format!("{tag}..HEAD"),
        "--format=%x01%s",
        "--name-only",
    ]) else {
        skip_history(&format!("`git log {tag}..HEAD` failed"));
        return;
    };

    let released = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let md = read("CHANGELOG.md");
    let hs = headings(&md);
    let Some(cut) = hs.iter().position(|(_, l)| *l == released) else {
        panic!(
            "the last release tag is `{tag}` but CHANGELOG.md has no \
             `## [{released}]` heading — the released version is undocumented, so \
             there is no boundary for 'work landed since the release'."
        );
    };
    // Everything above the released version's heading: `[Unreleased]`
    // mid-cycle, or the promoted `[<next>]` section in the release commit.
    let active: String = md.lines().take(hs[cut].0).collect::<Vec<_>>().join("\n");

    // Walk the log, crediting each subject's ids only when that commit touched
    // shipped source.
    let mut required: BTreeSet<String> = BTreeSet::new();
    let mut subject = String::new();
    let mut shipped = false;
    let mut commits = 0usize;
    fn flush(subject: &str, shipped: bool, required: &mut BTreeSet<String>) {
        if shipped {
            required.extend(pmat_ids_in(subject));
        }
    }
    for line in log.lines() {
        if let Some(s) = line.strip_prefix('\u{1}') {
            flush(&subject, shipped, &mut required);
            subject = s.to_string();
            shipped = false;
            commits += 1;
        } else if !line.trim().is_empty() && is_shipped_source(line.trim()) {
            shipped = true;
        }
    }
    flush(&subject, shipped, &mut required);

    let missing: Vec<String> = required
        .iter()
        .filter(|id| !active.contains(id.as_str()))
        .cloned()
        .collect();

    assert!(
        missing.is_empty(),
        "{} PMAT id(s) changed shipped source since {tag} but are NOT named \
         anywhere above `## [{released}]` in CHANGELOG.md: {}\n\n\
         ({commits} commit(s) scanned; 'shipped source' = crates/*/src/** and \
         contracts/**.yaml — the surface a consumer of the published crates \
         receives.)\n\n\
         Write the entry in the SAME change that ships the work. Reproduce \
         outside cargo with:\n  \
         git log {tag}..HEAD --format='%h %s' --name-only",
        missing.len(),
        missing.join(", ")
    );
}

// ── Anti-vacuity unit tests for the parsing primitives ──────────────────────

#[cfg(test)]
mod changelog_tests {
    use super::*;

    const SAMPLE: &str = "\
# Changelog

## [Unreleased]

### Fixed

- **a thing** (PMAT-1234) — with a
  continuation line that is not a bullet.

## [0.1.617] - 2026-07-26

### Added

- old news (PMAT-1)
";

    #[test]
    fn headings_are_found_in_order_with_their_labels() {
        let hs = headings(SAMPLE);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].1, "Unreleased");
        assert_eq!(hs[1].1, "0.1.617");
    }

    #[test]
    fn a_heading_inside_a_code_fence_is_not_a_section_boundary() {
        // A changelog that DOCUMENTS the changelog format must not have its
        // active region silently truncated at the example.
        let md =
            "## [Unreleased]\n\n```text\n## [9.9.9] - fake\n```\n\n- real (PMAT-7)\n\n## [0.1.0]\n";
        let hs = headings(md);
        assert_eq!(
            hs.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
            vec!["Unreleased", "0.1.0"],
            "the fenced `## [9.9.9]` must not be read as a heading"
        );
        assert!(section_body(md, &hs, 0).contains("real (PMAT-7)"));
    }

    #[test]
    fn section_body_stops_at_the_next_heading() {
        let hs = headings(SAMPLE);
        let body = section_body(SAMPLE, &hs, 0);
        assert!(body.contains("PMAT-1234"));
        // NB: assert on the released section's distinctive PROSE, not on
        // `PMAT-1` — that is a substring of `PMAT-1234` and the check would
        // pass no matter how badly `section_body` leaked.
        assert!(
            !body.contains("old news"),
            "the released section leaked into the unreleased body: {body}"
        );
        assert!(!pmat_ids_in(&body).contains(&"PMAT-1".to_string()));
    }

    #[test]
    fn only_top_level_bullets_count() {
        let hs = headings(SAMPLE);
        let body = section_body(SAMPLE, &hs, 0);
        assert_eq!(
            body.lines().filter(|l| l.starts_with("- ")).count(),
            1,
            "an indented continuation line must not be credited as a bullet"
        );
    }

    #[test]
    fn an_empty_unreleased_body_fails_every_substantive_floor() {
        let md = "# Changelog\n\n## [Unreleased]\n\n## [0.1.617] - 2026-07-26\n\n- x (PMAT-1)\n";
        let hs = headings(md);
        let body = section_body(md, &hs, 0);
        assert_eq!(body.lines().filter(|l| l.starts_with("### ")).count(), 0);
        assert_eq!(body.lines().filter(|l| l.starts_with("- ")).count(), 0);
        assert_eq!(pmat_ids_in(&body).len(), 0);
    }

    #[test]
    fn shipped_source_classifier_admits_src_and_contracts_only() {
        assert!(is_shipped_source("crates/bashrs-frontend/src/lib.rs"));
        assert!(is_shipped_source("contracts/compile-rust-to-wasm-v1.yaml"));
        // Deliberately NOT required (documented in the module header):
        assert!(!is_shipped_source("crates/xpile/tests/witness_floor.rs"));
        assert!(!is_shipped_source(
            "crates/xpile-wasm-codegen/tests/x_witness.rs"
        ));
        assert!(!is_shipped_source(".github/workflows/ci.yml"));
        assert!(!is_shipped_source("docs/roadmaps/queue.yaml"));
        assert!(!is_shipped_source("CHANGELOG.md"));
        // A `src` directory that is not a crate's own must not be admitted.
        assert!(!is_shipped_source("src/lib.rs"));
    }

    #[test]
    fn pmat_ids_are_extracted_without_the_trailing_prose() {
        assert_eq!(
            pmat_ids_in("fix(a): thing (Refs PMAT-1356) and PMAT-99x"),
            vec!["PMAT-1356".to_string(), "PMAT-99".to_string()]
        );
        assert!(pmat_ids_in("PMAT- with no digits").is_empty());
    }

    #[test]
    fn the_workspace_version_parses_to_a_dotted_triple() {
        let v = workspace_version();
        assert_eq!(
            v.split('.').count(),
            3,
            "workspace version `{v}` is not a dotted triple — the manifest parse drifted"
        );
    }
}
