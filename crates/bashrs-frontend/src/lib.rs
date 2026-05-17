//! POSIX shell frontend for xpile (sh / bash / zsh) — Layer A scaffold.
//!
//! This is the v0.1.0 scaffold for the bashrs merger (see
//! `docs/specifications/sub/bashrs-merger.md`). It implements the
//! [`Frontend`] trait so the dispatch table and `xpile info` recognize
//! the shell domain, but the real parser / lowering pipeline is
//! deferred to v0.2.0 (the `weeks 1-6 extract` phase of Layer A).
//!
//! At v0.1.0 `parse_and_lower` returns a structurally empty `Module`
//! tagged `SourceLang::Shell`. This is enough to:
//!   * Register the frontend with `xpile-core::default_session`.
//!   * Route `.sh` / `.bash` / `.zsh` / `.mk` files through the
//!     standard dispatch surface so a future change can plug in the
//!     real lowering without touching the bin or the session.
//!   * Make `xpile transpile foo.sh --target shell` work end-to-end
//!     (the bashrs-backend at v0.1.0 emits a placeholder POSIX
//!     comment carrying the contract citation).
//!
//! Special-file matching (`Makefile`, `Dockerfile`) is deferred —
//! the existing dispatch keys off file extension only. Adding a
//! `matches_path(...) -> bool` method to the `Frontend` trait is on
//! the v0.2.0 bashrs-source-folding ticket.

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::{Module, SourceLang};

pub struct BashrsFrontend;

impl Frontend for BashrsFrontend {
    fn name(&self) -> &'static str {
        "bashrs"
    }

    /// Shell-dialect extensions. Special-named files (Makefile,
    /// Dockerfile) come at v0.2.0 with a richer matcher.
    fn extensions(&self) -> &[&'static str] {
        &["sh", "bash", "zsh", "mk"]
    }

    fn parse_and_lower(&self, path: &Path, _source: &str) -> Result<Module, FrontendError> {
        // v0.1.0 scaffold: structurally empty module tagged as Shell.
        // The bashrs source folding (Layer A weeks 1-6) replaces this
        // body with the real parser → meta-HIR lowering pipeline.
        Ok(Module {
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            source_lang: SourceLang::Shell,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn name_is_bashrs() {
        assert_eq!(BashrsFrontend.name(), "bashrs");
    }

    #[test]
    fn extensions_cover_posix_shell_dialects() {
        let exts = BashrsFrontend.extensions();
        for needle in &["sh", "bash", "zsh", "mk"] {
            assert!(
                exts.contains(needle),
                "BashrsFrontend should recognise `.{needle}`; got {exts:?}"
            );
        }
    }

    #[test]
    fn parse_and_lower_returns_empty_shell_module() {
        // Scaffold contract: any input lowers to an empty Module
        // tagged `SourceLang::Shell`. The `name` mirrors the file
        // stem so downstream tooling can attribute the module.
        let module = BashrsFrontend
            .parse_and_lower(&PathBuf::from("/tmp/example.sh"), "echo hi\n")
            .expect("scaffold lower");
        assert_eq!(module.name, "example");
        assert_eq!(module.source_lang, SourceLang::Shell);
        assert!(module.items.is_empty(), "v0.1.0 scaffold emits no items");
    }
}
