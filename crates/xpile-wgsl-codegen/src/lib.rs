//! WGSL backend — scaffold stub.
//!
//! Lowers Rust meta-HIR to WebGPU Shading Language. Validation via
//! `naga`. Layer 5 compile contracts live under
//! `contracts/compile-rust-to-wgsl-*.yaml` (to author).

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, HwProfile, QuorumStatus, Target,
};
use xpile_meta_hir::Module;

pub struct WgslBackend;

impl Backend for WgslBackend {
    fn name(&self) -> &'static str {
        "wgsl"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Wgsl]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        let features = match &config.hardware {
            Some(HwProfile::Wgsl { features }) => features.clone(),
            None => Vec::new(),
            _ => return Err(BackendError::MissingHardware(Target::Wgsl)),
        };
        Ok(Artifact {
            primary: format!(
                "// xpile-wgsl-codegen scaffold\n// module: {}\n// features: {:?}\n// TODO: lower to WGSL.\n",
                module.name, features,
            ),
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-wgsl-codegen-scaffold".to_string(),
            },
        })
    }
}
