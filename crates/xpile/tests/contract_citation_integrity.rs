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
use std::path::{Path, PathBuf};
use std::process::Command;

use depyler_frontend::PythonFrontend;
use xpile_frontend::Frontend;
use xpile_meta_hir::{Function, Item, Module};

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
        // PMAT-935 (R6): pure-bool translation is now wired + cited.
        "C-XLATE-PY-BOOL-TO-RUST-BOOL",
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
        ("bool_contract.py", &["C-XLATE-PY-BOOL-TO-RUST-BOOL"]),
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

/// PMAT-907 (R6 contract-integrity, Day 8): extend the citation gate to the
/// EMITTED hybrid shim file. The corpus gate above only scans `transpile`
/// stdout (the Rust backend's codegen); the `xpile hybrid --emit-shims` path
/// emits a SEPARATE `ffi_shims.rs` whose `// xpile-contract:` lines were never
/// under the gate — exactly the kind of file a Shell/C shim could ship citing a
/// non-existent contract (the original R6 drift, one layer out). This test runs
/// `xpile hybrid <dir> --emit-shims <file>` on every hybrid fixture, scans the
/// emitted shim file, and FAILS on any citation that does not resolve to a
/// `contracts/*.yaml` id — and asserts the C boundary's `C-FFI-CPYTHON-EXT`
/// citation stays live so the wiring can't silently regress.
#[test]
fn every_emitted_hybrid_shim_citation_resolves() {
    let ids = on_disk_contract_ids();
    // Both governing FFI-shim contracts must be on disk: the C-extension one
    // (cited by the live hybrid_sum fixture) and the Shell one authored this
    // slice (cited by emit_shell_shim once a Shell-frontend producer lands).
    assert!(
        ids.contains("C-FFI-CPYTHON-EXT"),
        "sanity: C-FFI-CPYTHON-EXT must be on disk, got {} ids",
        ids.len()
    );
    assert!(
        ids.contains("C-FFI-SHELL-SUBPROCESS"),
        "PMAT-907: C-FFI-SHELL-SUBPROCESS must be authored in contracts/ — \
         emit_shell_shim now cites it, so a missing YAML is the phantom-citation \
         sin the gate exists to prevent"
    );

    let bin = env!("CARGO_BIN_EXE_xpile");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));

    let mut emitted = 0usize;
    let mut total_citations = 0usize;
    let mut cited: HashSet<String> = HashSet::new();
    let mut phantom: Vec<String> = Vec::new();

    for entry in fs::read_dir(&fixtures).expect("tests/fixtures dir readable") {
        let dir = entry.unwrap().path();
        // A hybrid fixture is a directory whose name starts `hybrid_`.
        if !dir.is_dir()
            || !dir
                .file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.starts_with("hybrid_"))
                .unwrap_or(false)
        {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let out_file = tmp.join(format!("{name}_ffi_shims.rs"));
        let _ = fs::remove_file(&out_file);

        let out = Command::new(bin)
            .args(["hybrid", dir.to_str().unwrap(), "--emit-shims"])
            .arg(&out_file)
            .output()
            .expect("xpile hybrid runs");
        // Fixtures with no resolvable FFI boundary (same-language siblings) or a
        // deliberately-unresolved boundary either emit nothing or exit non-zero;
        // this gate is about citations on EMITTED shims, not coverage.
        if !out.status.success() || !out_file.exists() {
            continue;
        }
        emitted += 1;
        let shims = fs::read_to_string(&out_file).unwrap();
        for line in shims.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("// xpile-contract:") {
                let id = rest.trim().to_string();
                total_citations += 1;
                if !ids.contains(&id) {
                    phantom.push(format!(
                        "{name}/ffi_shims.rs cites non-existent contract `{id}`"
                    ));
                }
                cited.insert(id);
            }
        }
    }

    assert!(
        emitted > 0,
        "PMAT-907: expected at least one hybrid fixture to emit shims (hybrid_sum \
         is a live Python→C boundary); none did"
    );
    assert!(
        total_citations > 0,
        "PMAT-907: emitted shim files carried no contract citations — the C \
         boundary's citation regressed?"
    );

    // (a) No phantom citations in emitted shim files — the R6 sin, one layer out.
    assert!(
        phantom.is_empty(),
        "PMAT-907: every `// xpile-contract:` in an EMITTED ffi_shims.rs must \
         resolve to a contracts/*.yaml id. Offenders:\n{}",
        phantom.join("\n")
    );

    // (b) The live C boundary keeps citing its contract (regression guard).
    assert!(
        cited.contains("C-FFI-CPYTHON-EXT"),
        "PMAT-907: expected the emitted hybrid shims to cite `C-FFI-CPYTHON-EXT` \
         (hybrid_sum's Python→C boundary). Cited: {cited:?}"
    );
}

/// Every `Function` reachable in the codegen surface of a Module: the
/// top-level `Item::Function`s plus the instance/static methods inside an
/// `Item::Struct`. This is EXACTLY the set of functions `emit_contract_citations`
/// runs over in `xpile-rust-codegen` (struct definitions and consts/enums carry
/// no per-function citation line), so the derived gate below cannot over-expect a
/// citation the codegen never emits.
fn reachable_functions(module: &Module) -> Vec<&Function> {
    let mut fns = Vec::new();
    for item in &module.items {
        match item {
            Item::Function(f) => fns.push(f),
            Item::Struct { methods, .. } => fns.extend(methods.iter()),
            Item::Const { .. } | Item::Enum { .. } => {}
        }
    }
    fns
}

/// PMAT-955 (Pillar-A contract-citation-integrity capstone): the DERIVED
/// citation-completeness gate.
///
/// The `every_emitted_citation_resolves_to_an_on_disk_contract` gate above pins
/// expected citations with a HAND-CURATED `EXPECTED` list. That catches a witness
/// fixture dropping a known citation, but it is drift-prone in the OTHER
/// direction: when a NEW type family is wired into `Function::applicable_contracts()`
/// (the str/list/dict/float/set/class/tuple/Optional/bool arc), nothing forces a
/// matching `EXPECTED` entry, so a newly-applicable-but-uncited construct can ship
/// silently — the exact capability-vs-contract drift (audit-design.md §6) PMAT-955
/// exists to close.
///
/// This gate removes the hand list from the loop entirely. For each `.py` fixture
/// it parses the source through the REAL `PythonFrontend`, asks each reachable
/// `Function` for its `applicable_contracts()` (the SAME call codegen cites from),
/// unions them into the fixture's EXPECTED set, transpiles the fixture, and FAILS
/// if any derived id is absent from the emitted `// xpile-contract:` lines. The
/// expected set is COMPUTED from the meta-HIR, so it can never drift behind a new
/// `applicable_contracts()` arm: wire a family in and forget to emit its citation,
/// and this gate goes RED for every fixture exercising it.
#[test]
fn every_applicable_contract_is_actually_cited() {
    let bin = env!("CARGO_BIN_EXE_xpile");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let mut checked = 0usize;
    // Fixtures whose derived expected-set is non-empty (so we know the gate has
    // teeth — at least one fixture drives a real applicable contract).
    let mut with_expectations = 0usize;
    let mut missing: Vec<String> = Vec::new();

    for entry in fs::read_dir(&fixtures).expect("tests/fixtures dir readable") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("py") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        // Derive the expected citation set from the live frontend. A fixture the
        // Python frontend cannot lower (deliberate-reject corpus, multi-construct
        // probes) is skipped — the binary would not emit Rust for it either, so it
        // is out of scope for an EMITTED-citation gate.
        let Ok(module) = PythonFrontend.parse_and_lower(Path::new(&path), &src) else {
            continue;
        };
        let mut expected: HashSet<&'static str> = HashSet::new();
        for f in reachable_functions(&module) {
            for id in f.applicable_contracts() {
                expected.insert(id);
            }
        }
        if expected.is_empty() {
            continue;
        }

        // Transpile through the binary and collect the EMITTED citations.
        let out = Command::new(bin)
            .args(["transpile", path.to_str().unwrap()])
            .output()
            .expect("xpile binary runs");
        if !out.status.success() {
            // Frontend lowered but the default (Rust) backend declined (a capability
            // gap, not an integrity gap). Skip — no emitted code to cite against.
            continue;
        }
        let rust = String::from_utf8_lossy(&out.stdout);
        let emitted: HashSet<String> = rust
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("// xpile-contract:"))
            .map(|r| r.trim().to_string())
            .collect();

        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        checked += 1;
        with_expectations += 1;
        for id in &expected {
            if !emitted.contains(*id) {
                missing.push(format!(
                    "{fname}: `applicable_contracts()` derives `{id}` from the parsed \
                     meta-HIR, but the emitted Rust carries NO `// xpile-contract: {id}` \
                     line (emitted = {emitted:?})"
                ));
            }
        }
    }

    assert!(
        with_expectations > 20,
        "PMAT-955: expected a broad corpus of contract-bearing fixtures to drive the \
         derived gate, only {with_expectations} had a non-empty applicable-contract set \
         (parsing or codegen regressed?)"
    );
    assert!(
        missing.is_empty(),
        "PMAT-955: a construct with an applicable contract shipped UNCITED — \
         `Function::applicable_contracts()` and the emitted `// xpile-contract:` lines \
         diverged. This is the capability-vs-contract drift the citation-integrity gate \
         closes; either emit the citation in xpile-rust-codegen or (if the contract no \
         longer applies) drop it from applicable_contracts(). Offenders ({} across \
         {checked} fixtures):\n{}",
        missing.len(),
        missing.join("\n")
    );
}
