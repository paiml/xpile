//! XPILE-LEDGER-001 — the roadmap-REGISTRATION gate (PMAT-1345).
//!
//! ## What this exists to catch
//!
//! Every slice in this repo cites a `PMAT-NNNN` id in its commit SUBJECT (the
//! `commit-msg` hook enforces that much) and is supposed to register that id in
//! `docs/roadmaps/roadmap.yaml` in the SAME change. Nothing enforced the second
//! half, so five ids (PMAT-1268/1269/1277/1281/1283) shipped without ever being
//! registered and the drift went unnoticed until PMAT-1343 swept the ledger by
//! hand.
//!
//! The *mechanical* root cause was found in the same sweep: the local
//! `.git/hooks/pre-commit` "Documentation synchronization" step guards on
//!
//! ```text
//! if [ -f "docs/execution/roadmap.md" ] && [ -f "CHANGELOG.md" ]; then
//! ```
//!
//! and `docs/execution/roadmap.md` **does not exist in this repo** — the ledger
//! lives at `docs/roadmaps/roadmap.yaml`. The guard therefore took its `else`
//! branch (a non-fatal `⚠️` warning) on every commit ever made and has never
//! once fired. Repointing the hook fixes the local loop, but hooks live under
//! `.git/` and are **not cloned**, so a hook alone can never be the gate. This
//! test is the durable half: it runs inside the REQUIRED `workspace-test`
//! context, where no contributor and no cron firing can skip it.
//!
//! ## The three assertions
//!
//! 1. `every_done_queue_item_is_registered_in_the_roadmap_ledger` — pure text,
//!    no git, no runtime: every `docs/roadmaps/queue.yaml` entry marked
//!    `status: done` must appear as a `- id:` row in `docs/roadmaps/roadmap.yaml`.
//!    This one can never skip, so the gate is non-vacuous even in a shallow
//!    checkout or an extracted `.crate` with no history at all.
//! 2. `every_pmat_id_cited_since_the_release_tag_is_registered` — the machine
//!    form of the sprint's exit criterion 12: `comm -23` of (ids cited in commit
//!    subjects since the last `v*` tag) against (ids registered in the ledger)
//!    must be EMPTY. `.github/workflows/ci.yml` gives `workspace-test` a
//!    `fetch-depth: 0` checkout precisely so this runs for real in CI rather
//!    than skipping green; it skips-with-reason (loudly, on stderr) only where
//!    the history genuinely is not there — a shallow clone, a packaged crate, or
//!    a host without `git`.
//! 3. `the_pre_commit_documentation_guard_points_at_a_path_that_exists` — if a
//!    `pre-commit` hook is installed, every path it `[ -f ... ]`-guards must
//!    exist. That is what silently rotted; a guard on a dead path is strictly
//!    worse than no guard, because it reports a checkmark it never earned. Skips
//!    with a reason when no hook is installed (the CI case).
//!
//! Like `claims_drift.rs` and `witness_floor.rs` this is `std::fs` plus one
//! `git` subprocess — no new dependency and no runtime linkage, so `gate`
//! compiles it under `clippy --all-targets` and `workspace-test` runs it with
//! zero extra CI wiring.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Every `PMAT-NNNN` id registered in the ledger, i.e. every line that is
/// exactly a top-level `- id: PMAT-…` row of `docs/roadmaps/roadmap.yaml`.
fn registered_ids() -> BTreeSet<String> {
    read("docs/roadmaps/roadmap.yaml")
        .lines()
        .filter_map(|l| l.strip_prefix("- id: "))
        .map(str::trim)
        .filter(|id| id.starts_with("PMAT-"))
        .map(str::to_string)
        .collect()
}

/// Pull every `PMAT-NNNN` occurrence out of a line, in order.
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
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ── 1. Static: the queue's own DONE rows must be in the ledger ──────────────

#[test]
fn every_done_queue_item_is_registered_in_the_roadmap_ledger() {
    let registered = registered_ids();
    assert!(
        registered.len() > 1_000,
        "docs/roadmaps/roadmap.yaml parsed to only {} ids — the `- id: ` row \
         format changed and this gate has gone blind",
        registered.len()
    );

    // Walk queue.yaml pairing each `- id: PMAT-…` with the `status:` that
    // follows it. Non-PMAT ids (the `owner_decisions` block) clear the pairing,
    // so their fields are never mis-attributed to the preceding slice.
    let queue = read("docs/roadmaps/queue.yaml");
    let mut current: Option<String> = None;
    let mut missing: Vec<String> = Vec::new();
    for line in queue.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- id: ") {
            let id = rest.trim();
            current = id.starts_with("PMAT-").then(|| id.to_string());
        } else if let Some(rest) = t.strip_prefix("status: ") {
            if let Some(id) = current.take() {
                if rest.trim() == "done" && !registered.contains(&id) {
                    missing.push(id);
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "docs/roadmaps/queue.yaml marks {} item(s) `status: done` that are NOT \
         registered in docs/roadmaps/roadmap.yaml: {}\n\n\
         Registration is part of the slice: append a `- id: <ID>` entry to \
         docs/roadmaps/roadmap.yaml in the SAME change that flips the queue row \
         to done.",
        missing.len(),
        missing.join(", ")
    );
}

// ── 2. History: no cited id may be missing from the ledger ──────────────────

#[test]
fn every_pmat_id_cited_since_the_release_tag_is_registered() {
    if git(&["rev-parse", "--git-dir"]).is_none() {
        eprintln!(
            "warning: no git repository (or no `git` on PATH); skipping the \
             commit-subject half of XPILE-LEDGER-001. The static queue half \
             still ran."
        );
        return;
    }
    if git(&["rev-parse", "--is-shallow-repository"]).as_deref() == Some("true") {
        eprintln!(
            "warning: shallow clone; skipping the commit-subject half of \
             XPILE-LEDGER-001. Give the job a `fetch-depth: 0` checkout to run \
             it for real (`.github/workflows/ci.yml`, job `workspace-test`)."
        );
        return;
    }
    // Range: since the most recent release tag. Without tags there is nothing
    // to anchor to, so say so rather than silently scanning all of history.
    let Some(tag) = git(&["describe", "--tags", "--abbrev=0", "--match", "v*"]) else {
        eprintln!(
            "warning: no `v*` tag reachable from HEAD; skipping the \
             commit-subject half of XPILE-LEDGER-001 (a `fetch-depth: 0` \
             checkout fetches tags too)."
        );
        return;
    };
    let Some(log) = git(&["log", &format!("{tag}..HEAD"), "--format=%s"]) else {
        eprintln!("warning: `git log {tag}..HEAD` failed; skipping the commit-subject half.");
        return;
    };

    let registered = registered_ids();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for subject in log.lines() {
        for id in pmat_ids_in(subject) {
            if !registered.contains(&id) {
                missing.insert(id);
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} PMAT id(s) are cited in commit subjects since {tag} but are NOT \
         registered in docs/roadmaps/roadmap.yaml: {}\n\n\
         This is the id drift PMAT-1343 had to repair by hand. Register the id \
         in the SAME change that cites it — append a `- id: <ID>` entry to \
         docs/roadmaps/roadmap.yaml. Reproduce outside cargo with:\n  \
         comm -23 <(git log {tag}..HEAD --format=%s | grep -oE 'PMAT-[0-9]+' | sort -u) \\\n       \
              <(grep -oE '^- id: PMAT-[0-9]+' docs/roadmaps/roadmap.yaml | grep -oE 'PMAT-[0-9]+' | sort -u)",
        missing.len(),
        missing.iter().cloned().collect::<Vec<_>>().join(", ")
    );
}

// ── 3. The hook itself: no guard may name a path that does not exist ────────

#[test]
fn the_pre_commit_documentation_guard_points_at_a_path_that_exists() {
    let Some(common) = git(&["rev-parse", "--git-common-dir"]) else {
        eprintln!(
            "warning: no git repository (or no `git` on PATH); skipping the \
             pre-commit hook check of XPILE-LEDGER-001."
        );
        return;
    };
    // `--git-common-dir` is relative to the worktree root when the repo is the
    // primary checkout, absolute for a linked worktree. Normalise both.
    let common = {
        let p = PathBuf::from(&common);
        if p.is_absolute() {
            p
        } else {
            workspace_root().join(p)
        }
    };
    let hook = common.join("hooks").join("pre-commit");
    if !hook.is_file() {
        eprintln!(
            "warning: no pre-commit hook installed at {}; skipping. Hooks are \
             not cloned, which is exactly why the ledger gate above does not \
             depend on one.",
            hook.display()
        );
        return;
    }

    let text = fs::read_to_string(&hook).unwrap_or_else(|e| panic!("read {}: {e}", hook.display()));
    let root = workspace_root();
    let mut dead: Vec<String> = Vec::new();
    for line in text.lines() {
        for guarded in file_test_paths(line) {
            // Only repo-relative doc/source paths are checkable; anything with
            // a shell expansion in it is left alone.
            if guarded.contains('$') || guarded.starts_with('/') {
                continue;
            }
            if !Path::new(&root).join(&guarded).exists() {
                dead.push(format!("{guarded}  (guarded at: {})", line.trim()));
            }
        }
    }

    assert!(
        dead.is_empty(),
        "the pre-commit hook `[ -f ... ]`-guards {} path(s) that do NOT exist, \
         so the guard silently takes its else-branch and reports a check it \
         never performed:\n  {}\n\n\
         This is precisely how the roadmap-completeness guard rotted \
         (`docs/execution/roadmap.md` was guarded for months; the ledger lives \
         at `docs/roadmaps/roadmap.yaml`). Repoint the guard at a real path.",
        dead.len(),
        dead.join("\n  ")
    );
}

/// Extract the operand of every `[ -f "PATH" ]` / `[ -f PATH ]` file test on a
/// line. Anchored on the `[` so that `rm -f …` and `grep -f …` are not mistaken
/// for existence guards.
const FILE_TEST: &str = "[ -f ";

fn file_test_paths(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    while let Some(rel) = line[idx..].find(FILE_TEST) {
        let operand_at = idx + rel + FILE_TEST.len();
        let after = &line[operand_at..];
        let lead = after.len() - after.trim_start().len();
        let after = after.trim_start();
        let (raw, width) = match after.strip_prefix('"') {
            Some(quoted) => match quoted.find('"') {
                Some(end) => (&quoted[..end], end + 2),
                None => break,
            },
            None => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], end)
            }
        };
        if !raw.is_empty() {
            out.push(raw.to_string());
        }
        idx = (operand_at + lead + width).max(idx + 1);
    }
    out
}
