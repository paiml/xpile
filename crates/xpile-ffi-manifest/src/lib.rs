//! FFI boundary registry.
//!
//! In a hybrid transpile (e.g. Python + C extension, Python + CUDA
//! kernel), the agent must know exactly which symbols cross language
//! lines and what their Rust shim signatures should be. This crate is
//! the single source of truth for that mapping within a session.

use serde::{Deserialize, Serialize};
use xpile_meta_hir::SourceLang;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FfiManifest {
    pub entries: Vec<FfiEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiEntry {
    pub symbol: String,
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub source_signature: String,
    pub rust_shim_signature: String,
    pub shim_id: String,
}

impl FfiManifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, entry: FfiEntry) {
        self.entries.push(entry);
    }
}
