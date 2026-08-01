//! XPILE-PROOFSEAM-001 — a proof that cannot be falsified by xpile's own code
//! may not be described as machine-checking xpile (PMAT-1512).
//!
//! ## The measurement this file exists to keep honest
//!
//! Measured 2026-08-01 at `aba0546a`, before this slice:
//!
//! * all **36** modules in `contracts/lean/` import exactly `Lake` — nothing
//!   from xpile, which a Lean file could not import anyway;
//! * `kani_verify.rs` materialised every harness into a temp crate whose
//!   `Cargo.toml` had **no `[dependencies]` section**, so a harness could not
//!   reference an xpile crate even in principle.
//!
//! So 489 Lean theorems and 108 Kani proofs verified hand-written
//! RE-IMPLEMENTATIONS, and **no proof in this repository could be turned red by
//! a wrong lowering**. `contracts/kani/bashrs.rs` discloses this in its own
//! header (*"Standalone Rust module reproducing the property under test"*); the
//! project-level claim that the lane machine-checks xpile did not.
//!
//! An unfalsifiable guarantee is worse than a false one. A false claim can be
//! disproved; this one could not be disturbed by any evidence at all.
//!
//! ## What changed, and what deliberately did not
//!
//! `real_binop_governed_set.rs` declares `kani-deps: xpile-meta-hir` and proves
//! a property of the SHIPPED `binop_is_int_arith`. Removing `BinOp::Shl` from
//! the governed set turns it FAILED in 27 ms; restoring it returns SUCCESSFUL.
//!
//! **That is one proof out of over a hundred.** The rest still verify models,
//! and this gate exists so that fact stays written down: `most_proofs_are_still
//! _models_and_the_docs_must_say_so` fails the moment a document rounds the
//! claim up. The honest sentence is *"the contracts are machine-checked as
//! models; one property is machine-checked against the emitter"*, and it stops
//! being honest the day someone drops the qualifier.
//!
//! ## Why the seam is scalar
//!
//! Two properties over real code were attempted first and abandoned with the
//! measurement recorded, because "we tried and it did not work" is a result:
//!
//! | attempt | outcome |
//! |---|---|
//! | `Function::uses_int_arithmetic`, depth-2 body | no result in 10 min; `expr_has_int_arith` ⇄ `stmt_has_int_arith` unwound past iteration 100 |
//! | `xpile_backend::strip_contract_citations` | linked, 1913 checks inside the real fn, then stalled in `str::pattern::TwoWaySearcher` |
//! | `binop_is_int_arith` over all 19 operators | **SUCCESSFUL, 27 ms** |
//!
//! Kani is strong on fixed-width scalars and fieldless enums and weak on
//! recursive heap types — which is exactly what `Expr` is. So the recipe for
//! growing this lane is to expose scalar predicates as seams, not to point Kani
//! at the tree walk and hope.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/xpile → repo root")
        .to_path_buf()
}

fn harness_sources() -> Vec<(String, String)> {
    let dir = repo_root().join("contracts").join("kani");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("contracts/kani must exist: {}", dir.display());
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if let Ok(src) = std::fs::read_to_string(&p) {
            out.push((name, src));
        }
    }
    out.sort();
    out
}

/// The crates a harness declares — the same parse the runner performs.
fn declared_deps(src: &str) -> Vec<String> {
    src.lines()
        .take_while(|l| l.starts_with("//!") || l.trim().is_empty())
        .filter_map(|l| {
            l.trim_start_matches("//!")
                .trim()
                .strip_prefix("kani-deps:")
        })
        .flat_map(|list| {
            list.split(',')
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Harnesses that verify xpile's own code, as opposed to a model of it.
fn load_bearing() -> Vec<String> {
    harness_sources()
        .into_iter()
        .filter(|(_, src)| !declared_deps(src).is_empty())
        .map(|(name, _)| name)
        .collect()
}

/// PROPERTY 1 — the lane must never return to being entirely unfalsifiable.
///
/// This is the property whose absence let 108 proofs verify re-implementations
/// for months without anything noticing.
#[test]
fn at_least_one_proof_is_wired_to_the_real_emitter() {
    let real = load_bearing();
    assert!(
        !real.is_empty(),
        "every Kani harness in contracts/kani/ builds against a temp crate with no \
         dependencies, so not one of them can be turned red by a wrong lowering. That \
         is the state PMAT-1512 repaired; at least one harness must declare \
         `//! kani-deps: <crate>` and prove something about the shipped code."
    );
    eprintln!("proofs wired to real crates: {real:?}");
}

/// PROPERTY 2 — every declared dependency must name a crate that exists.
///
/// A harness declaring a crate that is not there would fail to build, and a
/// build failure in the proof lane is reported as "kani absent, skipping" on a
/// host without kani — a hollow skip of exactly the kind XPILE-SKIPGUARD-001
/// exists for.
#[test]
fn every_declared_dependency_resolves_to_a_workspace_crate() {
    let root = repo_root();
    let mut offences = Vec::new();
    for (name, src) in harness_sources() {
        for d in declared_deps(&src) {
            if !root.join("crates").join(&d).join("Cargo.toml").is_file() {
                offences.push(format!(
                    "  {name} declares `kani-deps: {d}` — no such crate"
                ));
            }
        }
    }
    assert!(offences.is_empty(), "{}", offences.join("\n"));
}

/// PROPERTY 3 — the load-bearing harness must actually reference its crate.
///
/// Declaring a dependency and then not using it produces a proof that still
/// verifies a model while advertising that it does not. The declaration is
/// cheap to write and worthless on its own.
#[test]
fn a_wired_harness_actually_calls_into_the_crate_it_declares() {
    let mut offences = Vec::new();
    for (name, src) in harness_sources() {
        for d in declared_deps(&src) {
            let ident = d.replace('-', "_");
            let uses =
                src.contains(&format!("use {ident}::")) || src.contains(&format!("{ident}::"));
            if !uses {
                offences.push(format!(
                    "  {name} declares `kani-deps: {d}` but never names `{ident}::` — the \
                     dependency is decoration and the proof is still a model"
                ));
            }
        }
    }
    assert!(offences.is_empty(), "{}", offences.join("\n"));
}

/// PROPERTY 4 — ANTI-VACUITY. A parse that stops matching would report an empty
/// corpus and pass PROPERTY 2 and 3 for free.
#[test]
fn the_harness_corpus_is_not_empty() {
    let all = harness_sources();
    assert!(
        all.len() >= 20,
        "found only {} harness file(s) under contracts/kani/; the corpus walk has broken \
         and every property here is vacuous",
        all.len()
    );
    // And the declaration parser must be able to see a declaration at all.
    let probe = "//! kani-deps: xpile-meta-hir, xpile-backend\n//! more prose\n";
    assert_eq!(
        declared_deps(probe),
        vec!["xpile-meta-hir".to_string(), "xpile-backend".to_string()],
        "the kani-deps parser no longer reads a declaration it is looking straight at"
    );
    assert!(
        declared_deps("//! ordinary header with no declaration\n").is_empty(),
        "the parser invents a dependency where none is declared"
    );
}

/// PROPERTY 5 — THE HONESTY CLAUSE, and the reason this file is a gate rather
/// than a comment.
///
/// One proof out of more than a hundred is wired to the emitter. Any document
/// that describes the proof lane must not state or imply that xpile's own code
/// is machine-checked without saying which part. This is quantified over the
/// documents that make the claim, and it reds when a qualifier is dropped.
#[test]
fn no_document_claims_the_whole_emitter_is_machine_checked() {
    let root = repo_root();
    let pages = [
        "README.md",
        "docs/specifications/xpile-spec.md",
        "book/src/introduction.md",
    ];
    let total = harness_sources().len();
    let real = load_bearing().len();
    assert!(
        real < total,
        "every harness is now wired to a real crate — delete this property, it has \
         become false in the good direction ({real}/{total})"
    );

    let mut offences = Vec::new();
    for page in pages {
        let p = root.join(page);
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let l = line.to_ascii_lowercase();
            if !l.contains("machine-checked") && !l.contains("machine checked") {
                continue;
            }
            // A sentence that says WHAT is machine-checked is fine. One that
            // leaves it open reads as a claim about the emitter.
            let qualified = ["model", "contract", "as models", "one property", "seam"]
                .iter()
                .any(|q| l.contains(q));
            if !qualified {
                offences.push(format!(
                    "  {page}:{} — {:?}\n      says machine-checked without naming WHAT. \
                     {real} of {total} Kani harnesses verify xpile's code; the rest verify \
                     models of it.",
                    n + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(offences.is_empty(), "{}", offences.join("\n"));
}
