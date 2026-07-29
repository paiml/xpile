//! XPILE-SHELLPOLICY-001 — the shell-artifact POLICY gate (PMAT-1396).
//!
//! ## What this exists to catch
//!
//! `CLAUDE.md` — the file every agent session in this repo loads first — opened
//! its "Shell / Makefile / Dockerfile artifacts" section with
//!
//! ```text
//! xpile currently has zero `.sh` / `.bash` / `.zsh` / `Makefile` /
//! `Dockerfile` files.
//! ```
//!
//! That sentence was written by `da2cfff0` on 2026-05-17 11:19:33 and was TRUE
//! then. `e63b75f5` (#83, PMAT-052) added the first `.sh` at 19:33:05 the SAME
//! DAY. It stayed false for 71 days, and the same file names one of those
//! fixtures by filename 89 lines further down — the document contradicted
//! itself and nothing noticed, because a prose cardinality is not executable.
//!
//! The interesting part is not that the number was wrong. It is that the whole
//! bashrs-gate policy — "don't introduce shell artifacts without routing them
//! through bashrs" — was *conditioned* on that number, so the policy's own
//! premise had quietly stopped holding while the policy read as satisfied.
//!
//! ## Why this gate asserts an INVARIANT and never a COUNT
//!
//! Restating "11" in prose (or pinning `== 11` here) reproduces the defect in a
//! smaller font: the count grew by three on 2026-07-27 alone, and the next
//! fixture to land would re-falsify the doc and red this file for a reason that
//! has nothing to do with correctness. What is actually load-bearing is the
//! POLICY, and the policy is a property of each artifact, not of their number:
//!
//! 1. no `Makefile` / `Dockerfile` is tracked at all (this half of the original
//!    sentence is still true, and is worth keeping true);
//! 2. every tracked shell artifact lives under one of the two gated directories;
//! 3. every one of them is accepted by `bashrs-frontend`, so none is an opaque
//!    blob that the frontend cannot even read;
//! 4. the shell that comes back out `sh -n`-parses; and
//! 5. re-transpiling that output reproduces it byte-for-byte — the
//!    `C-BASHRS-POSIX-IDEMPOTENCE` fixed point, checked over the corpus rather
//!    than over a curated list someone has to remember to extend.
//!
//! Those five hold for whatever set `git ls-files` reports, today and after the
//! next fixture lands.
//!
//! ## What this gate deliberately does NOT do
//!
//! It does not EXECUTE the artifacts and diff their stdout, even though that is
//! the strongest available evidence and it was measured to hold for all 11
//! tracked files while writing this slice. Auto-enumeration plus auto-execution
//! means the REQUIRED `workspace-test` job would run whatever `.sh` a future
//! change happens to track, which is a worse hazard than the one being closed.
//! Execution equivalence stays where it is already pinned against expected
//! stdout for a curated set: `tests/shell_diff_exec.rs`.
//!
//! So this file's honest claim is *structural* round-trip over the whole
//! corpus, plus *executed* round-trip over the curated subset elsewhere — and
//! `CLAUDE.md` now says exactly that and no more.
//!
//! ## What acceptance does NOT prove (XPILE-SHELLPASS-001, PMAT-1479)
//!
//! Invariant 3 used to end "so none is an opaque blob sitting outside the
//! substrate-quality regime". The five invariants are unchanged and still hold;
//! that *therefore* was wrong. Twenty shell constructs outside the frontend's
//! enumerated surface are accepted at exit 0 and lower to `Stmt::Cmd` with the
//! operator carried as an opaque `Expr::LitStr` word — so acceptance,
//! `sh -n`-cleanliness and byte-identical re-emission are exactly what a
//! verbatim word gives you **for free**, and invariants 3–5 hold *vacuously*
//! over that class. `crates/xpile/examples/inputs/install.sh`, one of the
//! artifacts this gate certifies, ends `echo "done" > /tmp/out/install.log`.
//!
//! The lesson worth keeping: a gate can measure the right property and still
//! launder a stronger claim through the sentence that explains it. Class
//! membership is now pinned by
//! `crates/xpile/tests/shell_passthrough_disclosure_witness.rs`.
//!
//! ## Non-vacuity
//!
//! Assertion 1 is a NEGATIVE over an enumeration, which is the classic shape
//! that passes for free when the enumeration silently returns nothing. Every
//! test here therefore first requires the file list to contain a known-tracked
//! anchor (`Cargo.toml`), and the corpus test requires the shell corpus itself
//! to be non-empty. A missing `git` skips loudly with a reason rather than
//! reporting a checkmark it did not earn.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn xpile_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

/// Every path `git` tracks, repo-relative. `None` when the history is not
/// there (a packaged `.crate`, a host without `git`) — callers skip loudly.
fn tracked_files() -> Option<Vec<String>> {
    let out = Command::new("git")
        .args(["ls-files"])
        .current_dir(repo_root())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .collect();
    // The anchor. `git ls-files` succeeding with an empty or nonsense list
    // would make assertion 1 pass vacuously, so treat that as "no data".
    if !files.iter().any(|f| f == "Cargo.toml") {
        return None;
    }
    Some(files)
}

fn skip(reason: &str) {
    eprintln!("shell-artifact-policy: SKIP — {reason} (XPILE-SHELLPOLICY-001)");
}

/// `.sh` / `.bash` / `.zsh`, by extension.
fn is_shell_artifact(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|e| e.to_str()),
        Some("sh") | Some("bash") | Some("zsh")
    )
}

/// `Makefile` / `makefile` / `GNUmakefile` / `*.mk` / `Dockerfile*`, by
/// basename — the build-driver family the policy keeps at zero.
fn is_build_driver(path: &str) -> bool {
    let base = Path::new(path)
        .file_name()
        .and_then(|b| b.to_str())
        .unwrap_or("");
    base == "Makefile"
        || base == "makefile"
        || base == "GNUmakefile"
        || base.starts_with("Dockerfile")
        || base.ends_with(".mk")
}

/// The two directories CLAUDE.md declares as the gated home for shell
/// fixtures and examples.
const GATED_PREFIXES: [&str; 2] = [
    "crates/xpile/tests/fixtures/",
    "crates/xpile/examples/inputs/",
];

fn shell_corpus() -> Option<Vec<String>> {
    Some(
        tracked_files()?
            .into_iter()
            .filter(|f| is_shell_artifact(f))
            .collect(),
    )
}

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir()
        .join("xpile-shell-policy")
        .join(format!(
            "{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// `xpile transpile <path> --target shell`. `Ok(stdout)` on exit 0,
/// `Err(stderr)` otherwise.
fn transpile_shell(path: &Path) -> Result<String, String> {
    let out = Command::new(xpile_bin())
        .args([
            "transpile",
            path.to_str().expect("utf-8 path"),
            "--target",
            "shell",
        ])
        .output()
        .map_err(|e| format!("spawn xpile: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

fn have_sh() -> bool {
    Command::new("/bin/sh")
        .args(["-c", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Assertion 1 — the half of CLAUDE.md's original sentence that is still true.
#[test]
fn no_build_driver_artifact_is_tracked() {
    let Some(files) = tracked_files() else {
        skip("no git history / anchor file absent");
        return;
    };
    let drivers: Vec<&String> = files.iter().filter(|f| is_build_driver(f)).collect();
    assert!(
        drivers.is_empty(),
        "the policy keeps Makefile/Dockerfile at zero, but git tracks: {drivers:?} \
         (XPILE-SHELLPOLICY-001 — route it through bashrs or amend CLAUDE.md)"
    );
    eprintln!(
        "shell-artifact-policy: 0 build-driver artifacts across {} tracked files",
        files.len()
    );
}

/// Assertion 2 — location. A shell artifact outside the gated directories is
/// how ungated shell re-enters the repo.
#[test]
fn every_tracked_shell_artifact_lives_under_a_gated_directory() {
    let Some(corpus) = shell_corpus() else {
        skip("no git history / anchor file absent");
        return;
    };
    let stray: Vec<&String> = corpus
        .iter()
        .filter(|f| !GATED_PREFIXES.iter().any(|p| f.starts_with(p)))
        .collect();
    assert!(
        stray.is_empty(),
        "shell artifacts outside the gated directories {GATED_PREFIXES:?}: {stray:?} \
         (XPILE-SHELLPOLICY-001)"
    );
    eprintln!(
        "shell-artifact-policy: {} shell artifacts, all under {:?}",
        corpus.len(),
        GATED_PREFIXES
    );
}

/// Assertions 3-5 — every tracked shell artifact is accepted by the frontend,
/// the emitted shell parses, and re-emitting it is a fixed point.
#[test]
fn every_tracked_shell_artifact_round_trips_with_an_idempotent_emit() {
    let Some(corpus) = shell_corpus() else {
        skip("no git history / anchor file absent");
        return;
    };
    // Non-vacuity: this test is a universal quantification, so an empty corpus
    // would pass while proving nothing. The repo has carried shell fixtures
    // since 2026-05-17; an empty corpus means the enumeration broke.
    assert!(
        !corpus.is_empty(),
        "shell artifact corpus is EMPTY — the enumeration broke, or every fixture \
         was deleted; either way this gate would pass vacuously \
         (XPILE-SHELLPOLICY-001)"
    );

    let root = repo_root();
    let sh_available = have_sh();
    if !sh_available {
        skip("/bin/sh absent — the syntax half of this test is not run");
    }

    let mut refused: Vec<String> = Vec::new();
    let mut unparsable: Vec<String> = Vec::new();
    let mut non_idempotent: BTreeSet<String> = BTreeSet::new();

    for rel in &corpus {
        let src = root.join(rel);
        let first = match transpile_shell(&src) {
            Ok(shell) => shell,
            Err(e) => {
                refused.push(format!("{rel}: {}", e.replace('\n', " ")));
                continue;
            }
        };

        // Keep the basename: the emitted header carries `# module: <stem>`, so
        // re-emitting under a different name would differ for a reason that has
        // nothing to do with idempotence.
        let base = Path::new(rel)
            .file_name()
            .and_then(|b| b.to_str())
            .expect("utf-8 basename");
        let dir = scratch("rt");
        let first_path = dir.join(base);
        std::fs::write(&first_path, &first).expect("write emitted shell");

        if sh_available {
            let syntax = Command::new("/bin/sh")
                .arg("-n")
                .arg(&first_path)
                .output()
                .expect("spawn /bin/sh -n");
            if !syntax.status.success() {
                unparsable.push(format!(
                    "{rel}: {}",
                    String::from_utf8_lossy(&syntax.stderr)
                        .trim()
                        .replace('\n', " ")
                ));
            }
        }

        match transpile_shell(&first_path) {
            Ok(second) if second == first => {}
            Ok(_) => {
                non_idempotent.insert(rel.clone());
            }
            Err(e) => {
                non_idempotent.insert(format!("{rel} (re-emit refused: {})", e.replace('\n', " ")));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    assert!(
        refused.is_empty(),
        "tracked shell artifacts the bashrs frontend REFUSES — they are outside the \
         substrate-quality regime CLAUDE.md claims covers every language in the repo: \
         {refused:#?} (XPILE-SHELLPOLICY-001)"
    );
    assert!(
        unparsable.is_empty(),
        "emitted shell that `/bin/sh -n` rejects: {unparsable:#?} \
         (XPILE-SHELLPOLICY-001)"
    );
    assert!(
        non_idempotent.is_empty(),
        "re-emitting the emitted shell was not a fixed point, so \
         C-BASHRS-POSIX-IDEMPOTENCE does not hold over the tracked corpus: \
         {non_idempotent:#?} (XPILE-SHELLPOLICY-001)"
    );

    eprintln!(
        "shell-artifact-policy: {} artifacts round-tripped; emit is idempotent{}",
        corpus.len(),
        if sh_available {
            " and `sh -n`-clean"
        } else {
            " (sh -n NOT run)"
        }
    );
}

/// The doc half. CLAUDE.md's shell section must not carry the falsified
/// zero-artifact cardinality that this slice removed, and must point at this
/// gate so the next reader knows the claim is enforced rather than asserted.
#[test]
fn claude_md_shell_section_does_not_restate_a_cardinality() {
    let doc = std::fs::read_to_string(repo_root().join("CLAUDE.md")).expect("read CLAUDE.md");
    assert!(
        !doc.contains("has zero `.sh`"),
        "CLAUDE.md still claims xpile has zero `.sh` files; 11 were tracked when \
         PMAT-1396 removed that sentence (XPILE-SHELLPOLICY-001)"
    );
    assert!(
        doc.contains("shell_artifact_policy_witness.rs"),
        "CLAUDE.md's shell section must name the gate that enforces its policy, \
         otherwise the policy is prose again (XPILE-SHELLPOLICY-001)"
    );
}
