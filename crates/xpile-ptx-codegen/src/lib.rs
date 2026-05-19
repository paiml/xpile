//! PTX backend.
//!
//! Lowers Rust meta-HIR (functions annotated `#[gpu_kernel(...)]`) to
//! NVIDIA PTX text targeting `sm_80`+. Layer 5 compile contract:
//! `contracts/compile-rust-to-ptx-mma-v1.yaml`.
//!
//! **Architecture (PMAT-264 / Section 29):** [`PtxBackend`] wraps a
//! [`MultiEmitterBackend`] so emission routes through the same
//! general/specialist quorum framework that will eventually carry
//! `rustc_codegen_nvvm` (general) + `aprender-gpu` (specialist). At
//! v0.1.0 the wrapper holds a single [`ScaffoldPtxEmitter`] in the
//! general slot — the same code path real emitters will plug into.
//!
//! When `rustc_codegen_nvvm` lights up (next phase per
//! `sub/layer5-multi-emitter-quorum.md`), it slots into the `general`
//! position; when `aprender-gpu` ships its bridge, it slots into the
//! `specialist` position; no changes to [`PtxBackend`]'s public API.

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, HwProfile, MultiEmitterBackend,
    Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

/// PTX backend — `Backend` impl wrapping a [`MultiEmitterBackend`] so
/// the v0.1.0 scaffold drives through the same routing the future
/// `rustc_codegen_nvvm` + `aprender-gpu` quorum will use.
pub struct PtxBackend {
    inner: MultiEmitterBackend,
}

impl Default for PtxBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PtxBackend {
    pub fn new() -> Self {
        Self {
            inner: MultiEmitterBackend::new_single(Target::Ptx, Box::new(ScaffoldPtxEmitter)),
        }
    }
}

impl Backend for PtxBackend {
    fn name(&self) -> &'static str {
        "ptx"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Ptx]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        // Reject inputs without an HwProfile::Ptx eagerly — the
        // scaffold emitter can't synthesize a compute_capability and
        // the contract requires one.
        match &config.hardware {
            Some(HwProfile::Ptx { .. }) => {}
            _ => return Err(BackendError::MissingHardware(Target::Ptx)),
        }
        self.inner.lower(module, config)
    }
}

/// Scaffold emitter — produces the placeholder PTX text current users
/// see at v0.1.0. Will be replaced by `rustc_codegen_nvvm` integration
/// in the next Section 29 phase.
struct ScaffoldPtxEmitter;

impl TargetEmitter for ScaffoldPtxEmitter {
    fn name(&self) -> &str {
        "xpile-ptx-codegen-scaffold"
    }

    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        let compute_capability = match &config.hardware {
            Some(HwProfile::Ptx { compute_capability }) => compute_capability,
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        };
        Some(Ok(EmittedText {
            primary: format!(
                "// xpile-ptx-codegen scaffold\n// module: {}\n// compute_capability: {}\n// TODO: lower to real PTX via rustc_codegen_nvvm.\n",
                module.name, compute_capability,
            ),
            citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
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

    fn ptx_config(sm: &str) -> BackendConfig {
        BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Ptx {
                compute_capability: sm.to_string(),
            }),
        }
    }

    #[test]
    fn ptx_backend_emits_through_multi_emitter() {
        let backend = PtxBackend::new();
        let artifact = backend
            .lower(&dummy_module(), &ptx_config("sm_80"))
            .unwrap();
        // Quorum status comes from the wrapped MultiEmitterBackend,
        // which means the scaffold emitter name is propagated.
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "xpile-ptx-codegen-scaffold".to_string()
            }
        );
        assert!(artifact.primary.contains("sm_80"));
        assert!(artifact
            .citations
            .iter()
            .any(|c| c.as_str() == "C-COMPILE-RUST-TO-PTX-MMA"));
    }

    #[test]
    fn ptx_backend_rejects_missing_hardware() {
        let backend = PtxBackend::new();
        let cfg = BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: None,
        };
        let err = backend.lower(&dummy_module(), &cfg).unwrap_err();
        assert!(matches!(err, BackendError::MissingHardware(Target::Ptx)));
    }

    #[test]
    fn ptx_backend_targets_only_ptx() {
        let backend = PtxBackend::new();
        assert_eq!(backend.targets(), &[Target::Ptx]);
        assert_eq!(backend.name(), "ptx");
    }
}
