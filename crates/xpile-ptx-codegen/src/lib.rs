//! PTX backend — scaffold stub.
//!
//! Lowers Rust meta-HIR (functions annotated `#[gpu_kernel(...)]`) to
//! NVIDIA PTX text targeting `sm_80`+. Real emission goes via
//! `rustc_codegen_nvvm` or `rust-cuda`; this crate is the xpile-side
//! Backend adapter.
//!
//! Layer 5 compile contract: `contracts/compile-rust-to-ptx-mma-v1.yaml`.

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, HwProfile, QuorumStatus, Target,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

pub struct PtxBackend;

impl Backend for PtxBackend {
    fn name(&self) -> &'static str {
        "ptx"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Ptx]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        match &config.hardware {
            Some(HwProfile::Ptx { compute_capability }) => Ok(Artifact {
                primary: format!(
                    "// xpile-ptx-codegen scaffold\n// module: {}\n// compute_capability: {}\n// TODO: lower to real PTX via rustc_codegen_nvvm.\n",
                    module.name, compute_capability,
                ),
                sidecars: Vec::new(),
                citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
                quorum_status: QuorumStatus::Single {
                    emitter: "xpile-ptx-codegen-scaffold".to_string(),
                },
            }),
            _ => Err(BackendError::MissingHardware(Target::Ptx)),
        }
    }
}
