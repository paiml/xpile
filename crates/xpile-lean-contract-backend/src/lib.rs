//! Lean 4 contract backend — scaffold stub.
//!
//! Renders [`xpile_contracts::Contract`] as Lean 4 theorem text. Each
//! emitted theorem carries an `@[xpile_contract "<id>", xpile_equation "<eq>"]`
//! attribute (parsed by Lean's elaborator — NOT regex) with the
//! contract ID preserved VERBATIM (no dash→underscore mangling).
//!
//! See `docs/specifications/sub/contract-backend-trait.md` §"Citation bridge
//! (decision #4 — revised post-audit)". Layer 2 contract:
//! `contracts/xlate-rust-fn-to-lean-thm-v1.yaml`.

use xpile_contract_backend::{
    ContractBackend, ContractBackendError, ContractRenderConfig, RenderedDoc,
};
use xpile_contracts::{Contract, ContractFormat};

pub struct LeanContractBackend;

impl ContractBackend for LeanContractBackend {
    fn name(&self) -> &'static str {
        "lean-theorem"
    }

    fn formats(&self) -> &[ContractFormat] {
        &[ContractFormat::LeanTheorem]
    }

    /// PMAT-1429: SCAFFOLD. `render` interpolates `contract.id` into a fixed
    /// `theorem _scaffold : True := True.intro` and ignores every other
    /// field. The only config knob it reads is `lean_version` (to reject
    /// Lean 3); `embed_citation` / `include_falsification` are ignored.
    fn renders_contract_body(&self) -> bool {
        false
    }

    fn render(
        &self,
        contract: &Contract,
        config: &ContractRenderConfig,
    ) -> Result<RenderedDoc, ContractBackendError> {
        // Lean 4 only — reject Lean 3 explicitly per design decision #1.
        match config.lean_version {
            Some((4, _)) | None => {}
            other => return Err(ContractBackendError::UnsupportedLeanVersion(other)),
        }
        // Stub rendering. Real impl emits theorems with @[xpile_contract] attrs.
        let primary = format!(
            "-- xpile-lean-contract-backend scaffold\n\
             import XpileContracts.Attr\n\
             \n\
             @[xpile_contract \"{id}\", xpile_equation \"_scaffold\"]\n\
             theorem _scaffold : True := True.intro\n",
            id = contract.id,
        );
        let citations = std::iter::once(contract.id.clone())
            .chain(contract.depends_on.iter().cloned())
            .collect();
        Ok(RenderedDoc {
            primary,
            sidecars: Vec::new(),
            citations,
        })
    }
}
