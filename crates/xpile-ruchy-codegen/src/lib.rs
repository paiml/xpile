//! Ruchy backend — scaffold stub.
//!
//! Lowers meta-HIR to Ruchy source. Operates under
//! `Profile::RuchyOut` — the two-mHIR asymmetric profile that
//! reconstructs the pipeline operator and other Ruchy-specific syntax
//! at emission time. See `docs/specifications/sub/bidirectional-ruchy.md`
//! (planned).

use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, Profile, Target};
use xpile_meta_hir::Module;

pub struct RuchyBackend;

impl Backend for RuchyBackend {
    fn name(&self) -> &'static str {
        "ruchy"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Ruchy]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        let profile_note = match config.profile {
            Profile::RuchyOut => "RuchyOut (pipeline reconstruction enabled)",
            Profile::RustOut => "RustOut (no pipeline reconstruction)",
        };
        Ok(Artifact {
            primary: format!(
                "// xpile-ruchy-codegen scaffold\n// module: {}\n// profile: {}\n// TODO: lower meta-HIR to Ruchy source.\n",
                module.name, profile_note,
            ),
            sidecars: Vec::new(),
            citations: Vec::new(),
        })
    }
}
