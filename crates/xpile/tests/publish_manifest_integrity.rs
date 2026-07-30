//! publish_manifest_integrity — the per-PR falsifier for XPILE-CLEANROOM-001.
//!
//! The `release` workflow (.github/workflows/release.yml) runs the real
//! `cargo publish --workspace --dry-run --locked` clean-room gate, but it is
//! dispatch/tag-triggered (heavy, network-bound) and NOT part of the per-PR
//! required lane. This test replicates the ONE precondition that gate enforces
//! which is cheap to check statically: every intra-workspace `path = "crates/…"`
//! dependency declared in the root manifest's `[workspace.dependencies]` table
//! must ALSO carry an explicit `version = ` on the same line.
//!
//! `cargo publish` refuses ("all dependencies must have a version specified
//! when packaging. dependency `X` does not specify a version") the instant a
//! path-dep lacks a version, so stripping a `version =` from any workspace
//! path-dep would turn the release dry-run RED. This test makes that exact
//! mutation fail in the fast `cargo test --workspace` lane instead of only at
//! release time.
//!
//! ## PMAT-1408 — the version the path-dep carries has to be the RIGHT one
//!
//! Carrying *a* `version =` is necessary but not sufficient. Through v0.1.617
//! all 34 intra-workspace path-deps declared `version = "0.1.12"` while
//! `[workspace.package] version` had reached `0.1.617` — a 605-release skew.
//! This is INVISIBLE to `cargo publish --dry-run`: under Cargo's 0.x semver,
//! `^0.1.12` means `>=0.1.12, <0.2.0`, which `0.1.617` satisfies, so every
//! manifest packaged cleanly. Verified against the live sparse index, not just
//! this file — published `xpile-core 0.1.617` declares 26 of its 27 siblings at
//! `^0.1.12`:
//!
//! ```text
//! $ curl -H 'User-Agent: …' https://index.crates.io/xp/il/xpile-core | tail -1
//! {"vers":"0.1.617", "deps":[{"name":"xpile-frontend","req":"^0.1.12"}, …]}
//! ```
//!
//! The consequence is a resolvable-but-never-built combination: a downstream
//! lockfile pinning `xpile-frontend 0.1.12` still satisfies `xpile-core
//! 0.1.617`'s requirement, so `cargo build` happily pairs a 2026-05 frontend
//! with a 2026-07 core across 605 releases of meta-HIR drift. Nothing in the
//! release lane could catch it, because nothing was wrong with the *manifest* —
//! only with the number in it.
//!
//! `[workspace.dependencies]` entries cannot inherit
//! `[workspace.package].version` (there is no `version.workspace = true` inside
//! the workspace table itself — the inheritance only flows workspace → member),
//! so the number is necessarily duplicated and can only be kept honest by a
//! gate. That is `every_workspace_path_dep_matches_the_workspace_version`
//! below. **A release version bump must therefore edit all 35 occurrences**
//! (`[workspace.package] version` + 34 path-deps), not just line 43; this gate
//! is what turns forgetting the other 34 into a red PR instead of a skewed
//! publish.
//!
//! Residual, disclosed rather than fixed: the requirement is still a caret, so
//! `xpile-core 0.1.617` admits `xpile-frontend 0.1.618`. That window is one
//! release wide and only reachable via an explicit downstream pin, versus the
//! 605-release window it replaces. Tightening to `=0.1.617` would forbid
//! downstream from ever mixing and is a separate decision.
//!
//! Shallow line-scan posture (no serde/toml dev-dep), mirroring
//! `qa_gate.rs` and `refinement_proofs.rs`.

use std::fs;
use std::path::PathBuf;

/// crates/xpile/ -> crates/ -> <workspace root>/Cargo.toml
/// (same idiom as qa_gate.rs and refinement_proofs.rs).
fn root_cargo_toml() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("Cargo.toml")
}

/// Every workspace path-dependency (`path = "crates/…"`) must also declare an
/// explicit `version = ` on the same line, or `cargo publish --dry-run` refuses
/// to package. Guards the clean-room precondition in the fast test lane.
#[test]
fn every_workspace_path_dep_carries_a_version() {
    let manifest = root_cargo_toml();
    let contents = fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));

    let mut offenders: Vec<String> = Vec::new();
    for (idx, line) in contents.lines().enumerate() {
        // Skip commented-out example lines (e.g. the [patch.crates-io] doc
        // block that shows a sibling `path =` override).
        if line.trim_start().starts_with('#') {
            continue;
        }
        if line.contains("path = \"crates/") && !line.contains("version =") {
            offenders.push(format!("  Cargo.toml:{}: {}", idx + 1, line.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "{} intra-workspace path-dep(s) in the root manifest are missing \
         `version = ` — `cargo publish --workspace --dry-run` (release.yml) \
         would refuse to package these:\n{}",
        offenders.len(),
        offenders.join("\n"),
    );
}

// ── PMAT-1408: the declared version must EQUAL the workspace version ────────

/// Is the byte at `idx` the start of a standalone `version` key, rather than
/// the tail of `rust-version` / `edition-version` / similar?
fn is_key_boundary(line: &str, idx: usize) -> bool {
    line[..idx]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_')
}

/// Extract the value of the first standalone `version = "…"` key on `line`.
/// Returns `None` when the line declares no version (that case belongs to
/// `every_workspace_path_dep_carries_a_version`, which owns it).
fn declared_version(line: &str) -> Option<&str> {
    let mut from = 0usize;
    while let Some(rel) = line[from..].find("version") {
        let at = from + rel;
        from = at + "version".len();
        if !is_key_boundary(line, at) {
            continue;
        }
        let rest = line[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        return rest.find('"').map(|end| &rest[..end]);
    }
    None
}

/// The `[workspace.package] version = "…"` of a root manifest.
///
/// Scans only inside the `[workspace.package]` table so it cannot be fooled by
/// a `version` key in some other table.
fn workspace_package_version(contents: &str) -> Option<&str> {
    let mut in_table = false;
    for line in contents.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_table = t == "[workspace.package]";
            continue;
        }
        if in_table && t.starts_with("version") {
            if let Some(v) = declared_version(t) {
                return Some(v);
            }
        }
    }
    None
}

/// Every intra-workspace path-dep line whose declared version is present but
/// differs from `want`. Lines with no version at all are skipped — the sibling
/// test owns that failure mode, and reporting it twice would make one fix look
/// like two.
fn skewed_path_deps(contents: &str, want: &str) -> Vec<String> {
    let mut skewed = Vec::new();
    for (idx, line) in contents.lines().enumerate() {
        if line.trim_start().starts_with('#') || !line.contains("path = \"crates/") {
            continue;
        }
        match declared_version(line) {
            Some(v) if v != want => skewed.push(format!(
                "  Cargo.toml:{}: declares {v} — {}",
                idx + 1,
                line.trim()
            )),
            _ => {}
        }
    }
    skewed
}

/// Every intra-workspace path-dep must declare the CURRENT workspace version.
/// A stale number here is satisfiable-but-wrong: `^0.1.12` admits `0.1.617`, so
/// `cargo publish --dry-run` packages it and a downstream resolver is free to
/// pair releases 605 apart. See the module docs.
#[test]
fn every_workspace_path_dep_matches_the_workspace_version() {
    let manifest = root_cargo_toml();
    let contents = fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));

    let want = workspace_package_version(&contents)
        .expect("root manifest declares [workspace.package] version");
    let skewed = skewed_path_deps(&contents, want);

    assert!(
        skewed.is_empty(),
        "{} intra-workspace path-dep(s) declare a version other than the \
         workspace version {want}. `cargo publish --dry-run` CANNOT catch this \
         — `^0.1.12` is satisfied by {want}, so the manifest is valid, just \
         wrong, and a downstream lockfile may legally pair versions that were \
         never built together. A release bump must edit ALL of them, not just \
         `[workspace.package] version`:\n{}",
        skewed.len(),
        skewed.join("\n"),
    );
}

/// The RED half, run rather than asserted: feed the detector a synthetic
/// manifest carrying exactly the v0.1.617 defect and confirm it fires, and the
/// repaired variant and confirm it does not. Without this, a detector that
/// silently matched nothing (a typo in `path = "crates/`, a `version` boundary
/// bug) would report the corpus clean forever.
#[test]
fn the_skew_detector_reds_on_a_deliberately_skewed_dep() {
    const SKEWED: &str = r#"
[workspace.package]
version = "0.1.617"
rust-version = "1.93"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
xpile-core     = { version = "0.1.12", path = "crates/xpile-core" }
xpile-frontend = { version = "0.1.617", path = "crates/xpile-frontend" }
depyler = { version = "0.1.12", path = "crates/depyler-frontend", package = "depyler-frontend" }
# xpile-commented = { version = "0.0.1", path = "crates/xpile-commented" }
xpile-noversion = { path = "crates/xpile-noversion" }
"#;

    let want = workspace_package_version(SKEWED).expect("fixture declares a workspace version");
    assert_eq!(
        want, "0.1.617",
        "must read [workspace.package] version, not `rust-version`'s 1.93 or serde's 1",
    );

    let skewed = skewed_path_deps(SKEWED, want);
    assert_eq!(
        skewed.len(),
        2,
        "expected exactly the two 0.1.12 deps to be flagged — the commented \
         line and the version-less line belong to other tests. got:\n{}",
        skewed.join("\n"),
    );
    assert!(skewed[0].contains("xpile-core"), "got {}", skewed[0]);
    assert!(skewed[1].contains("depyler-frontend"), "got {}", skewed[1]);

    // GREEN half: the same manifest with the skew repaired must be clean, so a
    // detector that flags everything unconditionally cannot pass this test.
    let repaired = SKEWED.replace("\"0.1.12\"", "\"0.1.617\"");
    assert!(
        skewed_path_deps(&repaired, want).is_empty(),
        "repairing the skew must clear the detector",
    );
}

/// `declared_version` must key on a standalone `version`, never the tail of
/// `rust-version`. A boundary bug here would read `1.93` as the workspace
/// version and then flag all 34 real deps, or read a dep's `rust-version` and
/// flag it spuriously.
#[test]
fn declared_version_ignores_hyphenated_lookalike_keys() {
    assert_eq!(declared_version(r#"rust-version = "1.93""#), None);
    assert_eq!(
        declared_version(r#"x = { rust-version = "1.93", version = "0.1.617" }"#),
        Some("0.1.617"),
    );
    assert_eq!(declared_version(r#"x = { path = "crates/x" }"#), None);
    assert_eq!(
        declared_version(r#"x = { version="0.1.617" }"#),
        Some("0.1.617")
    );
}

// ── XPILE-CLEANROOM-PATCH-001 (PMAT-1500) ───────────────────────────────────
//
// THE CLAIM THAT WAS FALSE. Through v0.1.618 three documents published the
// clean room's patch-isolation property as a CONSEQUENCE of the fresh
// CARGO_HOME: `.github/workflows/release.yml` ("ISOLATED, EMPTY CARGO_HOME so
// no sibling `[patch.crates-io]` path-override … can satisfy a dependency the
// real crates.io registry could not"), `docs/RELEASE.md` §3, and the
// `XPILE-CLEANROOM-001` row in `docs/status/enforcement-handoff.md`. It is the
// isolation guarantee under which an irreversible 31-crate publish is
// pre-flighted, and NO gate in this repository had ever read it.
//
// MEASURED, three arms, fresh empty `CARGO_HOME` + `--offline` throughout:
//   1. `[patch.crates-io]` in `./.cargo/config.toml` → cargo exit 0, the dep
//      resolves to the LOCAL path (`source: None`).
//   2. control, identical tree with the patch file removed → exit 101,
//      "no matching package … location searched: crates.io index".
//   3. `[patch.crates-io]` in the workspace ROOT `Cargo.toml` → exit 0, again
//      resolved to the local path.
// `CARGO_HOME` governs the registry cache, credentials and
// `$CARGO_HOME/config.toml`. Cargo ALSO discovers config by walking the CWD
// HIERARCHY, and `[patch]` is legal in the root manifest. Neither is reachable
// from `CARGO_HOME`, so the named mechanism cannot produce the named property.
//
// AND THE STEP THAT ACTUALLY PRODUCED IT COULD NOT FIRE. The workflow's
// tripwire was `grep -REn 'patch\.' .cargo | grep -q 'path *='`, which needs
// `patch.` and `path =` on the SAME LINE. The spelling this repository's own
// `Cargo.toml:95-96` instructs developers to write spans two lines, so against
// exactly that committed file the guard returned GREEN and printed "clean room
// preserved". It fired only on the dotted-key one-liner, documented nowhere
// here, and scanned only `.cargo/` — arm 3 had no coverage at all.
//
// This module is the authoritative copy of the corrected check: it runs in the
// fast per-PR `cargo test --workspace` lane, where the heavy dispatch/tag-only
// `cleanroom-publish` job cannot be verified by a reader at all. That
// unverifiability is the point — cf. PMAT-1499, where a sandbox property of a
// server you cannot start went unfalsified for the same reason.

/// Files cargo would actually read for a `[patch]` in this checkout, relative
/// to the workspace root. `actions/checkout@v4` materialises only tracked
/// files, so on CI "present" == "committed".
const PATCH_BEARING_FILES: [&str; 3] = [".cargo/config.toml", ".cargo/config", "Cargo.toml"];

/// The exact sibling-checkout recipe `Cargo.toml` documents, uncommented. Used
/// as the positive control: this is the input the superseded guard read as
/// clean.
const DOCUMENTED_SPELLING: &str = "[patch.crates-io]\n\
    aprender-contracts = { path = \"../aprender/crates/aprender-contracts\" }\n";

/// Section-aware scan for a path-based `[patch]` override. Returns one
/// `line:text` per offending line.
///
/// A line whose first non-space byte is `#` is skipped outright — that is how
/// the `#`-commented recipe in `Cargo.toml` stays green, and keying on the
/// leading `#` (rather than stripping from any `#`) means a `#` inside a
/// quoted path cannot blind the scan.
fn path_based_patch_hits(contents: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let mut in_patch = false;
    for (idx, line) in contents.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            in_patch = trimmed.starts_with("[patch.") || trimmed.starts_with("[patch]");
        }
        let has_path = declares_path(line);
        // Section form: `[patch.crates-io]` … `foo = { path = "…" }`.
        // Dotted form: `patch.crates-io.foo = { path = "…" }`.
        if has_path && (in_patch || trimmed.starts_with("patch.")) {
            hits.push(format!("{}: {}", idx + 1, line.trim()));
        }
    }
    hits
}

/// `path` as a standalone key, not the tail of `rust-path` / `search-path`.
fn declares_path(line: &str) -> bool {
    line.match_indices("path")
        .filter(|(i, _)| is_key_boundary(line, *i))
        .any(|(i, _)| line[i + "path".len()..].trim_start().starts_with('='))
}

/// No committed `[patch]` may carry a path override: it would let the release
/// dry-run resolve a dependency from a local sibling and mask a missing
/// `version =`, which is the one thing the clean room exists to catch.
#[test]
fn no_committed_patch_carries_a_path_override() {
    let root = root_cargo_toml()
        .parent()
        .expect("workspace root")
        .to_owned();

    let mut offenders: Vec<String> = Vec::new();
    for rel in PATCH_BEARING_FILES {
        let path = root.join(rel);
        let Ok(contents) = fs::read_to_string(&path) else {
            continue; // absent is the normal case for .cargo/*
        };
        for hit in path_based_patch_hits(&contents) {
            offenders.push(format!("  {rel}:{hit}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "XPILE-CLEANROOM-PATCH-001: {} committed `[patch]` path override(s) — the \
         release dry-run would resolve a dependency from a local sibling instead \
         of from crates.io as-published, masking exactly the defect the clean \
         room exists to catch. A fresh CARGO_HOME does NOT suppress these \
         (measured; see this file's header):\n{}",
        offenders.len(),
        offenders.join("\n"),
    );
}

/// RED HALF — the spelling this repository documents. The superseded workflow
/// grep returned GREEN on this exact input; if this assertion ever passes with
/// an empty result, the scan has regressed to same-line matching.
#[test]
fn the_patch_detector_reds_on_the_spelling_this_repo_documents() {
    let hits = path_based_patch_hits(DOCUMENTED_SPELLING);
    assert_eq!(
        hits.len(),
        1,
        "the two-line `[patch.crates-io]` spelling from Cargo.toml:95-96 must be \
         detected — this is the input the superseded same-line grep read as a \
         clean room. got: {hits:?}"
    );
    assert!(hits[0].contains("aprender-contracts"), "{hits:?}");
}

/// RED HALF — the dotted-key one-liner (the only form the old grep caught) and
/// the root-manifest route (arm 3, which it never scanned) must both red.
#[test]
fn the_patch_detector_reds_on_the_dotted_key_and_root_manifest_routes() {
    let dotted = "patch.crates-io.aprender-contracts = { path = \"../aprender\" }\n";
    assert_eq!(path_based_patch_hits(dotted).len(), 1, "dotted-key form");

    // Arm 3: the same section spelling, but living in the root manifest after
    // an unrelated table — the route with zero coverage before PMAT-1500.
    let in_manifest = format!(
        "[workspace.dependencies]\nserde = \"1\"\n\n{}",
        DOCUMENTED_SPELLING
    );
    assert_eq!(
        path_based_patch_hits(&in_manifest).len(),
        1,
        "root-manifest `[patch]` route"
    );

    // A `[patch]` section that ENDS must not leak into the next table.
    let closed = format!("{DOCUMENTED_SPELLING}\n[dependencies]\nx = {{ path = \"crates/x\" }}\n");
    assert_eq!(
        path_based_patch_hits(&closed).len(),
        1,
        "section state must reset at the next table header, so an ordinary \
         intra-workspace path-dep is not reported as a patch override"
    );
}

/// GREEN HALF, and the PMAT-1495 exemption trap: the recipe is exempt only
/// because it is commented, so the control is vacuous unless the commented
/// block is genuinely PRESENT in the manifest. Assert both halves.
#[test]
fn the_commented_recipe_in_the_root_manifest_is_present_and_exempt() {
    let contents = fs::read_to_string(root_cargo_toml()).expect("read root manifest");

    assert!(
        contents.contains("#   [patch.crates-io]"),
        "the commented sibling-checkout recipe has moved or been deleted from \
         Cargo.toml — this test's exemption control is now vacuous and the \
         `#`-skipping branch of path_based_patch_hits is unexercised by real data"
    );
    assert!(
        path_based_patch_hits(&contents).is_empty(),
        "the `#`-commented recipe must not be read as a live override"
    );
}

/// EXPIRY PROPERTY, keyed to the workflow rather than to prose: if the
/// superseded same-line grep is reinstated, or the guard stops scanning the
/// root manifest, this reds in the fast lane instead of silently restoring a
/// tripwire that cannot fire. A disclosure that cannot expire becomes the next
/// stale claim (PMAT-1499).
#[test]
fn the_release_workflow_guard_scans_the_root_manifest_and_not_only_dot_cargo() {
    let workflow = root_cargo_toml()
        .parent()
        .expect("workspace root")
        .join(".github/workflows/release.yml");
    let contents = fs::read_to_string(&workflow)
        .unwrap_or_else(|e| panic!("read {}: {e}", workflow.display()));

    const RETIRED: &str = "grep -REn 'patch\\.' .cargo";

    // PMAT-1495's exemption trap, live again on this very fix: the comment
    // block above the guard QUOTES the retired implementation verbatim to
    // record what was wrong with it, so a naive `contains` screen reds on the
    // sentence documenting the repair. Screen the EXECUTABLE lines only — a
    // reinstated grep would be a real command, never `#`-prefixed.
    let executable: String = contents
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    // …and because the only remaining mention is now exempt, the screen could
    // match nothing and report green forever. Positive control: the retired
    // spelling must still be present SOMEWHERE, i.e. in the comments.
    assert!(
        contents.contains(RETIRED),
        "the record of the superseded same-line grep has been deleted from \
         release.yml's comments, so the screen below can no longer distinguish \
         'never reinstated' from 'nothing to match'"
    );
    assert!(
        !executable.contains(RETIRED),
        "the superseded same-line `grep -REn 'patch.' .cargo | grep 'path *='` \
         tripwire is back in release.yml as EXECUTABLE shell. It returns GREEN \
         on the two-line spelling documented at Cargo.toml:95-96 and never scans \
         the root manifest — see this file's header for the executed measurement."
    );
    assert!(
        contents.contains("for f in .cargo/config.toml .cargo/config Cargo.toml"),
        "release.yml's clean-room tripwire must scan the root `Cargo.toml` as \
         well as `.cargo/config*` — `[patch]` in the root manifest is honoured \
         under a fresh CARGO_HOME (measured arm 3) and had no coverage at all \
         before PMAT-1500."
    );
    assert!(
        contents.contains("XPILE-CLEANROOM-PATCH-001"),
        "the CARGO_HOME comment block must keep the disclosure that a fresh \
         CARGO_HOME does not exclude a PATCHED sibling copy, only a CACHED one"
    );
}
