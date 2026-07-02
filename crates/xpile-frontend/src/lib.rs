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

/// PMAT-1024: how the eventual TARGET binds names to objects, threaded into
/// lowering so the frontend's alias dispositions can be target-aware.
///
/// The Python frontend guards Python's reference semantics with a
/// clone/move/refuse disposition suite (PMAT-884/1008/1016C/1018/1019)
/// because Rust VALUE semantics cannot express object sharing. A
/// linear-memory target ([`crate::AliasSemantics::Reference`] — WASM) holds
/// every container/struct local as an i32 base-pointer into the heap, so a
/// binding copy IS Python's object sharing: the dispositions there are not
/// just unnecessary, they actively break valid programs (an inserted
/// `Expr::Clone` refuses at the WASM backend; a refusal blocks a shape the
/// target executes exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AliasSemantics {
    /// Rust-lane value semantics: bindings copy/move values, so Python
    /// object sharing must be cloned, moved, or refused. The default.
    #[default]
    Value,
    /// Linear-memory reference semantics (`Target::Wasm`): heap locals are
    /// base-pointers, a binding copy shares the object natively.
    Reference,
}

impl AliasSemantics {
    pub fn is_reference(self) -> bool {
        matches!(self, Self::Reference)
    }
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

    /// PMAT-1024: parse and lower FOR a target of known [`AliasSemantics`].
    /// The default ignores the hint and preserves the target-blind lowering,
    /// so existing frontends are unaffected; a frontend whose lowering
    /// carries value-semantics alias dispositions (depyler-frontend)
    /// overrides this to skip them for reference-semantics targets.
    fn parse_and_lower_for(
        &self,
        path: &Path,
        source: &str,
        semantics: AliasSemantics,
    ) -> Result<Module, FrontendError> {
        let _ = semantics;
        self.parse_and_lower(path, source)
    }
}
