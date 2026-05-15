//! Top-level session orchestration.
//!
//! Wires together the language frontends (code lane), backends (code
//! lane), contract frontends + backends (proof lane), the agent loop,
//! the FFI manifest, and the oracle. A [`TranspileSession`] is the
//! lifetime boundary for a single transpile invocation.

use std::sync::Arc;
use xpile_agent::Session as AgentSession;
use xpile_backend::Backend;
use xpile_contract_backend::ContractBackend;
use xpile_contract_frontend::ContractFrontend;
use xpile_ffi_manifest::FfiManifest;
use xpile_frontend::Frontend;

pub struct TranspileSession {
    // ── code lane ───────────────────────────────────────────────
    pub frontends: Vec<Arc<dyn Frontend>>,
    pub backends: Vec<Arc<dyn Backend>>,
    // ── proof lane ──────────────────────────────────────────────
    pub contract_frontends: Vec<Arc<dyn ContractFrontend>>,
    pub contract_backends: Vec<Arc<dyn ContractBackend>>,
    // ── shared infrastructure ───────────────────────────────────
    pub ffi_manifest: FfiManifest,
    pub agent: Option<AgentSession>,
}

impl TranspileSession {
    pub fn new() -> Self {
        Self {
            frontends: Vec::new(),
            backends: Vec::new(),
            contract_frontends: Vec::new(),
            contract_backends: Vec::new(),
            ffi_manifest: FfiManifest::new(),
            agent: None,
        }
    }

    pub fn register_frontend(&mut self, frontend: Arc<dyn Frontend>) {
        self.frontends.push(frontend);
    }

    pub fn register_backend(&mut self, backend: Arc<dyn Backend>) {
        self.backends.push(backend);
    }

    pub fn register_contract_frontend(&mut self, cf: Arc<dyn ContractFrontend>) {
        self.contract_frontends.push(cf);
    }

    pub fn register_contract_backend(&mut self, cb: Arc<dyn ContractBackend>) {
        self.contract_backends.push(cb);
    }
}

impl Default for TranspileSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a session with every v0.1.0-scaffolded impl registered.
///
/// Concrete logic is still placeholder, but the dispatch tables are
/// wired so the binary can list what's available and the trait
/// abstractions are exercised end-to-end.
pub fn default_session() -> TranspileSession {
    let mut s = TranspileSession::new();

    // Code lane: frontends
    s.register_frontend(Arc::new(depyler_frontend::PythonFrontend));
    s.register_frontend(Arc::new(decy_frontend::CFrontend));
    s.register_frontend(Arc::new(ruchy_frontend::RuchyFrontend));

    // Code lane: backends
    s.register_backend(Arc::new(xpile_rust_codegen::RustBackend));
    s.register_backend(Arc::new(xpile_ruchy_codegen::RuchyBackend));
    s.register_backend(Arc::new(xpile_ptx_codegen::PtxBackend));
    s.register_backend(Arc::new(xpile_wgsl_codegen::WgslBackend));
    s.register_backend(Arc::new(xpile_lean_codegen::LeanBackend));

    // Proof lane
    s.register_contract_frontend(Arc::new(latex_contract_frontend::LatexContractFrontend));
    s.register_contract_backend(Arc::new(xpile_lean_contract_backend::LeanContractBackend));
    s.register_contract_backend(Arc::new(xpile_latex_contract_backend::LatexContractBackend));

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_backend::{BackendConfig, BackendError, HwProfile, Profile, Target};
    use xpile_contracts::ContractFormat;
    use xpile_meta_hir::{Module, SourceLang};

    fn empty_module() -> Module {
        Module {
            name: "test".into(),
            source_lang: SourceLang::Python,
            items: vec![],
            ffi_boundaries: vec![],
        }
    }

    #[test]
    fn default_session_registers_v0_1_0_frontends() {
        let s = default_session();
        let names: Vec<&str> = s.frontends.iter().map(|f| f.name()).collect();
        for expected in &["python", "c", "ruchy"] {
            assert!(names.contains(expected), "missing frontend: {}", expected);
        }
    }

    #[test]
    fn default_session_registers_v0_1_0_backends() {
        let s = default_session();
        let names: Vec<&str> = s.backends.iter().map(|b| b.name()).collect();
        for expected in &["rust", "ruchy", "ptx", "wgsl", "lean"] {
            assert!(names.contains(expected), "missing backend: {}", expected);
        }
    }

    #[test]
    fn default_session_registers_proof_lane_impls() {
        let s = default_session();
        assert!(s.contract_frontends.iter().any(|cf| cf.name() == "latex"));
        let cb_names: Vec<&str> = s.contract_backends.iter().map(|b| b.name()).collect();
        assert!(cb_names.contains(&"lean-theorem"));
        assert!(cb_names.contains(&"latex"));
    }

    #[test]
    fn ptx_backend_stub_returns_artifact_with_layer5_citation() {
        let s = default_session();
        let ptx = s
            .backends
            .iter()
            .find(|b| b.name() == "ptx")
            .expect("ptx backend registered");
        let cfg = BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Ptx {
                compute_capability: "sm_80".into(),
            }),
        };
        let artifact = ptx
            .lower(&empty_module(), &cfg)
            .expect("ptx lower stub returns Ok");
        assert!(artifact.primary.contains("xpile-ptx-codegen scaffold"));
        // compile_contract_citation invariant: structural citation chain
        // (Vec<ContractId>), not regex over primary. See
        // contracts/xpile-backend-trait-v1.yaml.
        assert!(
            artifact
                .citations
                .iter()
                .any(|c| c.as_str() == "C-COMPILE-RUST-TO-PTX-MMA"),
            "ptx artifact must cite a Layer-5 compile contract via Artifact.citations"
        );
    }

    #[test]
    fn ptx_backend_errors_without_hardware_profile() {
        let s = default_session();
        let ptx = s
            .backends
            .iter()
            .find(|b| b.name() == "ptx")
            .expect("ptx backend registered");
        let cfg = BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: None,
        };
        let err = ptx
            .lower(&empty_module(), &cfg)
            .expect_err("ptx lower without hardware must error");
        assert!(matches!(err, BackendError::MissingHardware(Target::Ptx)));
    }

    #[test]
    fn latex_contract_frontend_owns_latex_math_and_returns_empty_block_for_empty_input() {
        let s = default_session();
        let cf = s
            .contract_frontends
            .iter()
            .find(|cf| cf.name() == "latex")
            .expect("latex contract frontend registered");
        assert!(cf.formats().contains(&ContractFormat::LatexMath));
        let block = cf
            .parse_to_equations("")
            .expect("empty input returns empty block");
        assert!(block.equations.is_empty());
        assert!(block.proof_obligations.is_empty());
        assert!(block.citations.is_empty());
    }
}
