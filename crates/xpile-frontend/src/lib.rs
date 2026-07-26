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
    /// PMAT-1346 (XPILE-FRONTEND-SUBSTANCE-001): the frontend RECOGNISES the
    /// file but has no lowering for the language at all.
    ///
    /// Distinct from [`FrontendError::Parse`] (the user's source is
    /// malformed) and [`FrontendError::Lower`] (a well-formed construct is
    /// outside the supported subset): the input may be perfectly valid —
    /// xpile simply cannot read this language yet. Returning this instead of
    /// an empty `Ok(Module)` is what turns a missing parser into a LOUD
    /// refusal with a non-zero exit rather than a silent empty emission,
    /// which is the one silent-wrong-answer shape the transpile promise
    /// (`README.md`: "refuses at transpile time with a reason instead of
    /// emitting code that silently diverges") does not survive.
    #[error("unimplemented frontend: {0}")]
    Unimplemented(String),
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

/// PMAT-1034: target capabilities threaded into lowering, extending the
/// PMAT-1024 [`AliasSemantics`] hint with a runtime-abort capability.
///
/// `runtime_abort` is true when the target can express a runtime abort — a
/// Rust/Ruchy `panic!` or a WASM `unreachable` trap. The Python frontend
/// then emits the empty-iterable loop-var-leak guard (an `UnboundLocalError`
/// analogue: `for x in xs: …` then a post-loop read of `x` raises in CPython
/// when `xs` was empty). Lanes with no portable abort (PTX / WGSL / SPIR-V /
/// Lean / shell) keep `false`: emitting the guard there would refuse shapes
/// those lanes execute exactly on every non-empty input, which is the
/// over-refusal PMAT-1034 explicitly rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoweringProfile {
    pub alias_semantics: AliasSemantics,
    pub runtime_abort: bool,
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

    /// PMAT-1346 (XPILE-FRONTEND-SUBSTANCE-001): does this frontend actually
    /// LOWER its language, or is it registered for ROUTING ONLY — so a
    /// matching file reaches a specific refusal naming what is unimplemented,
    /// instead of the generic `no frontend handles .<ext>` message?
    ///
    /// Routing-only frontends must not be counted as source languages xpile
    /// reads (`xpile info`, `README.md`). `ruchy-frontend` is the only one
    /// today: xpile emits Ruchy but cannot read it.
    ///
    /// This is a DECLARATION, and a declaration on its own would be worth
    /// nothing — a hollow frontend could simply claim `true`. It is
    /// cross-checked against BEHAVIOUR by
    /// `crates/xpile/tests/claims_drift.rs`, which runs every registered
    /// frontend against a real program in its own language and fails if what
    /// the frontend did disagrees with what it declared here.
    fn lowers_input(&self) -> bool {
        true
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

    /// PMAT-1034: parse and lower for a target of known [`LoweringProfile`]
    /// (alias semantics + runtime-abort capability). The default delegates to
    /// [`Frontend::parse_and_lower_for`], ignoring the abort capability, so
    /// existing frontends are unaffected; a frontend that emits
    /// abort-carrying runtime guards (depyler-frontend's empty-iterable
    /// loop-var-leak guard) overrides this to honor the full profile.
    fn parse_and_lower_profiled(
        &self,
        path: &Path,
        source: &str,
        profile: LoweringProfile,
    ) -> Result<Module, FrontendError> {
        self.parse_and_lower_for(path, source, profile.alias_semantics)
    }
}
