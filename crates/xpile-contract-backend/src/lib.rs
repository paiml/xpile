//! ContractBackend trait — proof-lane rendering abstraction.
//!
//! Renders parsed [`xpile_contracts::Contract`] into LaTeX (math +
//! theorem environments), Lean 4 theorem text (with `@[xpile_contract]`
//! attribute carrying the verbatim contract ID), or mdBook.
//!
//! Architectural invariants codified in
//! `contracts/xpile-contract-backend-trait-v1.yaml`. The citation bridge
//! uses **format-native structured constructs** parseable by the host
//! elaborator/parser, NOT regex over body text. See
//! `docs/specifications/sub/contract-backend-trait.md` §"Citation bridge".

use serde::{Deserialize, Serialize};
use xpile_contracts::{Contract, ContractFormat, ContractId};

/// Configuration passed to [`ContractBackend::render`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractRenderConfig {
    pub format: ContractFormat,
    /// When true, the citation ID is emitted textually (in addition to
    /// `RenderedDoc.citations`) via the format's structured construct:
    /// `\xpileContract{}{}` (LaTeX), `@[xpile_contract ...]` (Lean),
    /// `<!-- xpile-contract: ... -->` (mdBook).
    pub embed_citation: bool,
    /// When true, every falsification_test surfaces as a `\begin{remark}`
    /// (LaTeX), `-- FALSIFY-<ID>` block (Lean), or `> **Falsification:**`
    /// block (mdBook). When false, falsification tests are absent.
    pub include_falsification: bool,
    /// (major, minor) Lean version. Only Lean 4 is supported
    /// (`lean_version: Some((4, _))`); rendering with Lean 3 returns
    /// `Err(ContractBackendError::UnsupportedLeanVersion)`.
    pub lean_version: Option<(u32, u32)>,
}

/// Rendered output of a single contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedDoc {
    /// Rendered `.tex` / `.lean` / `.md` text.
    pub primary: String,
    /// Optional sidecars (figures, BibTeX entries, oleantheorem files).
    pub sidecars: Vec<(String, Vec<u8>)>,
    /// Structural citation chain. Every contract ID in
    /// `Contract.depends_on` and `Contract.references` MUST be present.
    pub citations: Vec<ContractId>,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractBackendError {
    #[error("unsupported format for this backend: {0:?}")]
    UnsupportedFormat(ContractFormat),
    #[error("unsupported Lean version: {0:?} (only Lean 4 is supported)")]
    UnsupportedLeanVersion(Option<(u32, u32)>),
    #[error("rendering error: {0}")]
    Render(String),
    #[error("citation bridge violation: {0}")]
    CitationBridge(String),
}

/// Proof-lane rendering trait. See `docs/specifications/sub/contract-backend-trait.md`.
pub trait ContractBackend: Send + Sync {
    /// Human-readable name, e.g. "latex", "lean-theorem", "mdbook".
    fn name(&self) -> &'static str;

    /// Formats this backend can render. Each [`ContractFormat`] variant
    /// is owned by exactly one ContractBackend (`format_ownership`).
    fn formats(&self) -> &[ContractFormat];

    /// PMAT-1429: whether [`render`](Self::render) actually derives its
    /// `primary` from the contract it is handed, or is a SCAFFOLD that
    /// returns a fixed payload the contract cannot influence.
    ///
    /// This is the proof-lane twin of `Frontend::lowers_input()` (PMAT-1346)
    /// and exists for the same reason: `xpile info` prints the registry, and
    /// `README.md` / `book/src/reference/cli.md` point at that listing as the
    /// live capability surface. A registered backend that returns
    /// `theorem _scaffold : True := True.intro` for **every** contract must
    /// not be counted, silently, among the formats xpile RENDERS.
    ///
    /// There is deliberately no default: a new ContractBackend has to state
    /// which it is at the impl site, rather than inheriting "real" for free.
    ///
    /// `crates/xpile/tests/proof_lane_scaffold_witness.rs` MEASURES this
    /// rather than trusting it — it renders two contracts that differ in
    /// every field [`Contract`] carries and fails in BOTH directions: a
    /// backend whose output is contract-independent must report `false`, and
    /// one that reports `false` must actually be contract-independent.
    fn renders_contract_body(&self) -> bool;

    /// Render a contract under the given config.
    ///
    /// Invariants: deterministic per `(contract, config)`; every cited
    /// contract ID is parseable from `primary` via the host format's
    /// structured parser (Lean elaborator, LaTeX label machinery,
    /// mdBook preprocessor) — NOT regex over text.
    fn render(
        &self,
        contract: &Contract,
        config: &ContractRenderConfig,
    ) -> Result<RenderedDoc, ContractBackendError>;
}
