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

/// PMAT-1443: which claimed spellings a caller of
/// [`Frontend::spellings_by_disposition`] is asking about.
///
/// The two scopes exist because xpile has TWO different notions of "claimed",
/// and a surface that renders the wrong one lies in its own direction.
/// [`Frontend::matches_path`] is the DISPATCH claim (it accepts extensionless
/// `Makefile` / `Dockerfile`); an extension walk — what `xpile audit`'s
/// collector does — is strictly narrower.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellingScope {
    /// Only the `*.<ext>` globs: what an extension-scoped scanner can reach.
    Extensions,
    /// Additionally the extensionless filenames [`Frontend::matches_path`]
    /// claims: what a dispatch-failure message must report.
    All,
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

    /// PMAT-1433 (XPILE-FRONTEND-CLAIM-001): the path spellings this frontend
    /// CLAIMS via [`Frontend::matches_path`] but REFUSES for every input.
    ///
    /// [`Frontend::lowers_input`] is a WHOLE-FRONTEND boolean, and that is the
    /// granularity that let `.mk` be published as handled. `bashrs-frontend`
    /// lowers `.sh` / `.bash` / `.zsh` and so declares `lowers_input() ==
    /// true`, which is what `xpile info` and `book/src/reference/frontends.md`
    /// read — while PMAT-1420 made `*.mk`, `Makefile` and `Dockerfile` refuse
    /// unconditionally. A frontend can lower SOME of what it claims; there was
    /// no way to say so, so the reports said it lowered all of it.
    ///
    /// Entries are literal path spellings, each either a `*.<ext>` glob whose
    /// extension appears in [`Frontend::extensions`] or an exact extensionless
    /// filename that [`Frontend::matches_path`] claims. This is DECLARATION,
    /// worth nothing on its own; `crates/xpile/tests/frontend_claim_disposition_witness.rs`
    /// drives EVERY claimed spelling through [`Frontend::parse_and_lower`] and
    /// asserts set EQUALITY with what is declared here — too many entries (a
    /// spelling that in fact lowers) and too few (one that in fact refuses)
    /// both red. Implementing a disclosed gap therefore FORCES the disclosure
    /// to move rather than letting it go stale.
    ///
    /// REQUIRED, deliberately: a default of `&[]` would let the next frontend
    /// with a partial refusal inherit the exact silence this method exists to
    /// break.
    fn refused_claims(&self) -> &[&'static str];

    /// PMAT-1443 (XPILE-AUDITCLAIM-001): this frontend's claimed path
    /// spellings, split by disposition — `(lowers, refused)`.
    ///
    /// [`Frontend::extensions`] is a ROUTING set, not a capability set: a
    /// spelling is kept in it precisely so a matching file reaches a SPECIFIC
    /// refusal instead of the generic dispatch failure (see
    /// [`Frontend::refused_claims`]). So every user-facing rendering of the
    /// registry has to carry the disposition — and each surface that derived
    /// it independently got it wrong in its own way. The dispatch-failure
    /// message published the flat union until PMAT-1434; `xpile audit`'s
    /// no-source bail and `examples/06_inspect_session.rs` were still
    /// publishing it at PMAT-1443, the latter under the heading "read source
    /// → meta-HIR". This is the ONE derivation they all call.
    ///
    /// `scope` selects which spellings are in range for the caller.
    /// [`SpellingScope::Extensions`] yields only the `*.<ext>` globs — the
    /// set an EXTENSION-SCOPED scanner can reach, which is what `xpile
    /// audit` must report because its collector walks by extension and never
    /// sees `Makefile`. [`SpellingScope::All`] additionally yields the
    /// extensionless filenames [`Frontend::matches_path`] claims, which is
    /// what a DISPATCH-failure message must report. Rendering the wrong one
    /// trades an over-report for a different over-report: naming `Makefile`
    /// in the audit bail would advertise a spelling audit cannot collect at
    /// any extension.
    ///
    /// A pure function of `extensions()` + `refused_claims()`, both of which
    /// are already confronted with BEHAVIOUR at every claimed spelling by
    /// `crates/xpile/tests/frontend_claim_disposition_witness.rs`
    /// (XPILE-FRONTEND-CLAIM-001), so a caller reading this split reads a
    /// behaviour-checked fact and not a self-report. Registration order is
    /// preserved so the output is deterministic.
    fn spellings_by_disposition(&self, scope: SpellingScope) -> (Vec<String>, Vec<String>) {
        let declared = self.refused_claims();
        let mut lowers = Vec::new();
        let mut refused = Vec::new();
        for ext in self.extensions() {
            let claim = format!("*.{ext}");
            if declared.contains(&claim.as_str()) {
                refused.push(claim);
            } else {
                lowers.push(claim);
            }
        }
        if scope == SpellingScope::All {
            // The extensionless spellings. Every `*.<ext>` entry was already
            // placed by the loop above (XPILE-FRONTEND-CLAIM-001 asserts every
            // glob entry's extension is in `extensions()`), so taking it again
            // here would duplicate it.
            refused.extend(
                declared
                    .iter()
                    .filter(|c| !c.starts_with("*."))
                    .map(|c| (*c).to_string()),
            );
        }
        (lowers, refused)
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

/// PMAT-1443: the frontend roster, rendered for a human, with every claimed
/// spelling carrying its disposition.
///
/// This exists as a LIBRARY function rather than as a `println!` loop in each
/// consumer because the loop is exactly what drifted. `xpile info` grew the
/// disposition at PMAT-1428; `crates/xpile/examples/06_inspect_session.rs`,
/// which `book/src/quickstart.md` tells the reader to run to answer "what's
/// registered?", kept printing the raw [`Frontend::extensions`] union under
/// the heading "Frontends (read source → meta-HIR)" — so it published `ruchy`
/// and `mk`, which read nothing, as languages xpile reads. A shared renderer
/// makes that a property of ONE function that
/// `crates/xpile/tests/audit_claim_disposition_witness.rs` asserts on
/// directly, instead of a property of each caller's formatting.
///
/// Returns a multi-line block WITHOUT a trailing newline; the caller supplies
/// the surrounding layout.
pub fn render_frontend_roster(frontends: &[std::sync::Arc<dyn Frontend>]) -> String {
    let mut out = String::new();
    for (i, f) in frontends.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let (lowers, refused) = f.spellings_by_disposition(SpellingScope::All);
        // "(none)" rather than an empty list: a routing-only frontend
        // (`ruchy`) has NOTHING in the lowering half, and an empty `[]` there
        // reads as a rendering slip rather than as the finding.
        let lowers_txt = if lowers.is_empty() {
            "(none)".to_string()
        } else {
            lowers.join(", ")
        };
        out.push_str(&format!("    - {:8}  LOWERS: {lowers_txt}", f.name()));
        if !refused.is_empty() {
            out.push_str(&format!(
                "   ROUTED but REFUSED (no parser): {}",
                refused.join(", ")
            ));
        }
    }
    out
}
