//! Unified §14.4 quorum reporter gate (PMAT-033).
//!
//! Asserts the live state we shipped over PMAT-017..032:
//!   * `C-PY-INT-ARITH` has all four strata represented (Semantic via
//!     7 Lean theorems, Symbolic via 1 Kani harness, Runtime via fixture
//!     witnesses, Extrinsic via roadmap mentions).
//!   * Its quorum status is `QUORUM`.
//!   * The reporter walks all 11 contracts.
//!
//! This is the integration counterpart to the unit tests in
//! `quorum_tests` inside the binary crate — those exercise the threshold
//! logic against synthetic inputs; this one exercises the *live state*
//! the workspace claims about itself.

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
fn c_py_int_arith_has_full_four_stratum_quorum() {
    let root = workspace_root();
    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            "--witness-dir",
            root.join("crates/xpile-wasm-codegen/tests")
                .to_str()
                .unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "xpile quorum failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Hand-parse: locate the C-PY-INT-ARITH entry's status field.
    let id_marker = "\"id\":\"C-PY-INT-ARITH\"";
    let idx = stdout
        .find(id_marker)
        .unwrap_or_else(|| panic!("expected C-PY-INT-ARITH in quorum JSON:\n{stdout}"));
    let tail = &stdout[idx..];
    // Extract each field by scanning forward from the id marker.
    let read_field = |name: &str| -> &str {
        let key = format!("\"{name}\":");
        let kidx = tail.find(&key).expect("missing field");
        let after = &tail[kidx + key.len()..];
        let end = after.find([',', '}']).expect("delimiter");
        after[..end].trim().trim_matches('"')
    };
    let semantic: u64 = read_field("semantic").parse().unwrap();
    let symbolic: u64 = read_field("symbolic").parse().unwrap();
    let runtime: u64 = read_field("runtime").parse().unwrap();
    let extrinsic: u64 = read_field("extrinsic").parse().unwrap();
    let status = read_field("status");

    assert!(semantic >= 1, "expected Semantic ≥1, got {semantic}");
    assert!(symbolic >= 1, "expected Symbolic ≥1, got {symbolic}");
    assert!(runtime >= 1, "expected Runtime ≥1, got {runtime}");
    assert!(extrinsic >= 1, "expected Extrinsic ≥1, got {extrinsic}");
    assert_eq!(
        status, "QUORUM",
        "C-PY-INT-ARITH should have full quorum at v0.1.0 \
         (Sem={semantic}, Sym={symbolic}, Run={runtime}, Ext={extrinsic})"
    );
}

// PMAT-043 + PMAT-044 milestone: `C-BASHRS-POSIX-IDEMPOTENCE` is the
// *second* contract to reach QUORUM status with ≥1 vote in ≥3 strata.
// Composition:
//   - Semantic   (PMAT-044): Lean theorem `subprocess_run_eq_shell_run`
//     in `contracts/lean/Bashrs.lean`.
//   - Runtime    (PMAT-043): `shell_diff_exec.rs` observes CPython vs
//     bashrs-emit byte-identity; `bashrs_diff_demo.py` references the
//     contract ID.
//   - Extrinsic  (PMAT-037..044): roadmap work-item mentions.
// Symbolic (Kani) is still 0 — a Kani harness for shell idempotence
// ships in XPILE-BASHRS-MERGER-*** later.
#[test]
fn c_bashrs_posix_idempotence_has_runtime_witness() {
    let root = workspace_root();
    let out = std::process::Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            "--witness-dir",
            root.join("crates/xpile-wasm-codegen/tests")
                .to_str()
                .unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    assert!(out.status.success(), "xpile quorum failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Hand-parse: locate C-BASHRS-POSIX-IDEMPOTENCE's status field.
    let id_marker = "\"id\":\"C-BASHRS-POSIX-IDEMPOTENCE\"";
    let idx = stdout
        .find(id_marker)
        .unwrap_or_else(|| panic!("expected C-BASHRS-POSIX-IDEMPOTENCE in quorum JSON:\n{stdout}"));
    let tail = &stdout[idx..];
    let read_field = |name: &str| -> &str {
        let key = format!("\"{name}\":");
        let kidx = tail.find(&key).expect("missing field");
        let after = &tail[kidx + key.len()..];
        let end = after.find([',', '}']).expect("delimiter");
        after[..end].trim().trim_matches('"')
    };
    let semantic: u64 = read_field("semantic").parse().unwrap();
    let runtime: u64 = read_field("runtime").parse().unwrap();
    let extrinsic: u64 = read_field("extrinsic").parse().unwrap();
    let status = read_field("status");
    assert!(
        semantic >= 1,
        "expected Semantic ≥1 for C-BASHRS-POSIX-IDEMPOTENCE \
         (Bashrs.lean theorem `subprocess_run_eq_shell_run` should be \
         referenced from the contract YAML); got semantic={semantic}"
    );
    assert!(
        runtime >= 1,
        "expected Runtime ≥1 for C-BASHRS-POSIX-IDEMPOTENCE \
         (bashrs_diff_demo.py fixture should reference the contract); \
         got runtime={runtime}"
    );
    assert!(
        extrinsic >= 1,
        "expected Extrinsic ≥1 (roadmap mentions); got extrinsic={extrinsic}"
    );
    assert_eq!(
        status, "QUORUM",
        "C-BASHRS-POSIX-IDEMPOTENCE should have QUORUM at v0.1.0 — \
         second contract to reach full §14.4 N-of-M coverage \
         (Sem={semantic}, Run={runtime}, Ext={extrinsic}); got status={status}"
    );
}

#[test]
fn quorum_reporter_walks_all_contract_yamls() {
    // Counts the discovered contracts and asserts the number matches
    // what's in contracts/. Catches the regression where the reporter
    // silently misses YAMLs (wrong extension filter, dir not walked, etc.).
    let root = workspace_root();
    let contracts_dir = root.join("contracts");
    let yaml_count = std::fs::read_dir(&contracts_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("yaml"))
        .count();
    assert!(
        yaml_count >= 11,
        "expected at least 11 contracts/*.yaml, found {yaml_count}"
    );

    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            contracts_dir.to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            "--witness-dir",
            root.join("crates/xpile-wasm-codegen/tests")
                .to_str()
                .unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Count `"id":"` occurrences in the JSON — one per row.
    let n_rows = stdout.matches("\"id\":\"").count();
    assert_eq!(
        n_rows, yaml_count,
        "quorum reporter saw {n_rows} contracts; filesystem has {yaml_count}"
    );
}

// ── PMAT-1367: the Runtime stratum counts executions, not mentions ──────────
//
// Before this slice `count_runtime_witnesses` was, in full, a flat `read_dir`
// over `tests/fixtures` with the body `if text.contains(id) { hits += 1 }`. A
// bare comment in a fixture scored a full Runtime vote, while the entire WASM
// witness corpus — which assembles emitted modules with `wat2wasm` and runs
// them under `wasm-interp`, in the REQUIRED `workspace-test` job, with
// `XPILE_REQUIRE_WASM_RUNTIME=1` turning a missing runtime into a hard panic —
// scored zero. `C-COMPILE-RUST-TO-WASM` and `C-WASM-HEAP` reported Run=0 and
// status PARTIAL while being the two most-executed contracts in the repo.
//
// The widen is deliberately narrow. Every one of the 11 PARTIAL contracts sat
// at exactly two strata, so ANY Runtime vote flips a row — which makes this the
// single easiest number in the project to inflate. The NEGATIVE assertions
// below are therefore not decoration: they are what makes the change
// falsifiable.
//
// Measured, not assumed: a naive "any `crates/*/tests/*.rs` naming the ID" rule
// over the live 193-file set flips TEN of the eleven PARTIAL rows, and EIGHT of
// those ten are unearned:
//
//   * six (`C-CONST-TRANSLATION`, `C-PY-EXCEPT-ALLOWLIST`,
//     `C-PY-FILE-IO-ROUNDTRIP`, `C-PY-CONTEXT-MANAGER-EXIT`,
//     `C-PY-GENERATOR-EAGER`, `C-FFI-SHELL-SUBPROCESS`) come from gate files —
//     `contract_citation_integrity.rs` hardcodes a roster of contract IDs and
//     `lean_pilot_roots.rs` names them in comments. Neither executes an emitted
//     artifact; every `Command::new` in them spawns the `xpile` binary itself,
//     so `Command::new` is not a proxy for execution. Note the recursion: THIS
//     file names all six in `MUST_STAY_UNWITNESSED`, so under the naive rule the
//     test pinning the negatives would itself become a vote source for them.
//   * two (`C-COMPILE-RUST-TO-WGSL`, `C-COMPILE-RUST-TO-SPIRV`) come from
//     `gpu_witness.rs`. Those ARE real `DiffExec` witnesses and they DO carry an
//     availability probe — but on every CI runner they take the
//     `NotRun { no-engine }` branch, so the vote would rest on evidence the
//     required `workspace-test` job has never once produced. Hence
//     `RUNTIME_PROBES` names only the WASM probe, and widening it later is a
//     one-line reviewable edit rather than a grep that quietly loosens.
//
// If a future widen starts scoring any of those files, the six assertions below
// go red before the headline number moves.

/// Contracts that MUST NOT gain a Runtime vote from this widen. Each is
/// PARTIAL at exactly two strata today, so any of them flipping is the
/// signature of a meta-test leak — a gate file naming the ID being mistaken
/// for a witness executing the contract.
const MUST_STAY_UNWITNESSED: &[&str] = &[
    "C-CONST-TRANSLATION",
    "C-PY-EXCEPT-ALLOWLIST",
    "C-PY-FILE-IO-ROUNDTRIP",
    "C-PY-CONTEXT-MANAGER-EXIT",
    "C-PY-GENERATOR-EAGER",
    "C-FFI-SHELL-SUBPROCESS",
];

/// Contracts the executing WASM corpus genuinely witnesses, with a FLOOR on the
/// vote count rather than `>= 1`. A floor is what makes corpus deletion visible
/// here: dropping the 110 / 92 witness files to a handful would still leave
/// `>= 1` green.
const WASM_WITNESSED: &[(&str, u64)] = &[("C-COMPILE-RUST-TO-WASM", 50), ("C-WASM-HEAP", 50)];

fn quorum_rows() -> serde_json::Value {
    let root = workspace_root();
    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--json",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            // TRAP: the default is CWD-relative and `cargo test` sets CWD to
            // crates/xpile, so a relative path here would score 0 and this gate
            // would pass green while measuring nothing. Always absolute.
            "--witness-dir",
            root.join("crates/xpile-wasm-codegen/tests")
                .to_str()
                .unwrap(),
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "xpile quorum failed:\n  stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("quorum --json emits valid JSON")
}

fn row<'a>(v: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    v["contracts"]
        .as_array()
        .expect("contracts array")
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no quorum row for {id}"))
}

#[test]
fn wasm_contracts_earn_a_runtime_vote_from_the_executing_witness_corpus() {
    let v = quorum_rows();
    for (id, floor) in WASM_WITNESSED {
        let r = row(&v, id);
        let runtime = r["runtime"].as_u64().expect("runtime is a number");
        assert!(
            runtime >= *floor,
            "{id}: Runtime={runtime} is below the floor {floor}. The WASM \
             witness corpus under crates/xpile-wasm-codegen/tests executes \
             emitted modules through wat2wasm + wasm-interp; a count this low \
             means either the corpus shrank or the counter stopped reading it \
             (check that --witness-dir is ABSOLUTE — a relative default scores \
             0 silently under `cargo test`)."
        );
        assert_eq!(
            r["status"], "QUORUM",
            "{id} should reach QUORUM once the executing corpus votes; row = {r}"
        );
    }
}

#[test]
fn naming_a_contract_id_in_a_gate_file_never_earns_a_runtime_vote() {
    let v = quorum_rows();
    for id in MUST_STAY_UNWITNESSED {
        let r = row(&v, id);
        assert_eq!(
            r["runtime"], 0,
            "{id} gained a Runtime vote it did not earn — this is the \
             meta-test leak PMAT-1367 exists to prevent. A contract is \
             witnessed at the Runtime stratum only when a file EXECUTES an \
             emitted artifact under a real runtime; a gate file that hardcodes \
             the ID in a roster, or names it in a comment, is not a witness. \
             row = {r}"
        );
        assert_eq!(
            r["status"], "PARTIAL",
            "{id} should still be PARTIAL (Semantic + Extrinsic only); row = {r}"
        );
    }
}

#[test]
fn runtime_widen_flips_exactly_the_two_wasm_rows_and_leaves_none_unverified() {
    let v = quorum_rows();
    let rows = v["contracts"].as_array().expect("contracts array");
    let count = |s: &str| rows.iter().filter(|c| c["status"] == s).count();
    let (quorum, partial, unverified) = (count("QUORUM"), count("PARTIAL"), count("UNVERIFIED"));
    assert_eq!(
        rows.len(),
        35,
        "expected 35 contracts; adding one is fine but re-derive the floor below"
    );
    // A FLOOR, not an equality: a contract legitimately earning a Lean theorem
    // or a Kani harness must never red this gate, or the gate becomes pressure
    // to avoid improving. 26 is the derived state at PMAT-1367 (24 before the
    // widen + the two WASM rows); 9 PARTIAL and 0 UNVERIFIED accompany it.
    assert!(
        quorum >= 26,
        "expected ≥26 QUORUM after the Runtime widen, got {quorum} \
         (PARTIAL {partial}, UNVERIFIED {unverified})"
    );
    assert_eq!(
        unverified, 0,
        "a contract with zero represented strata appeared; \
         QUORUM {quorum}, PARTIAL {partial}"
    );
}

#[test]
fn a_missing_witness_dir_is_announced_on_stderr_and_is_not_fatal() {
    // TRAP (2): silently returning 0 for a missing dir would let a typo'd or
    // CWD-relative --witness-dir pass green while measuring nothing. The notice
    // is printed ONCE, by `quorum` itself, naming the path — not once per
    // contract — and the command still exits 0 because `quorum` is a reporter.
    let root = workspace_root();
    let out = Command::new(xpile_bin())
        .args([
            "quorum",
            "--contracts-dir",
            root.join("contracts").to_str().unwrap(),
            "--fixtures-dir",
            root.join("crates/xpile/tests/fixtures").to_str().unwrap(),
            "--witness-dir",
            "crates/xpile-wasm-codegen/tests",
            "--roadmap",
            root.join("docs/roadmaps/roadmap.yaml").to_str().unwrap(),
        ])
        .current_dir(root.join("crates/xpile"))
        .output()
        .expect("run xpile quorum");
    assert!(
        out.status.success(),
        "a missing witness dir must not be fatal"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("crates/xpile-wasm-codegen/tests") && stderr.contains("0 Runtime votes"),
        "expected a one-line notice naming the missing dir; stderr was:\n{stderr}"
    );
    assert_eq!(
        stderr.matches("is not a directory").count(),
        1,
        "the notice must fire once, not once per contract; stderr was:\n{stderr}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("C-COMPILE-RUST-TO-WASM"),
        "the report must still be produced"
    );
}
