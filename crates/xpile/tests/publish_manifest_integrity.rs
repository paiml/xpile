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
