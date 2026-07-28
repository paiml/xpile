//! Ruchy frontend for xpile — **routing only; `.ruchy` INPUT refuses.**
//!
//! [Ruchy](https://github.com/paiml/ruchy) is a modern language for
//! data science and scientific computing with a self-hosting compiler.
//! xpile emits Ruchy (`--target ruchy`, `xpile-ruchy-codegen`) but does
//! **not** read it: there is no `.ruchy` parser and no `.ruchy` → meta-HIR
//! lowering.
//!
//! ## PMAT-1346 — why this refuses instead of returning an empty module
//!
//! Until PMAT-1346 this frontend returned
//! `Ok(Module { items: Vec::new(), .. })` for **any** `.ruchy` input, so
//! `xpile transpile foo.ruchy --target rust` printed a header comment and
//! **exited 0** — a silently empty transpile of a program with real content.
//! That is the exact shape `README.md`'s core promise excludes: *"When xpile
//! cannot guarantee that for a construct, it refuses at transpile time with a
//! reason instead of emitting code that silently diverges."* An empty module
//! is not a refusal; it is a wrong answer delivered successfully.
//!
//! The frontend stays **registered** and keeps claiming `.ruchy` via
//! [`Frontend::extensions`] deliberately: dropping the registration would make
//! `.ruchy` fall through to the generic `no frontend handles .ruchy` message,
//! which is less informative than naming the missing parser. Routing is real;
//! lowering refuses.
//!
//! Restoring Ruchy as an INPUT language means reusing ruchy's own parser + AST
//! (see `docs/specifications/xpile-spec.md`, the bidirectional Rust↔Ruchy
//! mHIR profiles) and is tracked for v0.2.0 — not a stub to be filled in here.

use std::path::Path;
use xpile_frontend::{Frontend, FrontendError};
use xpile_meta_hir::Module;

pub struct RuchyFrontend;

/// The refusal text. A single constant so the frontend and its tests assert
/// against one string rather than two drifting copies.
pub const RUCHY_INPUT_UNIMPLEMENTED: &str =
    "the Ruchy frontend has no parser — Ruchy is an OUTPUT language only \
     (`--target ruchy`), not an INPUT language. Lowering `.ruchy` source to \
     meta-HIR requires reusing ruchy's own parser + AST (v0.2.0). Refusing \
     rather than emitting an empty module";

impl Frontend for RuchyFrontend {
    fn name(&self) -> &'static str {
        "ruchy"
    }

    fn extensions(&self) -> &[&'static str] {
        &["ruchy"]
    }

    /// PMAT-1433: ALL of them. This frontend is routing-only, so its single
    /// claim is a refused claim. Stated here rather than left to
    /// `lowers_input() == false` so the two reports cannot disagree: the
    /// witness asserts `lowers_input() == false` IFF every claimed spelling is
    /// listed here, which is what makes the frontend-level boolean and the
    /// per-claim list one fact instead of two.
    fn refused_claims(&self) -> &[&'static str] {
        &["*.ruchy"]
    }

    /// PMAT-1346: routing only. Ruchy is emit-only; `.ruchy` input refuses.
    fn lowers_input(&self) -> bool {
        false
    }

    fn parse_and_lower(&self, path: &Path, _source: &str) -> Result<Module, FrontendError> {
        // PMAT-1346: refuse loudly. `_source` is genuinely unused — there is
        // no parser — and that is precisely what the refusal reports.
        Err(FrontendError::Unimplemented(format!(
            "{}: {RUCHY_INPUT_UNIMPLEMENTED}",
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(source: &str) -> String {
        let err = RuchyFrontend
            .parse_and_lower(Path::new("/tmp/prog.ruchy"), source)
            .expect_err("`.ruchy` input must refuse, never lower");
        err.to_string()
    }

    /// PMAT-1346 core: no `.ruchy` input lowers, for ANY source — including
    /// one that is perfectly valid Ruchy.
    #[test]
    fn parse_and_lower_refuses_valid_ruchy_source() {
        let msg = refusal("fun add(a: i64, b: i64) -> i64 { a + b }\n");
        assert!(
            msg.contains("no parser"),
            "refusal must name the missing parser; got: {msg}"
        );
        assert!(
            msg.contains("OUTPUT language only"),
            "refusal must say Ruchy is emit-only; got: {msg}"
        );
    }

    /// The regression that matters most: a trivial source must refuse too.
    /// Before PMAT-1346 every input produced an empty `Ok` module, so a test
    /// that only exercised empty input would have passed against the bug.
    #[test]
    fn parse_and_lower_refuses_even_trivial_source() {
        refusal("");
        refusal("// just a comment\n");
    }

    /// The refusal must be `Unimplemented`, not `Parse`/`Lower` — the user's
    /// source is not at fault, so a message blaming it would be false.
    #[test]
    fn refusal_is_unimplemented_not_a_parse_error() {
        let err = RuchyFrontend
            .parse_and_lower(Path::new("/tmp/prog.ruchy"), "fun main() {}")
            .expect_err("must refuse");
        assert!(
            matches!(err, FrontendError::Unimplemented(_)),
            "expected FrontendError::Unimplemented, got {err:?}"
        );
        assert!(
            err.to_string().starts_with("unimplemented frontend:"),
            "rendered refusal must not read as a parse error: {err}"
        );
    }

    /// The refusal names the offending FILE, so a multi-file dispatch
    /// (`xpile hybrid`) reports which input refused.
    #[test]
    fn refusal_names_the_input_path() {
        let err = RuchyFrontend
            .parse_and_lower(Path::new("/tmp/some/deep/prog.ruchy"), "fun main() {}")
            .expect_err("must refuse");
        assert!(
            err.to_string().contains("/tmp/some/deep/prog.ruchy"),
            "refusal must name the path: {err}"
        );
    }

    /// Routing is deliberately RETAINED — the refusal is only reachable
    /// because this frontend still claims `.ruchy`. If `extensions()` were
    /// emptied, `.ruchy` would hit the generic "no frontend handles" path and
    /// this whole refusal would become dead code.
    #[test]
    fn still_claims_dot_ruchy_so_the_refusal_is_reachable() {
        assert!(RuchyFrontend.matches_path(Path::new("/tmp/prog.ruchy")));
        assert_eq!(RuchyFrontend.extensions(), &["ruchy"]);
        assert_eq!(RuchyFrontend.name(), "ruchy");
    }
}
