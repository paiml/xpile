//! POSIX shell backend for xpile — Layer A scaffold.
//!
//! This is the v0.1.0 scaffold for the bashrs merger (see
//! `docs/specifications/sub/bashrs-merger.md`). It implements the
//! [`Backend`] trait so `Target::Shell` has a registered emitter,
//! but the real ShellIR + quoting machinery + ShellCheck-compatible
//! verifier is deferred to v0.2.0 (Layer A weeks 1-6).
//!
//! At v0.1.0 `lower` emits a placeholder POSIX-shell comment
//! identifying the module name + the `C-BASHRS-POSIX-IDEMPOTENCE`
//! Layer-1 contract citation (using the same `# xpile-contract: ...`
//! comment idiom that `xpile-rust-codegen` uses for its citations,
//! but with `#` instead of `//` — sh's comment syntax).

use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Target};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

pub struct BashrsBackend;

impl Backend for BashrsBackend {
    fn name(&self) -> &'static str {
        "bashrs"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Shell]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        // v0.1.0 scaffold: a self-describing comment that:
        //   (a) carries the contract citation (so falsifier F1 / the
        //       `xpile audit` pipeline finds the bashrs domain when it
        //       grows to recognise `#`-prefixed citations).
        //   (b) names the scaffold status explicitly so a human running
        //       `xpile transpile foo.sh --target shell` sees what's
        //       wired and what isn't.
        let primary = format!(
            "#!/bin/sh\n\
             # xpile-bashrs-backend scaffold (v0.1.0 PMAT-037 / XPILE-BASHRS-MERGER-001)\n\
             # xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE\n\
             # module: {}\n\
             # source_lang: {:?}\n\
             # TODO: lower meta-HIR shell variants to ShellCheck-clean POSIX sh\n\
             # via the bashrs runtime, landing at v0.2.0 with the source fold.\n",
            module.name, module.source_lang,
        );
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: vec![ContractId::new("C-BASHRS-POSIX-IDEMPOTENCE")],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_backend::{BackendConfig, Profile};
    use xpile_meta_hir::SourceLang;

    fn empty_shell_module() -> Module {
        Module {
            name: "demo".into(),
            source_lang: SourceLang::Shell,
            items: vec![],
            ffi_boundaries: vec![],
        }
    }

    #[test]
    fn name_is_bashrs() {
        assert_eq!(BashrsBackend.name(), "bashrs");
    }

    #[test]
    fn targets_shell() {
        assert_eq!(BashrsBackend.targets(), &[Target::Shell]);
    }

    #[test]
    fn lower_emits_scaffold_with_citation() {
        let module = empty_shell_module();
        let config = BackendConfig {
            target: Target::Shell,
            profile: Profile::RustOut,
            hardware: None,
        };
        let art = BashrsBackend.lower(&module, &config).expect("lower");
        // Scaffold-defining landmarks:
        assert!(
            art.primary.contains("xpile-bashrs-backend scaffold"),
            "scaffold marker absent: {}",
            art.primary
        );
        assert!(
            art.primary
                .contains("# xpile-contract: C-BASHRS-POSIX-IDEMPOTENCE"),
            "citation absent: {}",
            art.primary
        );
        assert!(
            art.primary.contains("# module: demo"),
            "module name absent: {}",
            art.primary
        );
        assert_eq!(art.citations.len(), 1);
    }
}
