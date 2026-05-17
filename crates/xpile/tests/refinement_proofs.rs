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
// carries the load-bearing proof landmarks. If a future refactor
// removes the file without removing the YAML reference, the test
// above fires first; this test guards the *content invariants*.
//
// PMAT-029 (XPILE-REFINE-003): all four `C-PY-INT-ARITH` theorems
// are now discharged — `fast_path_eq_slow_path` (add) via
// `Int.bmod_def` + `split <;> omega`; the mul / floor_div / mod
// trio reuse the same shape (mul via `bmod_fits_i64`; floor_div
// and mod via `rfl`). The `XPILE-PENDING-UNTIL: v0.3.0` landmark
// is no longer required because no marker remains on this file.
//
// PMAT-028: `sorry` is also forbidden as a positive landmark. We
// assert the discharge technique (`Int.bmod_def`) is present and
// `sorry` is absent — so a regression that reintroduces `sorry`
// fires loudly.
#[test]
fn py_int_arith_lean_file_carries_required_landmarks() {
    let root = workspace_root();
    let lean = root.join("contracts/lean/PyIntArith.lean");
    assert!(lean.is_file(), "{} must exist", lean.display());
    let src = fs::read_to_string(&lean).expect("read PyIntArith.lean");
    for needle in &[
        "namespace XpileContracts.CPyIntArith",
        "theorem fast_path_eq_slow_path",
        "theorem mul_fast_path_eq_slow_path",
        "theorem floor_div_fast_path_eq_slow_path",
        "theorem mod_fast_path_eq_slow_path",
        // Positive proof landmark — the discharge technique is
        // `Int.bmod_def` + `split <;> omega`, factored through the
        // shared `bmod_fits_i64` helper. If a future refactor
        // regresses to `sorry`, this fires.
        "Int.bmod_def",
    ] {
        assert!(
            src.contains(needle),
            "PyIntArith.lean must contain `{needle}` — see refinement_proofs.rs for context"
        );
    }
    // Negative landmark: no theorem must contain `sorry` anymore.
    // If someone reintroduces it, this test fires loudly. We scan
    // the file ignoring comment / docstring lines to avoid false
    // positives from the word appearing in prose.
    let no_sorry_lines: Vec<&str> = src
        .lines()
        .filter(|l| {
            let trimmed = l.trim_start();
            !trimmed.starts_with("--") && !trimmed.starts_with("/-") && !trimmed.starts_with("|")
        })
        .filter(|l| l.contains("sorry"))
        .collect();
    assert!(
        no_sorry_lines.is_empty(),
        "PyIntArith.lean must not carry `sorry` in proof code (PMAT-028/029 discharged all four); \
         found on lines: {no_sorry_lines:?}"
    );
    // Negative landmark: the trivial-placeholder stubs are gone.
    // PMAT-029 replaced `by trivial` with real proofs; if anyone
    // re-stubs the theorems, this fires.
    let trivial_lines: Vec<&str> = src
        .lines()
        .filter(|l| {
            let trimmed = l.trim_start();
            !trimmed.starts_with("--") && !trimmed.starts_with("/-")
        })
        .filter(|l| l.contains("by trivial") || l.contains(":= by trivial"))
        .collect();
    assert!(
        trivial_lines.is_empty(),
        "PyIntArith.lean must not carry `by trivial` placeholders (PMAT-029 discharged the stub trio); \
         found on lines: {trivial_lines:?}"
    );
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
