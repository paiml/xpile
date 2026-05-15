//! C frontend for xpile.
//!
//! TODO: integrate with decy's existing parser (clang/tree-sitter
//! based) and HIR-lowering pipeline. For the scaffold, this is a
//! placeholder that implements the [`Frontend`] trait with stubs.

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{Module, SourceLang};

pub struct CFrontend;

impl Frontend for CFrontend {
    fn name(&self) -> &'static str {
        "c"
    }

    fn extensions(&self) -> &[&'static str] {
        &["c", "h"]
    }

    fn parse_and_lower(&self, path: &Path, _source: &str) -> Result<Module, FrontendError> {
        // TODO: clang/tree-sitter parse + HIR lowering. Returns an empty module for the scaffold.
        Ok(Module {
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            source_lang: SourceLang::C,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        })
    }
}
