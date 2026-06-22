//! PMAT-475 (R6 contract-integrity, slice 2): the citation→contract gate.
//!
//! The capability-vs-contract drift (audit-design.md §6, five-whys) was rooted
//! in there being NO enforcement that emitted `// xpile-contract: <ID>` lines
//! actually resolve to an on-disk contract — that is exactly how C-C-INT-ARITH /
//! C-XLATE-PY-DICT-TO-HASHMAP were once cited before their YAMLs existed. This
//! gate transpiles the whole fixture corpus and:
//!   (a) FAILS if any emitted citation references a contract not present in
//!       `contracts/*.yaml` (no phantom citations — the original sin), and
//!   (b) regression-guards that the slice-1 type-translation citations
//!       (str/list/dict + int-arith) stay actively emitted, so the wiring
//!       cannot silently regress to int-arith-only.
//!   (c) [R6-slice5] per-fixture EXPECTED-contracts: corpus-wide (b) cannot
//!       catch a single contract-bearing fixture that silently drops its
//!       citation — another fixture keeps the corpus set satisfied. (c) pins
//!       canonical witness fixtures to the citation their construct must emit,
//!       so severing one fixture's citation FAILS even if (b) still passes.
//!
//! It is the deterministic replacement for the frozen Diamond-depth pressure:
//! a construct can no longer ship citing a contract that does not exist.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// All `id:` values declared in `contracts/*.yaml` (both the `C-*` governing
/// contracts and their `QA-*` siblings).
fn on_disk_contract_ids() -> HashSet<String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let mut ids = HashSet::new();
    for entry in fs::read_dir(&dir).expect("contracts/ dir readable") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("id:") {
                ids.insert(rest.trim().to_string());
            }
        }
    }
    ids
}

#[test]
fn every_emitted_citation_resolves_to_an_on_disk_contract() {
    let ids = on_disk_contract_ids();
    assert!(
        ids.contains("C-PY-INT-ARITH") && ids.contains("C-XLATE-PY-STR-TO-RUST-STRING"),
        "sanity: contract id set should have loaded from contracts/, got {} ids",
        ids.len()
    );

    let bin = env!("CARGO_BIN_EXE_xpile");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let mut transpiled = 0usize;
    let mut total_citations = 0usize;
    let mut cited: HashSet<String> = HashSet::new();
    let mut phantom: Vec<String> = Vec::new();
    // Per-fixture emitted citations (filename -> cited ids) for the (c) gate.
    let mut per_fixture: HashMap<String, HashSet<String>> = HashMap::new();

    for entry in fs::read_dir(&fixtures).expect("tests/fixtures dir readable") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("py") {
            continue;
        }
        let out = Command::new(bin)
            .args(["transpile", path.to_str().unwrap()])
            .output()
            .expect("xpile binary runs");
        // Skip any fixture that does not transpile under the default (Rust)
        // target — this gate is about citations on emitted code, not coverage.
        if !out.status.success() {
            continue;
        }
        transpiled += 1;
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let fixture_cited = per_fixture.entry(fname.clone()).or_default();
        let rust = String::from_utf8_lossy(&out.stdout);
        for line in rust.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("// xpile-contract:") {
                let id = rest.trim().to_string();
                total_citations += 1;
                if !ids.contains(&id) {
                    phantom.push(format!("{fname} cites non-existent contract `{id}`"));
                }
                fixture_cited.insert(id.clone());
                cited.insert(id);
            }
        }
    }

    assert!(
        transpiled > 100,
        "expected to transpile a large corpus, only {transpiled} fixtures succeeded"
    );
    assert!(
        total_citations > 0,
        "expected the corpus to emit contract citations, found none"
    );

    // (a) No phantom citations — the original R6 sin.
    assert!(
        phantom.is_empty(),
        "PMAT-475: every emitted `// xpile-contract:` must resolve to a \
         contracts/*.yaml id. Offenders:\n{}",
        phantom.join("\n")
    );

    // (b) Slice-1 wiring must stay live: str/list/dict + int-arith are actively
    // cited somewhere in the corpus (guards against a regression to int-only).
    for required in [
        "C-PY-INT-ARITH",
        "C-XLATE-PY-STR-TO-RUST-STRING",
        "C-XLATE-PY-LIST-TO-VEC",
        "C-XLATE-PY-DICT-TO-HASHMAP",
        "C-PY-FLOAT-ARITH",
        "C-XLATE-PY-SET-TO-HASHSET",
        // PMAT-879 (R6): class/dataclass translation is now wired + cited.
        "C-XLATE-PY-CLASS-TO-STRUCT",
        // PMAT-880 (R6): fixed-arity tuple translation is now wired + cited.
        "C-XLATE-PY-TUPLE-TO-RUST-TUPLE",
        // PMAT-881 (R6): Optional → Option translation is now wired + cited.
        "C-XLATE-PY-OPTIONAL-TO-OPTION",
    ] {
        assert!(
            cited.contains(required),
            "PMAT-475: expected the corpus to cite `{required}` (slice-1 wiring \
             regressed?). Cited contracts: {cited:?}"
        );
    }

    // (c) [R6-slice5] Per-fixture EXPECTED-contracts. Each entry is a canonical
    // witness fixture whose construct is contract-bearing; the listed contract
    // id(s) MUST appear in THAT fixture's emitted citations. Unlike (b) — which
    // only requires a contract be cited somewhere corpus-wide — this fails the
    // moment one witness fixture silently drops its citation. Verified against
    // the live transpiler when authored; if a fixture is legitimately retired,
    // move the witness to another fixture that exercises the same construct.
    const EXPECTED: &[(&str, &[&str])] = &[
        ("add.py", &["C-PY-INT-ARITH"]),
        ("center.py", &["C-XLATE-PY-STR-TO-RUST-STRING"]),
        ("append_demo.py", &["C-XLATE-PY-LIST-TO-VEC"]),
        ("bool_dict_key.py", &["C-XLATE-PY-DICT-TO-HASHMAP"]),
        ("augmented_set_ops.py", &["C-XLATE-PY-SET-TO-HASHSET"]),
        ("bool_float.py", &["C-PY-FLOAT-ARITH"]),
        (
            "class_to_struct_contract.py",
            &["C-XLATE-PY-CLASS-TO-STRUCT"],
        ),
        ("tuple_contract.py", &["C-XLATE-PY-TUPLE-TO-RUST-TUPLE"]),
        ("optional_return.py", &["C-XLATE-PY-OPTIONAL-TO-OPTION"]),
        // Multi-contract witnesses — EVERY listed id must co-occur in the one
        // fixture (exercises the all-expected-present path, not just any-cited).
        (
            "comp_typed_element.py",
            &[
                "C-XLATE-PY-CLASS-TO-STRUCT",
                "C-XLATE-PY-LIST-TO-VEC",
                "C-XLATE-PY-TUPLE-TO-RUST-TUPLE",
            ],
        ),
        (
            "contract_citation_types.py",
            &[
                "C-XLATE-PY-DICT-TO-HASHMAP",
                "C-XLATE-PY-LIST-TO-VEC",
                "C-XLATE-PY-STR-TO-RUST-STRING",
            ],
        ),
    ];
    let mut missing_expected: Vec<String> = Vec::new();
    for (fixture, required_ids) in EXPECTED {
        match per_fixture.get(*fixture) {
            None => missing_expected.push(format!(
                "{fixture}: expected to transpile and cite {required_ids:?}, but it \
                 did not transpile successfully"
            )),
            Some(got) => {
                for rid in *required_ids {
                    if !got.contains(*rid) {
                        missing_expected.push(format!(
                            "{fixture}: must cite `{rid}` (per-fixture expected-contract) \
                             but did not — emitted {got:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        missing_expected.is_empty(),
        "R6-slice5: per-fixture expected-contract citation(s) missing — a \
         contract-bearing construct dropped its citation:\n{}",
        missing_expected.join("\n")
    );
}
