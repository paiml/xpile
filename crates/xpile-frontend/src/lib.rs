//! Frontend trait.
//!
//! Every source language in xpile (Python, C, Ruchy, ...) provides
//! one type implementing [`Frontend`]. The trait is intentionally
//! narrow: parse a source file and lower it to canonical meta-HIR.
//! Everything else — agent loop, oracle, codegen, MCP — is shared.

use std::path::Path;
use xpile_meta_hir::Module;

#[derive(Debug, thiserror::Error)]
pub enum FrontendError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("lowering error: {0}")]
    Lower(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait Frontend: Send + Sync {
    /// Human-readable language name, e.g. "python", "c", "ruchy".
    fn name(&self) -> &'static str;

    /// File extensions handled by this frontend, without leading dot.
    fn extensions(&self) -> &[&'static str];

    /// True when this frontend should claim `path`. PMAT-038
    /// (XPILE-BASHRS-MERGER-001 follow-up): default impl preserves
    /// the pre-existing extension-only routing — everything that
    /// matched via `extensions()` keeps matching here. Frontends with
    /// extensionless-filename idioms (`bashrs-frontend` for
    /// `Makefile` / `Dockerfile`) override this method to extend the
    /// match. Centralising routing here means dispatch sites can call
    /// one method instead of duplicating the extension-lookup logic.
    fn matches_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext_str| self.extensions().contains(&ext_str))
            .unwrap_or(false)
    }

    /// Parse source and lower to meta-HIR.
    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError>;
}
