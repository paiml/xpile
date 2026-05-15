//! LaTeX contract frontend — scaffold stub.
//!
//! Parses LaTeX source into [`EquationsBlock`]. Handles BOTH math mode
//! (`$...$`, `\[...\]`, `equation`, `align`, `gather`) AND theorem-class
//! environments (`theorem`, `lemma`, `corollary`, `proposition`,
//! `definition`, `remark`, `proof`).
//!
//! Layer 2 contract: `contracts/notation-latex-math-to-equation-v1.yaml`.
//!
//! Citation parsing uses LaTeX's `\label`/`\xpileContract` machinery,
//! NOT regex over body text. See
//! `docs/specifications/sub/contract-frontend-trait.md` §"Citation preservation".

use xpile_contract_frontend::{ContractFrontend, ContractFrontendError, EquationsBlock};
use xpile_contracts::ContractFormat;

pub struct LatexContractFrontend;

impl ContractFrontend for LatexContractFrontend {
    fn name(&self) -> &'static str {
        "latex"
    }

    fn formats(&self) -> &[ContractFormat] {
        &[ContractFormat::LatexMath]
    }

    fn parse_to_equations(&self, _source: &str) -> Result<EquationsBlock, ContractFrontendError> {
        // TODO: parse LaTeX via pulldown-latex / lalrpop / pylatexenc-rs.
        // Extract math spans → equations.
        // Extract theorem-class envs → proof_obligations.
        // Extract \cite{}, \xpileContract{}{}, \label{xpile:...} → citations.
        Ok(EquationsBlock::default())
    }
}
