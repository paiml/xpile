//! WGSL backend.
//!
//! Lowers Rust meta-HIR to WebGPU Shading Language. Validation via
//! `naga`. Layer 5 compile contracts live under
//! `contracts/compile-rust-to-wgsl-*.yaml` (to author).
//!
//! **Architecture (PMAT-265 / Section 29):** [`WgslBackend`] wraps a
//! [`MultiEmitterBackend`] (same pattern as [`xpile_ptx_codegen::PtxBackend`])
//! so emission routes through the general/specialist quorum framework.
//! At v0.1.0 the wrapper holds a single [`ScaffoldWgslEmitter`] in the
//! general slot. When a real WGSL emitter (e.g. `naga` round-trip or
//! `rust-gpu` SPIR-V→WGSL) ships, it slots into `general`; an aprender
//! `aprender-webgpu` could slot into `specialist`.

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, HwProfile, MultiEmitterBackend,
    Target, TargetEmitter,
};
use xpile_meta_hir::Module;

/// WGSL backend — `Backend` impl wrapping a [`MultiEmitterBackend`] so
/// the v0.1.0 scaffold drives through the same routing the future
/// real-emitter + specialist quorum will use.
pub struct WgslBackend {
    inner: MultiEmitterBackend,
}

impl Default for WgslBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WgslBackend {
    pub fn new() -> Self {
        Self {
            inner: MultiEmitterBackend::new_single(Target::Wgsl, Box::new(ScaffoldWgslEmitter)),
        }
    }
}

impl Backend for WgslBackend {
    fn name(&self) -> &'static str {
        "wgsl"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Wgsl]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        // Validate hardware shape — WGSL accepts `None` (defaulting to
        // an empty feature list) but rejects non-Wgsl HwProfiles.
        match &config.hardware {
            None | Some(HwProfile::Wgsl { .. }) => {}
            _ => return Err(BackendError::MissingHardware(Target::Wgsl)),
        }
        self.inner.lower(module, config)
    }
}

/// Scaffold emitter — produces the placeholder WGSL text current users
/// see at v0.1.0. Will be replaced by a real emitter in a future
/// Section 29 phase.
struct ScaffoldWgslEmitter;

impl TargetEmitter for ScaffoldWgslEmitter {
    fn name(&self) -> &str {
        "xpile-wgsl-codegen-scaffold"
    }

    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        let features = match &config.hardware {
            Some(HwProfile::Wgsl { features }) => features.clone(),
            None => Vec::new(),
            _ => return Some(Err(BackendError::MissingHardware(Target::Wgsl))),
        };
        Some(Ok(EmittedText {
            primary: format!(
                "// xpile-wgsl-codegen scaffold\n// module: {}\n// features: {:?}\n// TODO: lower to WGSL.\n",
                module.name, features,
            ),
            citations: Vec::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_backend::{Profile, QuorumStatus};
    use xpile_meta_hir::SourceLang;

    fn dummy_module() -> Module {
        Module {
            name: "test_kernel".into(),
            source_lang: SourceLang::Rust,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        }
    }

    fn wgsl_config(features: Vec<String>) -> BackendConfig {
        BackendConfig {
            target: Target::Wgsl,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Wgsl { features }),
        }
    }

    #[test]
    fn wgsl_backend_emits_through_multi_emitter() {
        let backend = WgslBackend::new();
        let artifact = backend
            .lower(&dummy_module(), &wgsl_config(vec!["f16".into()]))
            .unwrap();
        // Quorum status comes from the wrapped MultiEmitterBackend,
        // which means the scaffold emitter name is propagated.
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "xpile-wgsl-codegen-scaffold".to_string()
            }
        );
        assert!(artifact.primary.contains("f16"));
        assert!(artifact.primary.contains("test_kernel"));
    }

    #[test]
    fn wgsl_backend_accepts_no_hardware() {
        // WGSL allows None hardware — defaults to empty feature list.
        let backend = WgslBackend::new();
        let cfg = BackendConfig {
            target: Target::Wgsl,
            profile: Profile::RustOut,
            hardware: None,
        };
        let artifact = backend.lower(&dummy_module(), &cfg).unwrap();
        assert!(artifact.primary.contains("features: []"));
    }

    #[test]
    fn wgsl_backend_rejects_wrong_hardware() {
        let backend = WgslBackend::new();
        let cfg = BackendConfig {
            target: Target::Wgsl,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Ptx {
                compute_capability: "sm_80".to_string(),
            }),
        };
        let err = backend.lower(&dummy_module(), &cfg).unwrap_err();
        assert!(matches!(err, BackendError::MissingHardware(Target::Wgsl)));
    }

    #[test]
    fn wgsl_backend_targets_only_wgsl() {
        let backend = WgslBackend::new();
        assert_eq!(backend.targets(), &[Target::Wgsl]);
        assert_eq!(backend.name(), "wgsl");
    }
}
