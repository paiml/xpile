//! XPILE-QUORUM-006 (PMAT-1432) — the Runtime stratum's fixture half must rest
//! on a test that LOADS the fixture, not on a file that names a contract ID.
//!
//! ## What went wrong
//!
//! The §14.4 quorum has four strata. Three of them — Semantic (`lean_theorem:`
//! refs), Symbolic (`kani_harness:` refs) and Extrinsic (roadmap mentions) — are
//! satisfied by writing YAML and prose. Runtime is the only one that can
//! distinguish a MODELLED claim from a SHIPPED one, which is precisely
//! PMAT-1431's lesson: for every contract, ask what ties its model to code.
//!
//! PMAT-1367 tightened Pass B of that stratum, and wrote the reason down in the
//! collector's own doc comment: *"Naming a contract ID is not evidence of
//! anything."* It then left Pass A — the fixture corpus — as pure name
//! matching, and explicitly recorded the omission as a feature ("Kept
//! BYTE-IDENTICAL ... no existing vote can disappear"). Meanwhile
//! `docs/roadmaps/roadmap.yaml` records eight fixtures added in one batch, "one
//! per remaining 3-stratum contract — each carrying its contract ID in a header
//! comment", to lift "each from 3-stratum to full 4-stratum", with the tests
//! that would load them deferred as "XPILE-*-RUNTIME-001 follow-ons". Those
//! follow-ons were never written. Five of the files are named by no Rust source
//! anywhere under `crates/`, and for FOUR contracts one of them WAS the entire
//! Runtime stratum.
//!
//! ## Why this file names no fixture
//!
//! A gate that spells the filenames it is about would make every one of them
//! "loaded" — the scanner would find the names in this file and hand back the
//! votes it exists to remove. Every roster below is DERIVED from the tree at
//! run time. (PMAT-1416: a call-site scanner must not print call sites.)

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A fixture that no test loads must say so in its own text, so the next reader
/// is not misled by a header claiming a Runtime-stratum vote the file no longer
/// casts. Checked in BOTH directions below.
const NO_VOTE_MARKER: &str = "xpile-runtime-vote: none";

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

fn fixtures_dir() -> PathBuf {
    workspace_root().join("crates/xpile/tests/fixtures")
}

/// A unique scratch dir per CALL — not per test. Two tests sharing one path
/// race under the default parallel harness, and an intermittent red on a
/// REQUIRED context is worse than the defect it would have caught.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "xpile-quorum-fixture-{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Every `*.rs` source under `crates/`, concatenated, EXCLUDING the fixture
/// corpus itself. This is deliberately WIDER than the scan the binary performs
/// (`--fixture-loader-dir` + `--witness-dir`): if the two ever disagree about
/// which fixtures are loaded, `fixture_votes_match_the_loaded_set` reds and
/// says which side to widen.
fn all_rust_sources_outside_the_corpus() -> String {
    let mut out = String::new();
    let excluded = fixtures_dir().canonicalize().expect("fixtures dir exists");
    fn walk(dir: &Path, excluded: &Path, out: &mut String) {
        if dir.canonicalize().is_ok_and(|c| c == excluded) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, excluded, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
    }
    walk(&workspace_root().join("crates"), &excluded, &mut out);
    out
}

/// Contract IDs, derived from `contracts/*.yaml` `metadata.id:` lines rather
/// than hardcoded — a hardcoded roster silently stops covering a new contract.
fn contract_ids() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let dir = workspace_root().join("contracts");
    for entry in std::fs::read_dir(&dir).expect("contracts dir").flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("id:") {
                let id = rest.trim().trim_matches('"').to_string();
                if id.starts_with("C-") {
                    ids.insert(id);
                    break;
                }
            }
        }
    }
    assert!(
        ids.len() >= 30,
        "expected the full contract corpus, found {} ids — a shrunken universe \
         would make every assertion below vacuous",
        ids.len()
    );
    ids
}

/// Top-level fixture files (Pass A is non-recursive, so directories are out),
/// mapped to `(text, is_loaded_by_some_test)`.
fn fixture_table() -> BTreeMap<String, (String, bool)> {
    let sources = all_rust_sources_outside_the_corpus();
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(fixtures_dir())
        .expect("fixtures dir")
        .flatten()
    {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let loaded = sources.contains(name);
        out.insert(name.to_string(), (text, loaded));
    }
    assert!(
        out.len() >= 500,
        "expected the live fixture corpus, found {} files",
        out.len()
    );
    out
}

/// `xpile quorum --json` rows, with Pass B ZEROED (an existing but empty
/// witness dir) so `runtime` is exactly the fixture pass and can be compared
/// against an independently derived expectation.
fn fixture_only_runtime_counts() -> BTreeMap<String, u64> {
    let root = workspace_root();
    let empty = scratch("empty-witness-dir");
    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
            "--fixtures-dir",
            fixtures_dir().to_str().unwrap(),
            "--witness-dir",
            empty.to_str().unwrap(),
            "--fixture-loader-dir",
            root.join("crates/xpile/tests").to_str().unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    let _ = std::fs::remove_dir_all(&empty);
    assert!(
        out.status.success(),
        "xpile quorum failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("quorum --json is valid JSON");
    let mut map = BTreeMap::new();
    for row in v["contracts"].as_array().expect("contracts array") {
        map.insert(
            row["id"].as_str().expect("id").to_string(),
            row["runtime"].as_u64().expect("runtime"),
        );
    }
    map
}

/// THE GATE. Per contract, the fixture-pass Runtime count must equal the number
/// of fixtures that BOTH name the ID AND are named by a Rust source outside the
/// corpus.
///
/// Too high → a fixture nothing loads is voting again (the PMAT-1432 defect).
/// Too low  → a fixture a test really does load lost its vote, or the binary's
/// bounded loader scan no longer sees a reference this wide scan does.
#[test]
fn fixture_votes_match_the_loaded_set() {
    let table = fixture_table();
    let live = fixture_only_runtime_counts();
    for id in contract_ids() {
        let expected = table
            .values()
            .filter(|(text, loaded)| *loaded && text.contains(&id))
            .count() as u64;
        let actual = *live
            .get(&id)
            .unwrap_or_else(|| panic!("{id} missing from quorum --json"));
        assert_eq!(
            actual, expected,
            "{id}: fixture-pass Runtime = {actual}, but {expected} fixture(s) \
             both name it and are loaded by a test. Higher means a fixture no \
             test loads is casting a vote again — the whole point of PMAT-1432. \
             Lower means a real vote was dropped, or the binary's \
             --fixture-loader-dir scan no longer reaches a source this test's \
             workspace-wide scan does; widen the flag, do not weaken this."
        );
    }
}

/// Guard against a vacuous pass (PMAT-1396: a negative over an EMPTY
/// enumeration is satisfied for free). At the time of writing FIVE fixtures
/// name a contract ID and are loaded by nothing. If that set ever empties
/// legitimately — every fixture wired to a test — this assertion is the one to
/// delete, deliberately, with the count recorded in the commit message.
#[test]
fn the_unloaded_fixture_set_is_non_empty_and_every_member_discloses_it() {
    let ids = contract_ids();
    let unloaded: Vec<String> = fixture_table()
        .into_iter()
        .filter(|(_, (text, loaded))| !*loaded && ids.iter().any(|id| text.contains(id)))
        .map(|(name, _)| name)
        .collect();
    assert!(
        !unloaded.is_empty(),
        "no fixture names a contract ID without a test loading it — if that is \
         genuinely true now, this assertion has served its purpose and should \
         be removed on purpose rather than left to pass over nothing"
    );
    let table = fixture_table();
    for name in &unloaded {
        let (text, _) = &table[name];
        assert!(
            text.contains(NO_VOTE_MARKER),
            "{name} names a contract ID, no test loads it, and its text does \
             not carry the `{NO_VOTE_MARKER}` disclosure. A fixture that reads \
             as Runtime evidence while casting no vote is the same misfiling \
             one layer down: add the marker, or wire a test that loads it."
        );
    }
}

/// The other direction, so the marker cannot rot: a fixture that IS loaded must
/// not still claim it casts no vote.
#[test]
fn no_loaded_fixture_carries_a_stale_no_vote_marker() {
    for (name, (text, loaded)) in fixture_table() {
        if loaded && text.contains(NO_VOTE_MARKER) {
            panic!(
                "{name} is loaded by a test but still carries `{NO_VOTE_MARKER}`. \
                 The disclosure is now false — remove it."
            );
        }
    }
}

/// `C-XLATE-LEAN-TO-RUST` is the extreme case and the reason this gate exists:
/// 40 equations, 33 Lean theorems and 10 Kani harnesses modelling how Lean
/// constructs lower to Rust, with NO Rust behind any of it. No registered
/// frontend claims `.lean`, so no `SourceLang::Lean` module can be produced and
/// the contract's Runtime stratum is empty by construction.
///
/// This is the `notation_surface` idiom (PMAT-1431) in miniature: it must RED
/// the day a Lean frontend lands, forcing the disclosure — here, and in
/// `book/src/reference/contracts.md` — to move rather than quietly stay wrong.
#[test]
fn no_frontend_claims_dot_lean_so_the_lean_to_rust_contract_has_no_runtime_stratum() {
    let dir = scratch("lean-input");
    let src = dir.join("probe_input.lean");
    std::fs::write(&src, "def double (n : Int) : Int := n + n\n").expect("write lean probe");
    let out = Command::new(xpile_bin())
        .args(["transpile", src.to_str().unwrap(), "--target", "rust"])
        .output()
        .expect("run xpile transpile");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        !out.status.success(),
        "`xpile transpile *.lean --target rust` SUCCEEDED. A Lean frontend has \
         landed, which is good news and reds this gate on purpose: \
         C-XLATE-LEAN-TO-RUST now has a code lane, so give it a fixture a test \
         loads, and rewrite the C-XLATE-LEAN-TO-RUST paragraph in \
         book/src/reference/contracts.md, which currently discloses that the \
         direction is modelled-only.\nstdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let info = Command::new(xpile_bin())
        .arg("info")
        .output()
        .expect("run xpile info");
    let info = String::from_utf8_lossy(&info.stdout);
    // The code lane's FRONTEND block only — the backend list one section down
    // legitimately names Lean (`--target lean` is real and shipped).
    let block = info
        .split_once("frontends (")
        .and_then(|(_, rest)| rest.split_once("backends ("))
        .map(|(block, _)| block.to_string())
        .expect("code-lane frontend block in `xpile info`");
    assert!(
        block.contains("python") && block.contains("bashrs"),
        "the frontend block was not located — the assertion below would pass \
         over nothing.\n{block}"
    );
    assert!(
        !block.contains("lean"),
        "the code lane's frontend registry now lists Lean; see the message \
         above.\n{block}"
    );

    assert_eq!(
        fixture_only_runtime_counts()["C-XLATE-LEAN-TO-RUST"],
        0,
        "C-XLATE-LEAN-TO-RUST reported a fixture Runtime vote. Nothing can \
         execute a Lean-source lowering that does not exist; a file in the \
         corpus that names the ID is not evidence of one."
    );
}
