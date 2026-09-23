//! XPILE-LEAN-PAIRING-001 — every proof file under `contracts/` is claimed by
//! a contract, or says why not (xpile#2119).
//!
//! ## What this exists to catch
//!
//! `refinement_proofs.rs` checks one direction: every `lean_theorem:` a
//! contract cites exists in the `lean_file:` it names. Nothing checked the
//! other direction. A `.lean` file full of proven theorems that no contract
//! cites is invisible to `xpile quorum`, to the Semantic stratum, and to every
//! reader who starts from a contract, so it can rot or be deleted without any
//! gate noticing.
//!
//! The PVL-001 v2 audit put this at "28 of 42 Lean files unpaired". Measured on
//! 2026-09-23 with the rule this gate uses (a contract's `lean_file:` names
//! the file), 36 of 42 tracked `.lean` files are cited. Of the other six, four
//! carry no theorem at all (two `lakefile.lean`, the `Models.lean` package
//! root, and a test fixture outside `contracts/`). The two that matter are
//! `Models/Basic.lean` and `Models/SimpleLinear.lean`, eight proven theorems
//! that no contract cites. They are listed in [`UNCITED`] with the reason.
//!
//! ## The rule is a property, not a list
//!
//! A file is in scope when it sits under `contracts/` and declares at least
//! one `theorem` or `lemma`. That is detected from the file, so a lakefile or
//! an import-only root is out of scope by what it IS, and a new proof file is
//! in scope the day it lands. The only typed list is [`UNCITED`], and every
//! entry in it must stay live: present, theorem-bearing, and still uncited.
//!
//! `std::fs` + `serde_yaml` only: no git, no `lake`, so it cannot skip.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Theorem-bearing files under `contracts/` that no contract cites, each with
/// the reason. Adding a line here is a disclosure, not a fix.
const UNCITED: &[(&str, &str)] = &[
    (
        "contracts/lean-models/Models/Basic.lean",
        "Mathlib lane: the constant model's SSE optimum (sse_decomp, sse_mean_le, \
         sse_eq_mean_iff, sse_lt_of_ne). No xpile contract claims it.",
    ),
    (
        "contracts/lean-models/Models/SimpleLinear.lean",
        "Mathlib lane: simple linear regression (slr_*). It does NOT import \
         GeneralLinear.lean, so citing it from ols-model-uniqueness-v1 as a \
         special case would claim a derivation the file does not carry (PMAT-1472).",
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn rel(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            // Lake's build and package caches are not sources.
            if name != ".lake" && name != "build" {
                walk(&path, out);
            }
        } else if name.ends_with(".lean") {
            out.push(path);
        }
    }
}

/// True when a line declares a theorem or lemma, allowing leading attributes
/// (`@[simp]`) and modifiers (`private`, `protected`).
fn declares_theorem(line: &str) -> bool {
    let mut rest = line.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("@[") {
            match after.find(']') {
                Some(end) => rest = after[end + 1..].trim_start(),
                None => return false,
            }
        } else if let Some(after) = rest
            .strip_prefix("private ")
            .or_else(|| rest.strip_prefix("protected "))
        {
            rest = after.trim_start();
        } else {
            break;
        }
    }
    rest.starts_with("theorem ") || rest.starts_with("lemma ")
}

/// Every `.lean` file under `contracts/` → its theorem/lemma count.
fn proof_files() -> BTreeMap<String, usize> {
    let mut files = Vec::new();
    walk(&workspace_root().join("contracts"), &mut files);
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", rel(&p)));
            (
                rel(&p),
                text.lines().filter(|l| declares_theorem(l)).count(),
            )
        })
        .collect()
}

fn collect_lean_files(v: &serde_yaml::Value, out: &mut BTreeSet<String>) {
    match v {
        serde_yaml::Value::Mapping(m) => {
            for (k, val) in m {
                if k.as_str() == Some("lean_file") {
                    if let Some(s) = val.as_str() {
                        out.insert(s.trim().to_string());
                    }
                }
                collect_lean_files(val, out);
            }
        }
        serde_yaml::Value::Sequence(s) => s.iter().for_each(|x| collect_lean_files(x, out)),
        _ => {}
    }
}

/// Every `lean_file:` value cited by any `contracts/*.yaml`, parsed, not grepped.
fn cited() -> BTreeSet<String> {
    let dir = workspace_root().join("contracts");
    let mut out = BTreeSet::new();
    let mut yamls = 0usize;
    for entry in fs::read_dir(&dir).expect("read contracts/") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|x| x == "yaml" || x == "yml") {
            yamls += 1;
            let doc: serde_yaml::Value =
                serde_yaml::from_str(&fs::read_to_string(&path).expect("read contract"))
                    .unwrap_or_else(|e| panic!("{} is not valid YAML: {e}", rel(&path)));
            collect_lean_files(&doc, &mut out);
        }
    }
    assert!(
        yamls >= 30,
        "only {yamls} contract YAML(s) under contracts/ — scan has gone blind"
    );
    out
}

#[test]
fn every_theorem_bearing_lean_file_is_cited_by_a_contract() {
    let files = proof_files();
    let cited = cited();
    let exempt: BTreeSet<&str> = UNCITED.iter().map(|(f, _)| *f).collect();

    let in_scope: Vec<&String> = files
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(f, _)| f)
        .collect();
    assert!(
        in_scope.len() >= 30 && cited.len() >= 30,
        "vacuous: {} theorem-bearing file(s), {} cited — expected the full proof lane",
        in_scope.len(),
        cited.len()
    );

    let orphans: Vec<String> = in_scope
        .iter()
        .filter(|f| !cited.contains(f.as_str()) && !exempt.contains(f.as_str()))
        .map(|f| format!("  {f} ({} theorem(s))", files[*f]))
        .collect();
    assert!(
        orphans.is_empty(),
        "XPILE-LEAN-PAIRING-001: {} Lean file(s) prove theorems that no contract \
         cites:\n{}\n\nCite the file from its contract's `lean_file:`, or add it to \
         UNCITED in this test with the reason no contract claims it.",
        orphans.len(),
        orphans.join("\n")
    );
}

#[test]
fn every_cited_lean_file_exists() {
    let root = workspace_root();
    let dangling: Vec<String> = cited()
        .into_iter()
        .filter(|f| !root.join(f).is_file())
        .collect();
    assert!(
        dangling.is_empty(),
        "XPILE-LEAN-PAIRING-001: contracts cite `lean_file:` path(s) that do not \
         exist: {dangling:?}"
    );
}

/// An exemption that no longer applies is a hole: it would silently cover a
/// file that later loses its citation. Each must be present, theorem-bearing,
/// and still uncited.
#[test]
fn every_uncited_exemption_is_live() {
    let files = proof_files();
    let cited = cited();
    let stale: Vec<String> = UNCITED
        .iter()
        .filter_map(|(f, _)| match files.get(*f) {
            None => Some(format!("  {f}: no such file")),
            Some(0) => Some(format!("  {f}: declares no theorem, so it is out of scope")),
            Some(_) if cited.contains(*f) => Some(format!("  {f}: a contract now cites it")),
            Some(_) => None,
        })
        .collect();
    assert!(
        stale.is_empty(),
        "XPILE-LEAN-PAIRING-001: stale UNCITED exemption(s) — delete them:\n{}",
        stale.join("\n")
    );
}
