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
        // PMAT-1133 (R6): try/except dispatch is now wired + cited (a body with
        // a Stmt::TryCatch cites C-PY-EXCEPT-ALLOWLIST, proved core-Lean 1120).
        "C-PY-EXCEPT-ALLOWLIST",
        // PMAT-1135 (R6): whole-file I/O is now wired + cited (a FileReadAll/
        // FileReadLines/FileWrite body cites C-PY-FILE-IO-ROUNDTRIP, proved 1124).
        "C-PY-FILE-IO-ROUNDTRIP",
        // PMAT-1137 (R6): context managers now wired + cited (a body invoking
        // __enter__/__exit__ cites C-PY-CONTEXT-MANAGER-EXIT, proved 1131).
        "C-PY-CONTEXT-MANAGER-EXIT",
        // PMAT-1139 (R6): eager generators now wired + cited (a __gen_result
        // accumulator body cites C-PY-GENERATOR-EAGER, proved 1122).
        "C-PY-GENERATOR-EAGER",
        // PMAT-1145/1146 (R6): the last two module-level constructs — const +
        // enum — now cite their translation contracts (Item::applicable_contracts).
        "C-CONST-TRANSLATION",
        "C-ENUM-TRANSLATION",
        // PMAT-956 (provable-model-as-code): a fitted linear-model predictor
        // (∑ cᵢ·xᵢ + b over float params) now cites the model-uniqueness
        // contract. The structural determinism holds by construction; the
        // OLS-uniqueness is machine-checked in the Mathlib lane (ols_unique).
        "C-OLS-MODEL-UNIQUENESS",
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
        // PMAT-956: a fitted linear-model predictor cites the model-uniqueness
        // contract (a `∑ cᵢ·xᵢ + b` body over float params, ≥1 literal weight).
        ("ols_model.py", &["C-OLS-MODEL-UNIQUENESS"]),
        (
            "class_to_struct_contract.py",
            &["C-XLATE-PY-CLASS-TO-STRUCT"],
        ),
        // PMAT-958 (definition-level closure): a DEFINITION-ONLY fixture — two
        // method-less `@dataclass`es and NO functions at all. Before this slice
        // it emitted ZERO citations (no `Function` ran over it); now the struct
        // definition cites its class contract directly. The canonical witness
        // that the definition-level citation path is live, not just struct
        // methods cementing the citation as a side effect.
        ("dataclass_def.py", &["C-XLATE-PY-CLASS-TO-STRUCT"]),
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
        // PMAT-1140 (R6-slice5, skeptic-pass #5): the four newest R6 loops
        // (PMAT-1133/1135/1137/1139) were closed with only a (b) CORPUS-WIDE
        // required-cited entry — the weaker guarantee. C-PY-EXCEPT-ALLOWLIST is
        // emitted by 36 fixtures, C-PY-FILE-IO-ROUNDTRIP by 5, and
        // C-PY-CONTEXT-MANAGER-EXIT by 2, so any ONE could sever its citation and
        // (b) would still be satisfied by another fixture — exactly the hole (c)
        // exists to close. Pin a canonical witness for each so severing THAT
        // fixture's citation FAILS. (C-PY-GENERATOR-EAGER has a single fixture
        // today, but pinning it future-proofs against a second generator fixture.)
        ("except_allowlist.py", &["C-PY-EXCEPT-ALLOWLIST"]),
        ("file_read.py", &["C-PY-FILE-IO-ROUNDTRIP"]),
        ("context_managers.py", &["C-PY-CONTEXT-MANAGER-EXIT"]),
        ("generators_eager.py", &["C-PY-GENERATOR-EAGER"]),
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

/// PMAT-958 (Pillar-A definition-level citation closure): the contract ids
/// derived from the *definition-level* `Item`s of a module — the analog of
/// the per-function `applicable_contracts()` union, for the citation a
/// definition emits on ITSELF (not via any function). This is EXACTLY what
/// `emit_item_contract_citations` runs over in the rust/ruchy codegen, so the
/// derived gate below cannot over-expect a citation the codegen never emits.
/// Today only `Item::Struct` returns a definition-level contract
/// (`C-XLATE-PY-CLASS-TO-STRUCT`); `Item::Const`/`Item::Enum` have no
/// governing translation contract on disk yet (a documented follow-up) and
/// return nothing, so the gate does NOT go red on them.
fn definition_level_contracts(module: &Module) -> Vec<&'static str> {
    let mut ids = Vec::new();
    for item in &module.items {
        ids.extend(item.applicable_contracts());
    }
    ids
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
///
/// PMAT-958 (definition-level closure): the expected set now ALSO unions
/// `Item::applicable_contracts()` over the module's definition-level items
/// (`Item::Struct` → `C-XLATE-PY-CLASS-TO-STRUCT`). This closes the gap PMAT-955
/// named: a definition-only fixture (`dataclass_def.py` — a method-less
/// `@dataclass` with NO functions at all) previously drove an EMPTY expected set
/// (no `Function` ran over it), so it could ship `pub struct {..}` uncited without
/// the gate noticing. Now the struct definition's own contract is derived from the
/// meta-HIR and required in the emitted citations — drop the
/// `emit_item_contract_citations` call in codegen and this gate goes RED for
/// `dataclass_def.py` and every other struct-bearing fixture.
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
        // PMAT-958: union the DEFINITION-level contracts (a method-less struct
        // cites C-XLATE-PY-CLASS-TO-STRUCT on the definition, not via a function).
        for id in definition_level_contracts(&module) {
            expected.insert(id);
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

// ─── PMAT-956: the `on-disk → cited` orphan gate ───────────────────────────
//
// The other gates enforce `applicable → emitted` and `emitted → on-disk`. NONE
// enforced the third direction: that every on-disk GOVERNING (`C-*`) contract is
// cited SOMEWHERE in emitted output. Its absence is exactly how `C-WASM-HEAP`
// (a Layer-5 compile contract) shipped cited by NOTHING — an L5 orphan an audit
// had to find by hand. This gate makes that a mechanical invariant across all
// five layers: a contract added without a citation FAILS here unless it is
// deliberately listed as uncited-by-design with a reason.

/// Governing contracts that are intentionally NOT cited in emitted output, each
/// with the reason. Two honest categories only:
///   * Layer 3 ARCHITECTURAL — govern the transpiler's own Frontend/Backend
///     traits, not any emitted construct, so emitted code neither does nor
///     should cite them (contract-taxonomy §"Layer 3").
///   * Layer 2 DRAFT / scaffold proof lanes — the contract exists (`status:
///     draft`) but the lane emits only a scaffold stub (or is a frontend LIFT,
///     not an output emit), so citing it would misrepresent scaffold as
///     production. Reserved until the lane goes production — at which point the
///     entry must be removed (the `it IS now cited` assertion below enforces
///     that).
const UNCITED_BY_DESIGN: &[(&str, &str)] = &[
    (
        "C-XPILE-FRONTEND-TRAIT",
        "L3 architectural — governs the Frontend trait, not emitted output",
    ),
    (
        "C-XPILE-BACKEND-TRAIT",
        "L3 architectural — governs the Backend trait, not emitted output",
    ),
    (
        "C-XPILE-CONTRACT-FRONTEND-TRAIT",
        "L3 architectural — governs the ContractFrontend trait",
    ),
    (
        "C-XPILE-CONTRACT-BACKEND-TRAIT",
        "L3 architectural — governs the ContractBackend trait",
    ),
    (
        "C-XLATE-RUST-FN-TO-LEAN-THM",
        "L2 draft — LeanContractBackend::render is a scaffold stub (`theorem _scaffold`)",
    ),
    (
        "C-XLATE-LEAN-TO-RUST",
        "L2 draft — no production Lean→Rust emit path yet",
    ),
    (
        "C-NOTATION-LATEX-MATH-TO-EQUATION",
        "L2 — latex-contract-frontend is a LIFT lane; not emitted as an output citation",
    ),
];

/// `true` when `id` appears in `src` in a CITATION-shaped position — a `"<id>"`
/// string literal (covers `ContractId::new("…")`, an `applicable_contracts`
/// push, and a `const … = "<id>"` an emitter then cites), or an in-text
/// `xpile-contract: <id>` / `xpile_contract "<id>"` comment/attribute. A bare
/// doc-comment mention (`/// … <id> …`, unquoted) deliberately does NOT count.
fn cited_shaped(src: &str, id: &str) -> bool {
    src.contains(&format!("\"{id}\""))
        || src.contains(&format!("xpile-contract: {id}"))
        || src.contains(&format!("xpile_contract \"{id}\""))
}

/// Concatenate every non-test `crates/**/src/**.rs` — the emit surface the
/// citation scanner searches. `tests/` dirs and in-`src` `tests.rs` modules are
/// excluded so test assertions that mention an id are not mistaken for a
/// citation.
fn workspace_emit_src() -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = String::new();
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !matches!(name, "tests" | "target") {
                    stack.push(p);
                }
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name != "tests.rs" {
                    if let Ok(t) = fs::read_to_string(&p) {
                        out.push_str(&t);
                        out.push('\n');
                    }
                }
            }
        }
    }
    out
}

#[test]
fn every_governing_contract_is_cited_or_uncited_by_design() {
    let on_disk: HashSet<String> = on_disk_contract_ids()
        .into_iter()
        .filter(|id| id.starts_with("C-"))
        .collect();
    assert!(
        on_disk.contains("C-WASM-HEAP") && on_disk.contains("C-PY-INT-ARITH"),
        "sanity: governing C-* id set should have loaded, got {} ids",
        on_disk.len()
    );
    let src = workspace_emit_src();
    let allow: HashMap<&str, &str> = UNCITED_BY_DESIGN.iter().copied().collect();

    // (1) No orphan: every governing contract is cited in emitted output OR
    //     explicitly allowlisted. This is the direction that would have caught
    //     C-WASM-HEAP before it shipped.
    let mut orphans: Vec<&String> = on_disk
        .iter()
        .filter(|id| !cited_shaped(&src, id) && !allow.contains_key(id.as_str()))
        .collect();
    orphans.sort();
    assert!(
        orphans.is_empty(),
        "PMAT-956 orphan gate: on-disk contract(s) cited by NOTHING and not \
         allowlisted. Either emit the citation (structural `ContractId::new(\"…\")` \
         or in-text `// xpile-contract: …`) where the governed construct is \
         emitted, or — if the lane is architectural/draft — add it to \
         UNCITED_BY_DESIGN with a reason. Offenders: {orphans:?}"
    );

    // (2) The allowlist stays honest: every entry is a real on-disk contract AND
    //     is still genuinely uncited. A draft lane that goes production (and
    //     starts citing) must be REMOVED from the allowlist here.
    for (id, _reason) in UNCITED_BY_DESIGN {
        assert!(
            on_disk.contains(*id),
            "UNCITED_BY_DESIGN lists `{id}`, not an on-disk C-* contract — stale entry"
        );
        assert!(
            !cited_shaped(&src, id),
            "UNCITED_BY_DESIGN lists `{id}` but it IS now cited in emitted output — \
             remove it from the allowlist (the lane went production)"
        );
    }
}
