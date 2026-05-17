//! Extrinsic-stratum attestation gate (PMAT-032 / XPILE-QUORUM-005).
//!
//! Verifies that `xpile attestations` finds at least one human
//! attestation for `C-PY-INT-ARITH` in the live `roadmap.yaml`. This
//! is the Extrinsic-stratum side of the ruchy 5.0 §14.4 quorum: a
//! contract should have ≥1 vote in *each* stratum it is verified by;
//! the Extrinsic vote is "humans have committed to working on this
//! contract" and is sourced from the roadmap log.
//!
//! Why C-PY-INT-ARITH specifically: it's the only contract with
//! multi-stratum coverage at v0.1.0 (Semantic via Lean, Symbolic via
//! Kani, Runtime via diff_exec). If the Extrinsic stratum can't even
//! find a roadmap mention for the most-attested contract in the
//! project, the scanner is broken.
//!
//! Skip behaviour: if the workspace happens to lack a roadmap.yaml
//! (e.g., someone runs the test from a checkout where the file was
//! moved), the test exits OK with a warning rather than failing —
//! mirrors `diff_exec.rs`'s "missing rustc → warn-and-exit" posture.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn xpile_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_xpile"))
}

#[test]
fn attestations_finds_c_py_int_arith_in_live_roadmap() {
    let root = workspace_root();
    let roadmap = root.join("docs/roadmaps/roadmap.yaml");
    let contracts = root.join("contracts");
    if !roadmap.is_file() {
        eprintln!(
            "warning: skipping XPILE-QUORUM-005 — {} missing",
            roadmap.display()
        );
        return;
    }

    let out = Command::new(xpile_bin())
        .args([
            "attestations",
            "--json",
            "--roadmap",
            roadmap.to_str().unwrap(),
            "--contracts-dir",
            contracts.to_str().unwrap(),
        ])
        .output()
        .expect("run xpile attestations");
    assert!(
        out.status.success(),
        "xpile attestations failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Hand-parse the relevant slice of the JSON without pulling
    // serde_json into the test crate: look for the substring
    // `"id":"C-PY-INT-ARITH"` and then the adjacent mention_count.
    // If a future audit demands tighter parsing, swap this for
    // serde_json — but this is enough to assert "found ≥1 mention".
    let id_marker = "\"id\":\"C-PY-INT-ARITH\"";
    let idx = stdout
        .find(id_marker)
        .unwrap_or_else(|| panic!("expected C-PY-INT-ARITH entry in attestations JSON:\n{stdout}"));
    let tail = &stdout[idx..];
    let count_marker = "\"mention_count\":";
    let cidx = tail
        .find(count_marker)
        .expect("expected mention_count in entry");
    let after = &tail[cidx + count_marker.len()..];
    let end = after.find([',', '}']).expect("expected delimiter");
    let count: u64 = after[..end]
        .trim()
        .parse()
        .expect("mention_count must be an integer");
    assert!(
        count >= 1,
        "expected ≥1 attestation for C-PY-INT-ARITH; the contract appears in titles for \
         multiple shipped PRs (PMAT-002 / 011 / 017 / 019 / 029 / 030). Got {count}. \
         Either the scanner regressed or the roadmap was rewritten."
    );
}

#[test]
fn attestations_text_output_runs_cleanly() {
    let root = workspace_root();
    let roadmap = root.join("docs/roadmaps/roadmap.yaml");
    let contracts = root.join("contracts");
    if !roadmap.is_file() {
        return;
    }
    let out = Command::new(xpile_bin())
        .args([
            "attestations",
            "--roadmap",
            roadmap.to_str().unwrap(),
            "--contracts-dir",
            contracts.to_str().unwrap(),
        ])
        .output()
        .expect("run xpile attestations");
    assert!(
        out.status.success(),
        "text-mode xpile attestations failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Must mention the QUORUM ticket, the contracts_scanned line,
    // and the stratum identification footer.
    for needle in &[
        "XPILE-QUORUM-005",
        "contract ID(s) from contracts/",
        "stratum: Extrinsic",
    ] {
        assert!(
            stdout.contains(needle),
            "text-mode attestations output missing landmark `{needle}`:\n{stdout}"
        );
    }
}
