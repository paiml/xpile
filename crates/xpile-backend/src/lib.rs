//! Backend trait — code-lane emission abstraction.
//!
//! Every target language in xpile (Rust, Ruchy, PTX, WGSL, SPIR-V, Lean)
//! provides one type implementing [`Backend`]. The trait is intentionally
//! narrow: take meta-HIR and a config, return an [`Artifact`].
//!
//! Sibling of `xpile-frontend::Frontend`. Architectural invariants
//! codified in `contracts/xpile-backend-trait-v1.yaml`.

use serde::{Deserialize, Serialize};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

/// Target language a backend can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    /// Idiomatic Rust source. Implemented by `xpile-rust-codegen`.
    Rust,
    /// Ruchy source. Implemented by `xpile-ruchy-codegen`.
    Ruchy,
    /// NVIDIA PTX text. Implemented by `xpile-ptx-codegen`.
    Ptx,
    /// WebGPU Shading Language. Implemented by `xpile-wgsl-codegen`.
    Wgsl,
    /// SPIR-V text or binary. Implemented by `xpile-spirv-codegen` (future).
    Spirv,
    /// Lean 4 executable code (def, partial def, inductive, ...).
    /// Implemented by `xpile-lean-codegen`. The proof-lane Lean (theorems)
    /// goes through `xpile-lean-contract-backend` instead.
    Lean,
}

/// Lowering profile — the two-mHIR asymmetric decision for Rust↔Ruchy.
///
/// See `docs/specifications/sub/bidirectional-ruchy.md` (planned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    /// meta-HIR normalized for Rust emission (default for most targets).
    RustOut,
    /// meta-HIR normalized for Ruchy emission (pipeline operator
    /// reconstructed at emission time).
    RuchyOut,
}

/// Hardware profile for targets whose emission depends on hardware
/// capabilities (PTX `compute_capability`, WGSL feature set, SPIR-V version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HwProfile {
    Ptx {
        /// e.g., "sm_80", "sm_89", "sm_90".
        compute_capability: String,
    },
    Wgsl {
        /// e.g., ["timestamp-query", "f16"].
        features: Vec<String>,
    },
    Spirv {
        version: (u32, u32),
    },
}

/// Configuration passed to [`Backend::lower`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub target: Target,
    pub profile: Profile,
    pub hardware: Option<HwProfile>,
}

/// Emitted artifact — primary source/IR text plus sidecar files plus
/// the structural citation chain to Layer-5 compile contracts.
///
/// The `citations` field is the structural channel that closes the
/// audit chain: every target-specific IR construct in `primary` cites
/// a Layer-5 compile contract by ID. Recovery is via this field, NOT
/// via regex over `primary` text. See
/// `contracts/xpile-backend-trait-v1.yaml` equation `compile_contract_citation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Emitted source / IR text (Rust source, Ruchy source, PTX text,
    /// WGSL source, SPIR-V text, Lean source).
    pub primary: String,
    /// Optional binaries, manifests, debug maps, accompanying the primary.
    pub sidecars: Vec<(String, Vec<u8>)>,
    /// Layer-5 compile contracts sanctioning every target-specific
    /// construct in `primary`. Structural — not regex-recoverable.
    pub citations: Vec<ContractId>,
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("unsupported target: {0:?}")]
    UnsupportedTarget(Target),
    #[error("missing hardware profile for target {0:?}")]
    MissingHardware(Target),
    #[error("lowering error: {0}")]
    Lower(String),
    #[error("compile-contract citation missing for emitted construct: {0}")]
    MissingCompileContractCitation(String),
}

/// Code-lane emission trait. See `docs/specifications/sub/backend-trait.md`.
pub trait Backend: Send + Sync {
    /// Human-readable backend name, e.g. "rust", "ptx", "wgsl".
    fn name(&self) -> &'static str;

    /// Targets this backend can emit. Each [`Target`] variant is
    /// owned by exactly one Backend impl (`target_ownership` invariant).
    fn targets(&self) -> &[Target];

    /// Lower a meta-HIR module under the given config to an [`Artifact`].
    ///
    /// Invariants: deterministic per `(module, config)`; frame-pure
    /// (no mutation of inputs); every target-specific IR construct
    /// in `Artifact.primary` cited via `Artifact.citations`.
    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError>;
}
