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

    /// Parse source and lower to meta-HIR.
    fn parse_and_lower(&self, path: &Path, source: &str) -> Result<Module, FrontendError>;
}
