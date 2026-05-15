//! xpile contracts — provable-contracts integration.
//!
//! xpile delegates its entire contract framework to
//! [`provable-contracts`](https://github.com/paiml/aprender/tree/main/crates/aprender-contracts)
//! (the `aprender-contracts` crate, library name `provable_contracts`).
//! See `docs/specifications/xpile-contract-driven-design-v1.md` for the
//! design rationale.
//!
//! This crate re-exports the upstream framework and adds xpile-specific
//! helpers — primarily the `XpileContractLayer` enum that tags each
//! contract by taxonomy layer (language semantics / translation /
//! architectural / hybrid pipeline).

pub use provable_contracts::{
    audit, binding, book_gen, coverage, diff, error, generate, graph, infer, kani_gen, latex,
    lean_gen, lint, probar_gen, query, readme_gen, scaffold, schema,
};

use serde::{Deserialize, Serialize};

/// Taxonomy layer of an xpile contract.
///
/// See `docs/specifications/sub/contract-taxonomy.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XpileContractLayer {
    /// Layer 1: per-language operational semantics.
    LanguageSemantics,
    /// Layer 2: source-construct → target translation.
    Translation,
    /// Layer 3: xpile-internal architectural invariant.
    Architectural,
    /// Layer 4: end-to-end hybrid pipeline.
    HybridPipeline,
    /// Layer 5: compile-time / IR / hardware invariants.
    CompileTime,
}

/// Which lane a contract belongs to.
///
/// Lanes are orthogonal to layers — a contract has exactly one lane
/// and exactly one layer. See `docs/specifications/sub/contract-taxonomy.md`
/// §"Lanes vs. layers".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XpileContractLane {
    /// Code lane — meta-HIR, FFI manifest, emitted target code.
    Code,
    /// Proof lane — notation, theorems, mdBook.
    Proof,
}

/// Canonical identifier of an xpile contract.
///
/// Matches the regex `^C-[A-Z0-9-]+$`. Preserved VERBATIM across all
/// formats (Lean attributes, LaTeX labels, mdBook comments) — see
/// `docs/specifications/sub/contract-backend-trait.md` §"Citation bridge".
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContractId(pub String);

impl ContractId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContractId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Format of a proof-lane artifact (LaTeX, Lean theorem text, mdBook, ...).
///
/// See `docs/specifications/sub/contract-frontend-trait.md` and
/// `docs/specifications/sub/contract-backend-trait.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractFormat {
    /// LaTeX math mode + theorem-class environments.
    LatexMath,
    /// Lean 4 theorem text. Lean 3 is not supported.
    LeanTheorem,
    /// Markdown with embedded math (mdBook).
    MdBook,
    /// Coq — future.
    Coq,
    /// Agda — future.
    Agda,
    /// Isabelle/HOL — future.
    Isabelle,
}

/// Parsed xpile contract. Placeholder shape for scaffolding — a full
/// parsed contract carries equations, proof obligations, falsification
/// tests, kani harnesses, and citation metadata. Real parsing is
/// performed by `provable-contracts`; this struct is the in-memory
/// projection xpile uses across crate boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id: ContractId,
    pub layer: XpileContractLayer,
    pub lane: XpileContractLane,
    pub depends_on: Vec<ContractId>,
    pub references: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum XpileContractError {
    #[error("upstream schema error: {0}")]
    Schema(String),
    #[error("xpile-specific extension error: {0}")]
    Extension(String),
}
