//! LaTeX contract backend — scaffold stub.
//!
//! Renders [`xpile_contracts::Contract`] as publication-quality LaTeX.
//! Each theorem environment is preceded by a `\xpileContract{<id>}{<eq>}`
//! macro that expands to a `\label{xpile:<id>:<eq>}` — parseable by
//! `latexmk`, `biblatex`, and the standard LaTeX cross-reference
//! tooling (NOT regex over body text).
//!
//! See `docs/specifications/sub/contract-backend-trait.md` §"Citation bridge
//! (decision #4 — revised post-audit)". Vendors `xpile-contracts.sty`
//! as a sidecar.

use xpile_contract_backend::{
    ContractBackend, ContractBackendError, ContractRenderConfig, RenderedDoc,
};
use xpile_contracts::{Contract, ContractFormat};

pub struct LatexContractBackend;

impl ContractBackend for LatexContractBackend {
    fn name(&self) -> &'static str {
        "latex"
    }

    fn formats(&self) -> &[ContractFormat] {
        &[ContractFormat::LatexMath]
    }

    fn render(
        &self,
        contract: &Contract,
        _config: &ContractRenderConfig,
    ) -> Result<RenderedDoc, ContractBackendError> {
        let primary = format!(
            "% xpile-latex-contract-backend scaffold\n\
             % Requires xpile-contracts.sty\n\
             \n\
             \\xpileContract{{{id}}}{{_scaffold}}\n\
             \\begin{{theorem}}[\\texttt{{{id}}}]\n\
             Scaffold — replace with real rendering of contract equations.\n\
             \\end{{theorem}}\n",
            id = contract.id,
        );
        let citations = std::iter::once(contract.id.clone())
            .chain(contract.depends_on.iter().cloned())
            .collect();
        Ok(RenderedDoc {
            primary,
            sidecars: vec![(
                "xpile-contracts.sty".to_string(),
                b"% xpile-contracts.sty (vendored, scaffold)\n\\NeedsTeXFormat{LaTeX2e}\n\\ProvidesPackage{xpile-contracts}\n\\newcommand{\\xpileContract}[2]{\\label{xpile:#1:#2}}\n".to_vec(),
            )],
            citations,
        })
    }
}
