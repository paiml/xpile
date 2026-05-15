//! Lean 4 (executable) backend — scaffold stub.
//!
//! Emits the executable subset of Lean 4 from meta-HIR: `def`,
//! `partial def`, `inductive`, `structure`, `instance`. Theorem
//! statements are NOT emitted here — those go through the proof lane
//! (`xpile-lean-contract-backend`).
//!
//! Layer 2 contract: `contracts/xlate-lean-to-rust-v1.yaml` covers the
//! Lean→Rust direction; this crate covers (meta-HIR → Lean executable).

use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
use xpile_meta_hir::Module;

pub struct LeanBackend;

impl Backend for LeanBackend {
    fn name(&self) -> &'static str {
        "lean"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Lean]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        Ok(Artifact {
            primary: format!(
                "-- xpile-lean-codegen scaffold\n-- module: {}\n-- TODO: lower meta-HIR to Lean 4 def/inductive.\n",
                module.name,
            ),
            sidecars: Vec::new(),
            citations: Vec::new(),
        })
    }
}
