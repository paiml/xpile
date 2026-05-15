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
