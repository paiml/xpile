//! ContractFrontend trait — proof-lane parsing abstraction.
//!
//! Parses notation (LaTeX math + theorem environments, Lean 4 theorem
//! text, mdBook with embedded math) into contract equation YAML.
//! Sibling of [`xpile_frontend::Frontend`]; the proof lane is disjoint
//! from the code lane and does NOT produce meta-HIR.
//!
//! Architectural invariants codified in
//! `contracts/xpile-contract-frontend-trait-v1.yaml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use xpile_contracts::{ContractFormat, ContractId};

/// A single equation parsed from notation, in canonical YAML form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equation {
    pub formula: String,
    pub domain: String,
    pub invariants: Vec<String>,
    pub preconditions: Vec<String>,
}

/// A single proof obligation parsed from a theorem-class environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofObligation {
    pub ty: ObligationType,
    pub property: String,
    pub formal: String,
    pub applies_to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationType {
    Precondition,
    Postcondition,
    Invariant,
    Frame,
    Bound,
}

/// External reference (BibTeX entry, URL, file path).
pub type Reference = String;

/// Result of parsing a notation source into the proof-lane contract
/// substrate. `equations_only` invariant: NO meta-HIR is produced or
/// mutated by ContractFrontend implementations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquationsBlock {
    pub equations: BTreeMap<String, Equation>,
    pub proof_obligations: Vec<ProofObligation>,
    pub references: Vec<Reference>,
    /// Contract IDs textually present in the source via structured
    /// citation constructs (LaTeX `\cite{}`/`\xpileContract{}`, Lean
    /// `@[xpile_contract ...]`, mdBook `<!-- xpile-contract: ... -->`).
    /// Parsers MUST extract these via the host format's structured
    /// parser, not via regex over body text. See
    /// `docs/specifications/sub/contract-frontend-trait.md`.
    pub citations: Vec<ContractId>,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractFrontendError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("malformed citation construct: {0}")]
    MalformedCitation(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Proof-lane parsing trait. See `docs/specifications/sub/contract-frontend-trait.md`.
pub trait ContractFrontend: Send + Sync {
    /// Human-readable name, e.g. "latex", "lean-theorem", "mdbook".
    fn name(&self) -> &'static str;

    /// Formats this frontend handles. Each [`ContractFormat`] variant
    /// is owned by exactly one ContractFrontend (`format_ownership`).
    fn formats(&self) -> &[ContractFormat];

    /// Parse notation source to an EquationsBlock.
    ///
    /// Invariants: deterministic; produces no Module; preserves every
    /// citation textually present in the source.
    fn parse_to_equations(&self, source: &str) -> Result<EquationsBlock, ContractFrontendError>;
}
