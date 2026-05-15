//! Python frontend for xpile.
//!
//! TODO: integrate with rustpython-parser (as in depyler) and depyler's
//! existing HIR-lowering. For the scaffold, this is a placeholder that
//! implements the [`Frontend`] trait with stubs.

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{Module, SourceLang};

pub struct PythonFrontend;

impl Frontend for PythonFrontend {
    fn name(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &[&'static str] {
        &["py", "pyi"]
    }

    fn parse_and_lower(&self, path: &Path, _source: &str) -> Result<Module, FrontendError> {
        // TODO: rustpython-parser + HIR lowering. Returns an empty module for the scaffold.
        Ok(Module {
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            source_lang: SourceLang::Python,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        })
    }
}
