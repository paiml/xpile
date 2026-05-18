//! qa_gate enforcement gate (PMAT-146).
//!
//! Every Layer-1/2 kernel contract carries a `qa_gate:` block with a
//! `required_tests:` list naming concrete `#[test]` functions that
//! the contract relies on for runtime verification. This test
//! enforces that every named test actually exists in the workspace
//! so a future refactor that renames or deletes one of those tests
//! fires loudly here, not silently in the contract's coverage claim.
//!
//! What this test does NOT check:
//!   - That the named test actually passes (the test runner already
//!     handles that — what we'd lose is the *binding* between the
//!     contract's claim and a real test).
//!   - That `min_coverage` is met — that requires `cargo llvm-cov`
//!     output and is tracked separately (XPILE-CI-COVERAGE-001+).
//!
//! Companion to `refinement_proofs.rs`: that one binds `lean_theorem`
//! claims to real Lean theorems; this one binds qa_gate
//! `required_tests` to real Rust test functions. Same shape, same
//! philosophy — make stale claims fail loudly rather than silently.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Parse the `required_tests:` list under each contract's `qa_gate:`
/// block. Returns `(contract_yaml_path, test_name)` pairs. The
/// parser is a shallow line scan (same posture as
/// `refinement_proofs.rs`) — no serde_yaml dep.
fn extract_required_tests(contents: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_qa_gate = false;
    let mut in_required_tests = false;
    for line in contents.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.is_empty() {
            continue;
        }
        // Top-level `qa_gate:` block starts when we see exactly that
        // at column 0 (or with no leading whitespace).
        if !line.starts_with(char::is_whitespace) && trimmed_start.starts_with("qa_gate:") {
            in_qa_gate = true;
            in_required_tests = false;
            continue;
        }
        // We left the qa_gate block when we hit another top-level
        // key (no leading whitespace).
        if !line.starts_with(char::is_whitespace)
            && in_qa_gate
            && !trimmed_start.starts_with("qa_gate:")
        {
            in_qa_gate = false;
            in_required_tests = false;
            continue;
        }
        if !in_qa_gate {
            continue;
        }
        // Inside qa_gate, look for `  required_tests:` (2-space
        // indent — pv schema convention).
        if trimmed_start.starts_with("required_tests:") {
            in_required_tests = true;
            continue;
        }
        // Sibling key inside qa_gate (e.g. `min_coverage:`) closes
        // the required_tests list.
        if in_required_tests && !trimmed_start.starts_with('-') {
            in_required_tests = false;
            continue;
        }
        if in_required_tests {
            if let Some(name) = trimmed_start.strip_prefix("- ") {
                let name = name.trim();
                // Strip optional surrounding quotes (single or
                // double — pv accepts both).
                let name = name.trim_matches('"').trim_matches('\'').to_string();
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// Collect all `#[test]`-annotated function names in every
/// `crates/*/tests/*.rs` and `crates/*/src/**/*.rs` file in the
/// workspace. Returns a deduplicated `Vec<String>` of test names.
fn collect_test_function_names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let crates_dir = root.join("crates");
    let Ok(crates) = fs::read_dir(&crates_dir) else {
        return names;
    };
    for c in crates.flatten() {
        let cp = c.path();
        if !cp.is_dir() {
            continue;
        }
        // Scan crates/*/tests/*.rs (integration tests live here)
        let tests_dir = cp.join("tests");
        if tests_dir.is_dir() {
            walk_rs_for_tests(&tests_dir, &mut names);
        }
        // Scan crates/*/src/**/*.rs (unit tests live here under
        // `#[cfg(test)] mod tests {}` blocks).
        let src_dir = cp.join("src");
        if src_dir.is_dir() {
            walk_rs_for_tests(&src_dir, &mut names);
        }
    }
    names.sort();
    names.dedup();
    names
}

fn walk_rs_for_tests(dir: &Path, names: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_rs_for_tests(&p, names);
            continue;
        }
        if p.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Ok(src) = fs::read_to_string(&p) else {
            continue;
        };
        let mut seen_test_attr = false;
        for line in src.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[test]") {
                seen_test_attr = true;
                continue;
            }
            if seen_test_attr {
                // Match `fn <name>(` after any whitespace / `pub`.
                let after_pub = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
                if let Some(rest) = after_pub.strip_prefix("fn ") {
                    if let Some(end) = rest.find(|c: char| !c.is_alphanumeric() && c != '_') {
                        names.push(rest[..end].to_string());
                    }
                }
                seen_test_attr = false;
            }
        }
    }
}

#[test]
fn every_qa_gate_required_test_exists_as_a_real_test_fn() {
    let root = workspace_root();
    let contracts_dir = root.join("contracts");
    let test_fns = collect_test_function_names(&root);
    assert!(
        test_fns.len() >= 10,
        "expected at least 10 #[test] fns to be collected; \
         test discovery probably broke (found {})",
        test_fns.len()
    );
    let entries = fs::read_dir(&contracts_dir).expect("read contracts/");
    let mut checked = 0;
    let mut missing: Vec<(PathBuf, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let contents = fs::read_to_string(&path).expect("read contract yaml");
        let required = extract_required_tests(&contents);
        for name in required {
            if !test_fns.iter().any(|t| t == &name) {
                missing.push((path.clone(), name));
            } else {
                checked += 1;
            }
        }
    }
    assert!(
        missing.is_empty(),
        "qa_gate required_tests reference {} test fn(s) that don't exist: {:#?}",
        missing.len(),
        missing
    );
    assert!(
        checked >= 2,
        "expected at least 2 qa_gate required_tests to be checked; \
         the qa_gate blocks themselves may have regressed (checked {})",
        checked
    );
}

#[test]
fn extract_required_tests_parses_the_common_yaml_shape() {
    let yaml = r#"
metadata:
  id: C-FOO
  version: "1.0.0"

equations: {}

qa_gate:
  id: QA-FOO
  name: "test contract"
  min_coverage: 0.50
  max_complexity: 20
  required_tests:
    - test_alpha
    - test_beta
    - "test_gamma"
"#;
    let got = extract_required_tests(yaml);
    assert_eq!(got, vec!["test_alpha", "test_beta", "test_gamma"]);
}

#[test]
fn extract_required_tests_returns_empty_when_no_qa_gate() {
    let yaml = r#"
metadata:
  id: C-FOO

equations: {}
"#;
    assert!(extract_required_tests(yaml).is_empty());
}
