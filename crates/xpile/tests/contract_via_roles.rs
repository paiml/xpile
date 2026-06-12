//! PMAT-484 (§30 Track 4): in-tree validator for the structured
//! `compile_targets.via_roles` records in the PTX contract.
//!
//! This is the authoritative check (the cross-repo `pv`-engine
//! enforcement of these roles is the residual PMAT-A5). It asserts the
//! §29 quorum invariants: at least one — and at most one — `role:
//! general`, no specialist-only route, and a `DiffExec` `quorum_policy`
//! carrying a `tolerance`.

use std::path::PathBuf;

fn ptx_contract() -> serde_yaml::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/compile-rust-to-ptx-mma-v1.yaml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_yaml::from_str(&text).expect("PTX contract is valid YAML")
}

/// The first `compile_targets` entry's `via_roles` sequence.
fn via_roles(doc: &serde_yaml::Value) -> Vec<serde_yaml::Value> {
    doc["compile_targets"][0]["via_roles"]
        .as_sequence()
        .expect("compile_targets[0].via_roles must be a sequence")
        .clone()
}

fn role_of(entry: &serde_yaml::Value) -> &str {
    entry["role"].as_str().unwrap_or("")
}

#[test]
fn ptx_contract_has_exactly_one_general_emitter() {
    let doc = ptx_contract();
    let roles = via_roles(&doc);
    let generals = roles.iter().filter(|e| role_of(e) == "general").count();
    assert_eq!(
        generals, 1,
        "the PTX contract must declare exactly one role: general emitter (the mandatory fallback)"
    );
}

#[test]
fn ptx_contract_has_no_specialist_only_route() {
    // A specialist is allowed, but only alongside a general (the quorum
    // needs a fallback for unmatched shapes). With exactly one general
    // guaranteed above, this just checks specialists don't exceed sanity.
    let doc = ptx_contract();
    let roles = via_roles(&doc);
    let specialists = roles.iter().filter(|e| role_of(e) == "specialist").count();
    let generals = roles.iter().filter(|e| role_of(e) == "general").count();
    assert!(
        generals >= 1 || specialists == 0,
        "a specialist route requires a general fallback (no specialist-only quorum)"
    );
}

#[test]
fn ptx_contract_diffexec_quorum_policy_carries_tolerance() {
    let doc = ptx_contract();
    let policy = &doc["compile_targets"][0]["quorum_policy"];
    assert_eq!(
        policy["kind"].as_str(),
        Some("DiffExec"),
        "PTX quorum_policy.kind must be DiffExec"
    );
    assert!(
        policy["tolerance"].as_f64().is_some(),
        "DiffExec quorum_policy must carry a numeric tolerance"
    );
}

#[test]
fn ptx_contract_general_emitter_names_its_crate() {
    let doc = ptx_contract();
    let roles = via_roles(&doc);
    let general = roles
        .iter()
        .find(|e| role_of(e) == "general")
        .expect("a general emitter exists");
    assert!(
        general["crate"].as_str().is_some(),
        "the general emitter must name its backend crate for citation provenance"
    );
}
