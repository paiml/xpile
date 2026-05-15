//! Ruchy frontend for xpile.
//!
//! [Ruchy](https://github.com/paiml/ruchy) is a modern language for
//! data science and scientific computing with a self-hosting compiler.
//!
//! TODO: depend on the `ruchy` crate from crates.io and reuse its
//! parser + AST. For the scaffold, this is a placeholder that
//! implements the [`Frontend`] trait with stubs.

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{Module, SourceLang};

pub struct RuchyFrontend;

impl Frontend for RuchyFrontend {
    fn name(&self) -> &'static str {
        "ruchy"
    }

    fn extensions(&self) -> &[&'static str] {
        &["ruchy"]
    }

    fn parse_and_lower(&self, path: &Path, _source: &str) -> Result<Module, FrontendError> {
        // TODO: reuse ruchy's own parser + AST → meta-HIR lowering.
        Ok(Module {
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            source_lang: SourceLang::Ruchy,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        })
    }
}
