//! Trait-contract Runtime stratum properties (PMAT-269).
//!
//! Upgrades the trait contracts (`C-XPILE-BACKEND-TRAIT`,
//! `C-XPILE-FRONTEND-TRAIT`, `C-XPILE-CONTRACT-BACKEND-TRAIT`,
//! `C-XPILE-CONTRACT-FRONTEND-TRAIT`) from "minimum-viable single
//! Runtime witness" (audit-design.md §4) to **property-specific
//! Runtime invariants** verified against the LIVE
//! `xpile_core::default_session()`.
//!
//! Where the existing `trait_determinism.rs` tests are inputs-fixed
//! (one fixture, byte-identical compare across two runs), this file
//! exercises *trait-level invariants* across every registered impl:
//!
//! * **`target_ownership`** — no two registered backends declare the
//!   same `Target` variant; every requested target reaches exactly one
//!   `lower()` call. Property-tested by enumerating every backend's
//!   `targets()` slice and asserting the multiset has all-unique
//!   entries.
//! * **`name_uniqueness`** — no two backends share the same
//!   `name()`. Distinct from `target_ownership` because a backend
//!   could theoretically advertise zero targets and still collide on
//!   name.
//! * **`frontend_extension_disjointness`** — every frontend's
//!   `extensions()` set is disjoint from every other's (Frontend
//!   counterpart to `target_ownership`).
//! * **`backend_lower_is_deterministic`** — for every registered
//!   backend, two `lower()` calls on the same `(module, config)`
//!   produce byte-identical `Artifact.primary`. This is the runtime
//!   counterpart to the Kani harness `lower_idempotency` (PMAT-065).
//!
//! ## Why this is the SECOND contract to escape "demo fixture" status
//!
//! Per `audit-design.md` §4: "10 of those 12 contracts reach QUORUM
//! with the minimum-viable single Runtime witness — a demo fixture in
//! `crates/xpile/tests/fixtures/` rather than a property-specific
//! differential-execution comparison." `C-PY-INT-ARITH` escaped that
//! status via PMAT-267..268 (9241 oracle votes). The trait contracts
//! escape via this file, which property-tests their invariants over
//! the live session rather than fixing inputs.

use std::collections::HashSet;
use std::sync::Arc;
use xpile_backend::{Backend, BackendConfig, HwProfile, Profile, Target};
use xpile_core::default_session;
use xpile_meta_hir::{Module, SourceLang};

fn empty_module(lang: SourceLang) -> Module {
    Module {
        name: "trait_runtime_test".into(),
        source_lang: lang,
        items: Vec::new(),
        ffi_boundaries: Vec::new(),
    }
}

/// Build a minimal `BackendConfig` valid for the given target. Targets
/// that require an `HwProfile` (PTX, WGSL) get one; others get `None`.
fn config_for(target: Target) -> BackendConfig {
    let hardware = match target {
        Target::Ptx => Some(HwProfile::Ptx {
            compute_capability: "sm_80".to_string(),
        }),
        Target::Wgsl => Some(HwProfile::Wgsl {
            features: Vec::new(),
        }),
        _ => None,
    };
    BackendConfig {
        target,
        profile: Profile::RustOut,
        hardware,
    }
}

/// PMAT-269 — `C-XPILE-BACKEND-TRAIT` :: `target_ownership` Runtime
/// invariant.
///
/// Walks `default_session().backends`, builds the multiset of declared
/// `Target`s, and asserts every entry appears exactly once. Catches a
/// regression where two backends accidentally claim the same target
/// (e.g., adding a SPIRV backend that overlaps with WGSL).
#[test]
fn backend_target_ownership_is_unique_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<Target> = HashSet::new();
    for backend in &session.backends {
        for target in backend.targets() {
            assert!(
                seen.insert(*target),
                "target {:?} is owned by more than one backend (collision on `{}`)",
                target,
                backend.name(),
            );
        }
    }
}

/// PMAT-269 — `C-XPILE-BACKEND-TRAIT` :: `name_uniqueness` Runtime
/// invariant.
///
/// `name()` is the dispatch key the agent loop uses to talk about a
/// backend; two backends with the same name would make audit logs
/// ambiguous.
#[test]
fn backend_names_are_unique_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<&'static str> = HashSet::new();
    for backend in &session.backends {
        assert!(
            seen.insert(backend.name()),
            "backend name `{}` is registered more than once",
            backend.name()
        );
    }
}

/// PMAT-269 — `C-XPILE-FRONTEND-TRAIT` :: `extension_disjointness`
/// Runtime invariant.
///
/// Frontend counterpart to `target_ownership`: every extension in the
/// union `frontends.flat_map(|f| f.extensions())` must appear in
/// exactly one frontend's set.
#[test]
fn frontend_extensions_are_disjoint_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<&str> = HashSet::new();
    for frontend in &session.frontends {
        for ext in frontend.extensions() {
            assert!(
                seen.insert(ext),
                "extension `.{}` is claimed by more than one frontend (collision on `{}`)",
                ext,
                frontend.name(),
            );
        }
    }
}

/// PMAT-269 — `C-XPILE-BACKEND-TRAIT` :: `lower_idempotency` Runtime
/// invariant.
///
/// For every registered backend and every target it owns, run
/// `lower()` twice on a minimal module + appropriate config and assert
/// byte-identical `Artifact.primary`. The Kani harness
/// `lower_idempotency` (PMAT-065) proves this symbolically over a
/// 4-byte input; this test exercises it against EVERY live impl on a
/// real (if trivial) module.
///
/// Backends that legitimately reject an empty module (returning
/// `BackendError`) are required to *return the same error twice* —
/// determinism applies to both success and failure paths.
#[test]
fn every_backend_lower_is_deterministic_on_minimal_module() {
    let session = default_session();
    for backend in &session.backends {
        for target in backend.targets() {
            let module = empty_module(SourceLang::Rust);
            let config = config_for(*target);
            let first = backend.lower(&module, &config);
            let second = backend.lower(&module, &config);
            match (&first, &second) {
                (Ok(a), Ok(b)) => assert_eq!(
                    a.primary, b.primary,
                    "backend `{}` target {:?} non-deterministic: outputs differ between calls",
                    backend.name(),
                    target,
                ),
                (Err(e1), Err(e2)) => assert_eq!(
                    format!("{e1}"),
                    format!("{e2}"),
                    "backend `{}` target {:?} non-deterministic: errors differ between calls",
                    backend.name(),
                    target,
                ),
                _ => panic!(
                    "backend `{}` target {:?} non-deterministic: one call succeeded, the other failed; first={:?} second={:?}",
                    backend.name(),
                    target,
                    first,
                    second,
                ),
            }
        }
    }
}

/// PMAT-269 — `C-XPILE-BACKEND-TRAIT` :: `targets_slice_is_stable`
/// Runtime invariant.
///
/// `targets()` is documented as returning a `&[Target]` — the slice
/// itself MUST be stable across calls (the same pointer-equality
/// semantics aren't required, but the contents must be identical).
/// Catches a regression where a backend builds its target list
/// lazily and returns different orderings on subsequent calls.
#[test]
fn every_backend_targets_slice_is_stable_across_calls() {
    let session = default_session();
    for backend in &session.backends {
        let first: Vec<Target> = backend.targets().to_vec();
        let second: Vec<Target> = backend.targets().to_vec();
        assert_eq!(
            first,
            second,
            "backend `{}` returned different `targets()` slices on subsequent calls",
            backend.name()
        );
    }
}

/// PMAT-269 — guards against the trivially-broken state where the
/// session has zero backends, which would silently pass every other
/// test in this file via vacuous truth.
#[test]
fn default_session_registers_at_least_one_backend() {
    let session = default_session();
    assert!(
        !session.backends.is_empty(),
        "default_session() must register at least one backend — \
         otherwise every other trait_runtime_properties test passes vacuously"
    );
    assert!(
        !session.frontends.is_empty(),
        "default_session() must register at least one frontend"
    );
}

// ─── PMAT-270: Contract-lane trait runtime invariants ───────────────
//
// Mirror of the code-lane invariants above for the proof-lane traits
// (`ContractFrontend`, `ContractBackend`). Closes the audit-design.md
// §4 "demo fixture" caveat for `C-XPILE-CONTRACT-BACKEND-TRAIT` and
// `C-XPILE-CONTRACT-FRONTEND-TRAIT` — the THIRD and FOURTH contracts
// to escape that status after C-PY-INT-ARITH and the code-lane traits
// (PMAT-267..269).

use xpile_contract_backend::ContractRenderConfig;
use xpile_contracts::{
    Contract, ContractFormat, ContractId, XpileContractLane, XpileContractLayer,
};

fn dummy_contract(id: &str) -> Contract {
    Contract {
        id: ContractId::new(id),
        layer: XpileContractLayer::Architectural,
        lane: XpileContractLane::Proof,
        depends_on: Vec::new(),
        references: Vec::new(),
    }
}

fn render_config_for(format: ContractFormat) -> ContractRenderConfig {
    ContractRenderConfig {
        format,
        embed_citation: true,
        include_falsification: false,
        // ContractBackend impls that don't care about Lean version
        // ignore this; the Lean backend accepts Some((4, _)) or None.
        lean_version: Some((4, 0)),
    }
}

/// PMAT-270 — `C-XPILE-CONTRACT-BACKEND-TRAIT` :: `format_ownership`
/// Runtime invariant.
///
/// Proof-lane counterpart to `backend_target_ownership` (PMAT-269).
/// Every `ContractFormat` declared by a `ContractBackend.formats()`
/// slice must appear in exactly one registered backend.
#[test]
fn contract_backend_format_ownership_is_unique_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<ContractFormat> = HashSet::new();
    for backend in &session.contract_backends {
        for format in backend.formats() {
            assert!(
                seen.insert(*format),
                "ContractFormat {:?} is owned by more than one contract backend (collision on `{}`)",
                format,
                backend.name(),
            );
        }
    }
}

/// PMAT-270 — `C-XPILE-CONTRACT-BACKEND-TRAIT` :: `name_uniqueness`.
#[test]
fn contract_backend_names_are_unique_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<&'static str> = HashSet::new();
    for backend in &session.contract_backends {
        assert!(
            seen.insert(backend.name()),
            "contract backend name `{}` is registered more than once",
            backend.name()
        );
    }
}

/// PMAT-270 — `C-XPILE-CONTRACT-FRONTEND-TRAIT` :: `format_ownership`.
#[test]
fn contract_frontend_format_ownership_is_unique_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<ContractFormat> = HashSet::new();
    for frontend in &session.contract_frontends {
        for format in frontend.formats() {
            assert!(
                seen.insert(*format),
                "ContractFormat {:?} is claimed by more than one contract frontend (collision on `{}`)",
                format,
                frontend.name(),
            );
        }
    }
}

/// PMAT-270 — `C-XPILE-CONTRACT-FRONTEND-TRAIT` :: `name_uniqueness`.
#[test]
fn contract_frontend_names_are_unique_across_registered_impls() {
    let session = default_session();
    let mut seen: HashSet<&'static str> = HashSet::new();
    for frontend in &session.contract_frontends {
        assert!(
            seen.insert(frontend.name()),
            "contract frontend name `{}` is registered more than once",
            frontend.name()
        );
    }
}

/// PMAT-270 — `C-XPILE-CONTRACT-BACKEND-TRAIT` :: `render_idempotency`
/// Runtime invariant.
///
/// Proof-lane counterpart to `every_backend_lower_is_deterministic`.
/// For every registered contract backend and every format it owns,
/// two `render()` calls on the same `(contract, config)` must produce
/// byte-identical `RenderedDoc.primary` and `RenderedDoc.citations`,
/// or identical errors. The fixture `contract_backend_trait_demo.yaml`
/// flagged this as XPILE-CONTRACT-BACKEND-TRAIT-RUNTIME-001 future
/// work — this test discharges it.
#[test]
fn every_contract_backend_render_is_deterministic_on_minimal_contract() {
    let session = default_session();
    for backend in &session.contract_backends {
        for format in backend.formats() {
            let contract = dummy_contract("C-CONTRACT-BACKEND-DETERMINISM");
            let config = render_config_for(*format);
            let first = backend.render(&contract, &config);
            let second = backend.render(&contract, &config);
            match (&first, &second) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(
                        a.primary,
                        b.primary,
                        "contract backend `{}` format {:?} non-deterministic primary",
                        backend.name(),
                        format,
                    );
                    assert_eq!(
                        a.citations,
                        b.citations,
                        "contract backend `{}` format {:?} non-deterministic citations",
                        backend.name(),
                        format,
                    );
                }
                (Err(e1), Err(e2)) => assert_eq!(
                    format!("{e1}"),
                    format!("{e2}"),
                    "contract backend `{}` format {:?} non-deterministic errors",
                    backend.name(),
                    format,
                ),
                _ => panic!(
                    "contract backend `{}` format {:?} non-deterministic: one call succeeded, the other failed; first={:?} second={:?}",
                    backend.name(),
                    format,
                    first,
                    second,
                ),
            }
        }
    }
}

/// PMAT-270 — `C-XPILE-CONTRACT-FRONTEND-TRAIT` :: `parse_idempotency`
/// Runtime invariant.
///
/// Proof-lane counterpart to `render_idempotency`. For every
/// registered contract frontend, `parse_to_equations(s)` must be
/// deterministic for any source `s`. We use a representative LaTeX
/// fixture (the same content as `contract_frontend_trait_demo.tex`
/// inlined for hermeticity). Discharges
/// XPILE-CONTRACT-FRONTEND-TRAIT-RUNTIME-001 future work flagged in
/// the demo-fixture comments.
#[test]
fn every_contract_frontend_parse_is_deterministic_on_minimal_source() {
    // Representative LaTeX with both inline and display math — same
    // shape as contract_frontend_trait_demo.tex. Inlined so the test
    // doesn't depend on the fixture's exact bytes.
    let source = r#"\documentclass{article}
\begin{document}
\section{Basic equations}
A simple math span: $E = mc^2$.
A display math span:
\[ \int_0^{\infty} e^{-x} \, dx = 1 \]
\end{document}
"#;

    let session = default_session();
    for frontend in &session.contract_frontends {
        let first = frontend.parse_to_equations(source);
        let second = frontend.parse_to_equations(source);
        match (&first, &second) {
            (Ok(a), Ok(b)) => assert_eq!(
                a,
                b,
                "contract frontend `{}` non-deterministic EquationsBlock",
                frontend.name(),
            ),
            (Err(e1), Err(e2)) => assert_eq!(
                format!("{e1}"),
                format!("{e2}"),
                "contract frontend `{}` non-deterministic error",
                frontend.name(),
            ),
            _ => panic!(
                "contract frontend `{}` non-deterministic: one call succeeded, the other failed",
                frontend.name(),
            ),
        }
    }
}

/// PMAT-270 — guards against zero registered contract backends /
/// frontends, which would silently pass every other test above via
/// vacuous truth.
#[test]
fn default_session_registers_at_least_one_contract_backend_and_frontend() {
    let session = default_session();
    assert!(
        !session.contract_backends.is_empty(),
        "default_session() must register at least one contract backend — \
         otherwise the contract-backend tests pass vacuously"
    );
    assert!(
        !session.contract_frontends.is_empty(),
        "default_session() must register at least one contract frontend — \
         otherwise the contract-frontend tests pass vacuously"
    );
}

// Silence unused-import warnings if a future refactor drops one of
// these helpers without touching the tests.
#[allow(dead_code)]
fn _force_use(_b: Arc<dyn Backend>) {}
