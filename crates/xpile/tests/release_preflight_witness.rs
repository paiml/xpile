//! XPILE-RELEASE-PREFLIGHT-001 — every anti-vacuity tripwire is armed by
//! something that exists (PMAT-1416).
//!
//! ## What this exists to catch
//!
//! `crates/xpile/tests/ruleset_drift.rs`, inside the skip branch of
//! `live_ruleset_matches_the_committed_snapshot` — a test that runs in the
//! REQUIRED `workspace-test` job — said this:
//!
//! ```text
//! // Anti-vacuity tripwire. Reading an ORG ruleset needs a token with
//! // org scope, which Actions' repo-scoped GITHUB_TOKEN does not have —
//! // so this test legitimately skips in CI and the STATIC half above is
//! // what runs there. The release pre-flight (docs/RELEASE.md) sets
//! // XPILE_REQUIRE_RULESET_CHECK=1 to refuse the skip.
//! ```
//!
//! `docs/RELEASE.md` **did not exist**. `git ls-files | grep -i release`
//! returned exactly one path, `.github/workflows/release.yml`. No tracked
//! file, no workflow, and not the sprint driver set
//! `XPILE_REQUIRE_RULESET_CHECK` — so the enforcement claim skipped green in
//! every automated run it had ever had, and the refusal mechanism its own
//! comment named as the compensating control had never once run.
//!
//! That is the shape this repo keeps finding: **a disclosed skip standing in
//! front of a mechanism that does not exist is still a false pass.** The skip
//! is honest about skipping and dishonest about what covers it, which is worse
//! than an undisclosed skip, because it reads as mitigated.
//!
//! It was not hypothetical. The org ruleset drifted on 2026-07-27 —
//! `workspace-test` dropped out of the required set, so a PR could merge with
//! it red — and what noticed was a cron fire that happened to hold an
//! org-scoped token, not the pre-flight the comment credits.
//!
//! ## The measurement widened it
//!
//! The citation names one tripwire. Deriving the whole set from the corpus
//! found eight `XPILE_REQUIRE_*` vars read by tests, of which only
//! `WASM_RUNTIME`, `KANI` and `DENY` were armed anywhere at all. The other
//! five — `RULESET_CHECK`, `CC`, `SH`, `RUCHY`, `CHANGELOG_HISTORY` — were
//! armed by nothing in the repo, so the anti-vacuity half of five separate
//! witnesses was unreachable. Four of the five pass when armed (measured, not
//! assumed, before `docs/RELEASE.md` was written); the fifth,
//! `RULESET_CHECK`, reds on the live drift above, which is the point.
//!
//! ## The invariant, and why it is not a count
//!
//! No number appears in this file. The tripwire set is re-derived from
//! `git ls-files` on every run, so a witness that adds a ninth tripwire is
//! covered the day it lands rather than the day someone remembers to update a
//! list. What is asserted is the property that was actually violated:
//!
//! 1. every tripwire the corpus READS is ARMED — by a workflow, or by a
//!    runnable command in `docs/RELEASE.md`;
//! 2. `docs/RELEASE.md` arms no tripwire that nothing reads (a phantom in the
//!    doc is the same false green pointing the other way); and
//! 3. any test source that CITES `docs/RELEASE.md` as its arming mechanism has
//!    the tripwires it reads actually armed there — the exact broken citation.
//!
//! ## What this gate deliberately does NOT do
//!
//! It does not RUN the pre-flight. Arming `XPILE_REQUIRE_RULESET_CHECK`
//! requires an org-scoped credential that CI does not have, and arming
//! `XPILE_REQUIRE_RUCHY` requires a toolchain `workspace-test` deliberately
//! does not install — so a gate that executed the pre-flight would itself skip
//! in CI, reproducing the defect one level up. Test 4 is therefore a
//! STRUCTURAL presence check over the procedure document: it can prove the
//! sections and the arming commands are there, and it cannot prove the prose
//! around them is true. The executable claim is 1–3.
//!
//! ## Self-reference, and why the corpus is TRACKED files
//!
//! The scan reads what `git ls-files` reports, so a witness is invisible to it
//! until it is committed — including this one. That bit immediately: the first
//! draft's doc comment illustrated the read spelling with a literal
//! `env::var_os` call on a placeholder name, which is indistinguishable from a
//! real read site, and the moment this file became tracked the gate reported a
//! tripwire armed by nothing. The prose example is gone and no literal of that
//! spelling appears in this file. Two things follow, both worth keeping: a new
//! witness's tripwire is checked from the commit that adds it and not before,
//! and a gate that scans source for call sites must not print call sites.
//!
//! ## Non-vacuity
//!
//! Tests 1–3 are universals over a derived set, the shape that passes for free
//! when the derivation silently returns nothing. Each therefore first requires
//! its set to be non-empty and to contain a known anchor. A missing `git`
//! skips loudly with a reason rather than reporting a checkmark it did not
//! earn.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

const RELEASE_DOC: &str = "docs/RELEASE.md";

/// A tripwire that is known to be read by the corpus today. Used only as an
/// anti-vacuity anchor — the asserted set is always derived, never this.
const ANCHOR_TRIPWIRE: &str = "XPILE_REQUIRE_WASM_RUNTIME";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
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
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect(),
    )
}

fn read_tracked(rel: &str) -> Option<String> {
    std::fs::read_to_string(repo_root().join(rel)).ok()
}

/// Pull every `XPILE_REQUIRE_*` name out of `text`, starting at each occurrence
/// of `marker`. `marker` is what distinguishes a READ site from prose: a read
/// passes the name to `env::var` / `env::var_os` as a string literal, so it is
/// preceded by an open paren and a quote.
///
/// Deliberately no literal example of that spelling appears anywhere in this
/// file — see the module header's note on self-reference.
fn scan_names(text: &str, marker: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (idx, _) in text.match_indices(marker) {
        let rest = &text[idx + marker.len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || *c == '_' || c.is_ascii_digit())
            .collect();
        // `name` is the SUFFIX after the `XPILE_REQUIRE_` the marker consumed,
        // so the only thing to reject is a bare `XPILE_REQUIRE_` with nothing
        // after it.
        if !name.is_empty() {
            out.insert(format!("XPILE_REQUIRE_{name}"));
        }
    }
    out
}

/// The tripwires a single Rust source READS — i.e. passes as a string literal
/// to `env::var` / `env::var_os`. Prose mentions in doc comments are not reads
/// and are deliberately not counted.
fn tripwires_read_by(src: &str) -> BTreeSet<String> {
    scan_names(src, "(\"XPILE_REQUIRE_")
}

/// Union of `tripwires_read_by` over every tracked Rust source.
fn tripwires_read_by_the_corpus(files: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in files.iter().filter(|f| f.ends_with(".rs")) {
        if let Some(text) = read_tracked(f) {
            out.extend(tripwires_read_by(&text));
        }
    }
    out
}

/// The tripwires a CI workflow ARMS. Comment lines are stripped first: `ci.yml`
/// *describes* `XPILE_REQUIRE_WASM_RUNTIME=1` in a comment several jobs away
/// from where it actually sets it, and counting prose as arming would let a
/// deleted `env:` key pass on the strength of the comment that survived it.
fn tripwires_armed_by_workflows(files: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in files
        .iter()
        .filter(|f| f.starts_with(".github/workflows/") && f.ends_with(".yml"))
    {
        let Some(text) = read_tracked(f) else {
            continue;
        };
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        // A YAML `env:` key (`XPILE_REQUIRE_X: "1"`) or a shell assignment.
        out.extend(
            scan_names(&code, "XPILE_REQUIRE_")
                .into_iter()
                .filter(|n| code.contains(&format!("{n}: ")) || code.contains(&format!("{n}=1"))),
        );
    }
    out
}

/// The tripwires `docs/RELEASE.md` ARMS. Only a runnable `NAME=1` counts — a
/// name listed in the document's table is documentation, not a mechanism, and
/// this gate exists precisely because a description of a mechanism had been
/// standing in for one.
fn tripwires_armed_by_the_release_doc() -> Option<BTreeSet<String>> {
    let text = read_tracked(RELEASE_DOC)?;
    Some(
        scan_names(&text, "XPILE_REQUIRE_")
            .into_iter()
            .filter(|n| text.contains(&format!("{n}=1")))
            .collect(),
    )
}

fn skip(why: &str) -> bool {
    eprintln!("SKIP: XPILE-RELEASE-PREFLIGHT-001 — {why}");
    true
}

/// The defect, stated as a universal: a tripwire nobody arms is a witness whose
/// anti-vacuity half cannot run.
#[test]
fn every_tripwire_the_corpus_reads_is_armed_somewhere() {
    let Some(files) = tracked_files() else {
        assert!(skip("no git history — cannot enumerate the corpus"));
        return;
    };
    let read = tripwires_read_by_the_corpus(&files);
    assert!(
        read.contains(ANCHOR_TRIPWIRE),
        "anti-vacuity: the derived read-set does not contain {ANCHOR_TRIPWIRE}, so the \
         scan found nothing and this universal would pass for free. Derived: {read:?}"
    );

    let armed_ci = tripwires_armed_by_workflows(&files);
    let armed_doc = tripwires_armed_by_the_release_doc().unwrap_or_else(|| {
        panic!(
            "{RELEASE_DOC} is missing. It is cited by name from test source as the place \
             that arms the tripwires CI cannot arm; without it those witnesses skip green \
             with nothing behind them."
        )
    });

    let unarmed: Vec<_> = read
        .iter()
        .filter(|t| !armed_ci.contains(*t) && !armed_doc.contains(*t))
        .collect();
    assert!(
        unarmed.is_empty(),
        "these anti-vacuity tripwires are read by a witness but armed by NOTHING — not a \
         workflow, not {RELEASE_DOC}: {unarmed:?}.\n\
         A witness that skips when a tool or credential is absent, whose tripwire nobody \
         ever sets, has an unreachable anti-vacuity half: it can only ever skip green.\n\
         Arm it in `.github/workflows/` if CI can, or add a runnable `NAME=1` command to \
         {RELEASE_DOC} §2 if only the release host can."
    );
}

/// The inverse false green: the document promising to arm something no witness
/// reads. That reads as coverage and buys none.
#[test]
fn the_release_doc_arms_no_tripwire_the_corpus_does_not_read() {
    let Some(files) = tracked_files() else {
        assert!(skip("no git history — cannot enumerate the corpus"));
        return;
    };
    let Some(armed_doc) = tripwires_armed_by_the_release_doc() else {
        panic!("{RELEASE_DOC} is missing");
    };
    assert!(
        !armed_doc.is_empty(),
        "anti-vacuity: {RELEASE_DOC} arms no tripwire at all, so this check passes for \
         free. §2 is supposed to contain runnable `XPILE_REQUIRE_*=1` commands."
    );

    let read = tripwires_read_by_the_corpus(&files);
    let phantom: Vec<_> = armed_doc.iter().filter(|t| !read.contains(*t)).collect();
    assert!(
        phantom.is_empty(),
        "{RELEASE_DOC} arms {phantom:?}, which NO tracked source reads. Either the witness \
         that read it was deleted (drop it from the doc) or the name is misspelled — in \
         both cases the pre-flight sets a variable nothing consults, which is coverage on \
         paper only."
    );
}

/// The broken citation itself, generalised: if a test source names the release
/// document as what arms its tripwire, the document must actually arm it.
#[test]
fn every_test_source_citing_the_release_doc_has_its_tripwires_armed_there() {
    let Some(files) = tracked_files() else {
        assert!(skip("no git history — cannot enumerate the corpus"));
        return;
    };
    let Some(armed_doc) = tripwires_armed_by_the_release_doc() else {
        panic!("{RELEASE_DOC} is missing");
    };

    let mut citing = 0usize;
    let mut broken: Vec<String> = Vec::new();
    for f in files.iter().filter(|f| f.ends_with(".rs")) {
        let Some(text) = read_tracked(f) else {
            continue;
        };
        if !text.contains(RELEASE_DOC) {
            continue;
        }
        citing += 1;
        for t in tripwires_read_by(&text) {
            if !armed_doc.contains(&t) {
                broken.push(format!(
                    "{f} cites {RELEASE_DOC} but reads {t}, unarmed there"
                ));
            }
        }
    }

    assert!(
        citing > 0,
        "anti-vacuity: no tracked source cites {RELEASE_DOC}, so this check ranged over \
         nothing. `ruleset_drift.rs` is expected to — if that citation was removed, this \
         gate has lost its subject and should be re-pointed, not left passing."
    );
    assert!(
        broken.is_empty(),
        "a test source credits {RELEASE_DOC} with arming a tripwire the document does not \
         arm:\n  {}\n\
         This is the exact defect PMAT-1416 closed: a skip branch that names a compensating \
         control which does not exist reads as mitigated and is not.",
        broken.join("\n  ")
    );
}

/// Structural presence over the procedure document. This can prove the sections
/// exist; it cannot prove the prose is true — see the module header.
#[test]
fn the_release_doc_documents_the_procedure_and_the_abort_rules() {
    let Some(text) = read_tracked(RELEASE_DOC) else {
        panic!(
            "{RELEASE_DOC} is missing — the sprint plan's exit criteria and the Thursday \
             tag slice both require it to document the procedure, the overlay purge, the \
             pre-flights and the abort rules."
        )
    };

    let mut missing: Vec<&str> = Vec::new();
    // Abort rules A1..A8 plus the drift rule A1b.
    for rule in [
        "**A1 —",
        "**A1b —",
        "**A2 —",
        "**A3 —",
        "**A4 —",
        "**A5 —",
        "**A6 —",
        "**A7 —",
        "**A8 —",
    ] {
        if !text.contains(rule) {
            missing.push(rule);
        }
    }
    // The overlay purge — A5's recoverable condition is unrecoverable without
    // knowing which directories to remove.
    for frag in ["tmp-registry", "tmp-crate"] {
        if !text.contains(frag) {
            missing.push(frag);
        }
    }
    // The deliberate one-day skew: the tag object's date and the CHANGELOG
    // heading's date disagree ON PURPOSE, and this file is where that is
    // disclosed. Undisclosed, it reads as a mistake and invites a "fix".
    if !text.contains("skew") {
        missing.push("the tag/CHANGELOG date skew disclosure");
    }
    // The verification trap that fired on v0.1.617: without a User-Agent the
    // crates.io API returns an error body and the verify loop reports every
    // crate MISSING on a successful publish.
    if !text.contains("User-Agent") {
        missing.push("the crates.io User-Agent verification trap");
    }

    assert!(
        missing.is_empty(),
        "{RELEASE_DOC} is missing required content: {missing:?}"
    );
}
