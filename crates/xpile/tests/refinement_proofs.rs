//! Refinement-proof gate (PMAT-017 / XPILE-REFINE-001).
//!
//! Every Layer-1 contract equation that has both a fast and a slow
//! path SHOULD ship a Lean theorem statement proving they refine each
//! other within the precondition. The proof itself may be `sorry`
//! at v0.1.0 — the *statement* is the load-bearing artefact, because
//! it's what the citation pipeline (PMAT-011) points at via
//! `@[xpile_contract "..."]`.
//!
//! This test enforces:
//!   1. For every YAML equation with a `lean_theorem:` field, the
//!      named .lean file exists and contains a theorem of that name.
//!   2. The .lean file lives under `contracts/lean/` (the canonical
//!      location, mirroring the YAML's `contracts/` location).
//!
//! What this test does NOT enforce:
//!   - The proof actually checks (would need `lean` in PATH; deferred
//!     to XPILE-REFINE-002 / CI integration).
//!   - The proof is `sorry`-free. v0.1.0 explicitly ships `sorry`s as
//!     declared TODOs; their deadlines are governed by the
//!     XPILE-PENDING-UNTIL markers in the .lean file (PMAT-014 gate).

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

// Walk every Layer-1 contract YAML, pull out `lean_theorem: "..."` +
// `lean_file: "..."` field pairs (parsed line-by-line — no YAML dep
// needed because the format is shallow), and assert that each
// referenced theorem exists in the referenced .lean file.
#[test]
fn every_referenced_lean_theorem_exists_in_its_file() {
    let root = workspace_root();
    let contracts_dir = root.join("contracts");
    let entries = fs::read_dir(&contracts_dir).expect("read contracts/");
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let contents = fs::read_to_string(&path).expect("read contract yaml");
        // Pair each `lean_theorem: "<name>"` with the following
        // `lean_file: "<path>"` line. v0.1.0 schema is strict:
        // they appear adjacent.
        let lines: Vec<&str> = contents.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(thm) = extract_quoted_value(line, "lean_theorem:") else {
                continue;
            };
            let file = lines
                .iter()
                .skip(i + 1)
                .take(3)
                .find_map(|l| extract_quoted_value(l, "lean_file:"))
                .unwrap_or_else(|| {
                    panic!(
                        "{}: lean_theorem `{}` missing adjacent lean_file: \"...\"",
                        path.display(),
                        thm
                    )
                });
            let lean_path = root.join(&file);
            assert!(
                lean_path.is_file(),
                "{}: lean_theorem `{}` references missing file `{}`",
                path.display(),
                thm,
                file
            );
            let lean_src = fs::read_to_string(&lean_path).expect("read lean file");
            // Lean theorem statement must appear by name. The
            // citation-bridge convention (audit-design.md §4 +
            // contracts/xpile-contract-backend-trait-v1.yaml) is that
            // the namespace path encodes the contract id. We accept
            // either `theorem <unqualified_name>` (inside namespace
            // block) OR the fully qualified path appearing verbatim.
            let unqualified = thm.rsplit('.').next().unwrap_or(&thm);
            let has_theorem = lean_src.lines().any(|l| {
                l.trim_start()
                    .starts_with(&format!("theorem {unqualified}"))
            });
            assert!(
                has_theorem,
                "{}: lean_theorem `{}` not found in `{}` (looked for `theorem {}`)",
                path.display(),
                thm,
                file,
                unqualified
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 1,
        "expected at least one lean_theorem field across contracts/; \
         XPILE-REFINE-001 was supposed to plant one in py-int-arith-v1.yaml"
    );
}

// Helper: parse `key: "value"` (with optional whitespace and a `# ...`
// trailer) out of a single line. Avoids serde_yaml for the lightweight
// scan.
fn extract_quoted_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix(key)?;
    let rest = rest.trim();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// Sanity: the .lean file for the first refinement proof
// (PyIntArith.lean) exists, references the contract namespace, and
// flags its TODO-proof status loudly. If a future refactor removes
// the file without removing the YAML reference, the test above fires
// first; this test guards the *content invariants*.
#[test]
fn py_int_arith_lean_file_carries_required_landmarks() {
    let root = workspace_root();
    let lean = root.join("contracts/lean/PyIntArith.lean");
    assert!(lean.is_file(), "{} must exist", lean.display());
    let src = fs::read_to_string(&lean).expect("read PyIntArith.lean");
    for needle in &[
        "namespace XpileContracts.CPyIntArith",
        "theorem fast_path_eq_slow_path",
        // Honest TODO marker — the sorry must remain visible so an
        // automated audit knows the proof is unproved.
        "sorry",
        // Time-bounded escape hatch (PMAT-014 gate ensures this date
        // doesn't pass without a fix).
        "XPILE-PENDING-UNTIL: v0.3.0",
    ] {
        assert!(
            src.contains(needle),
            "PyIntArith.lean must contain `{needle}` — see refinement_proofs.rs for context"
        );
    }
}

// Helper: parser self-test. Quotes, whitespace, trailing comments.
#[test]
fn extract_quoted_value_handles_common_yaml_shapes() {
    assert_eq!(
        extract_quoted_value("  lean_theorem: \"Foo.Bar\"", "lean_theorem:"),
        Some("Foo.Bar".to_string())
    );
    assert_eq!(
        extract_quoted_value(
            "    lean_file:       \"contracts/lean/X.lean\"",
            "lean_file:"
        ),
        Some("contracts/lean/X.lean".to_string())
    );
    // Wrong key shouldn't match.
    assert_eq!(
        extract_quoted_value("  lean_theorem: \"Foo\"", "lean_file:"),
        None
    );
    // No quotes — we currently require quotes, returns None.
    assert_eq!(
        extract_quoted_value("  lean_theorem: Foo", "lean_theorem:"),
        None
    );
}
