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
    /// POSIX shell (sh / bash / zsh) — the bashrs merger domain.
    /// Implemented by `bashrs-backend` (scaffold at v0.1.0; full emit
    /// at v0.2.0 once the bashrs source folding lands). PMAT-037 /
    /// XPILE-BASHRS-MERGER-001. See `sub/bashrs-merger.md` Layer A.
    Shell,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    /// Emitted source / IR text (Rust source, Ruchy source, PTX text,
    /// WGSL source, SPIR-V text, Lean source).
    pub primary: String,
    /// Optional binaries, manifests, debug maps, accompanying the primary.
    pub sidecars: Vec<(String, Vec<u8>)>,
    /// Layer-5 compile contracts sanctioning every target-specific
    /// construct in `primary`. Structural — not regex-recoverable.
    pub citations: Vec<ContractId>,
    /// Multi-emitter quorum status (PMAT-262 / Section 29). At v0.1.0
    /// every Backend impl emits exactly one artifact, so this is always
    /// `QuorumStatus::Single { emitter: <backend_name> }`. Multi-emitter
    /// backends (future rustc_codegen_nvvm + aprender-gpu quorum on PTX)
    /// populate `QuorumStatus::Multi { ... }` with the diff_exec result.
    ///
    /// Defaults via serde to `Single { emitter: "unknown" }` for
    /// backward-compatible deserialization of older JSON payloads.
    #[serde(default = "default_quorum_status")]
    pub quorum_status: QuorumStatus,
}

fn default_quorum_status() -> QuorumStatus {
    QuorumStatus::Single {
        emitter: "unknown".to_string(),
    }
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

// ─── PMAT-261 / Section 29: Multi-emitter quorum scaffolding ────────────
//
// Types codifying the design in
// `docs/specifications/sub/layer5-multi-emitter-quorum.md`. Pure
// scaffolding at PMAT-261 — no Backend impl yet uses these. Future PRs
// (rustc_codegen_nvvm wiring, aprender-gpu bridge, DiffExec engine)
// build against this stable API surface.

/// Role of a backend emitter within a multi-emitter quorum.
///
/// `compile_targets.via.role` in the YAML schema corresponds to this
/// enum. At most one `General` per quorum (the mandatory fallback);
/// any number of `Specialist` (each with its own `shape_filter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmitterRole {
    /// Handles any contract-conforming input. Mandatory fallback —
    /// `pv lint` (post-PMAT-262) will require at least one `General`
    /// emitter per Layer-5 contract's `compile_targets.via`.
    /// Examples: `rustc_codegen_nvvm` (PTX), `naga` (WGSL),
    /// `rspirv` (SPIR-V).
    General,
    /// Handles a domain-specific subset via hand-tuned templates.
    /// Optional — degrades gracefully to single-emitter when missing.
    /// Examples: `aprender-gpu` (GEMM/MMA PTX kernels),
    /// `bashrs-realistic` (corpus-tuned POSIX patterns).
    Specialist,
}

/// Policy for combining outputs when both General and Specialist
/// emitters fire on the same input. Configured per Layer-5 contract
/// via `compile_targets.quorum_policy` in the YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum QuorumPolicy {
    /// If the specialist handles the kernel, use its output. Falls
    /// back to general otherwise. Single-vote Runtime stratum.
    PreferSpecialist,
    /// Emit via BOTH, run BOTH on test inputs, compare numerical
    /// outputs within tolerance. Multi-vote Runtime stratum.
    /// **Falsifies the contract on divergence** — this is the
    /// stratum-upgrading policy that closes the §4 "Run=1 demo
    /// fixture" caveat from audit-design.md.
    DiffExec {
        /// Maximum allowed absolute difference between corresponding
        /// numerical outputs of the two emitters.
        tolerance: f64,
    },
    /// Strict text-equality between PTX outputs. Useful for
    /// regression-locking, NOT for falsification — different valid
    /// PTX programs commonly produce identical execution results via
    /// different instruction sequences.
    Strict,
}

/// Status of a multi-emitter quorum vote, attached to an [`Artifact`]
/// produced by a multi-emitter backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuorumStatus {
    /// Only one emitter fired (specialist missing for this shape, or
    /// quorum policy is `PreferSpecialist` and specialist matched).
    /// Runtime stratum vote count: 1.
    Single { emitter: String },
    /// Both emitters fired; outputs were combined under the policy.
    /// Runtime stratum vote count: 2.
    Multi {
        emitters: Vec<String>,
        diff_exec: Option<DiffExecResult>,
    },
}

/// Result of a `DiffExec` quorum policy execution. Two PTX (or WGSL/
/// SPIR-V) programs were run on test inputs; the engine compared
/// their numerical outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffExecResult {
    /// Outputs matched within tolerance. Records the max absolute
    /// difference observed for audit trail.
    Match { max_abs_diff: f64 },
    /// Outputs diverged beyond tolerance. **Contract violation** —
    /// CI fails. Records the divergence for diagnosis.
    Divergent { max_abs_diff: f64, tolerance: f64 },
    /// Engine did not run (e.g., test hardware unavailable). Vote
    /// downgrades from Runtime to a placeholder. The substrate
    /// records this rather than silently dropping it.
    NotRun { reason: String },
}

/// A single emitter entry from `compile_targets.via` in the YAML
/// schema. Mirrors the structured-record form spec'd in
/// `sub/layer5-multi-emitter-quorum.md` §"Contract YAML schema extension".
///
/// At v0.1.0 the YAML schema is still a flat `[String]`; this struct
/// is the v0.2.0+ target representation `pv lint` will deserialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViaEntry {
    /// Emitter name, e.g. "rustc_codegen_nvvm", "aprender-gpu".
    pub emitter: String,
    /// Role within the quorum.
    pub role: EmitterRole,
    /// Local crate that registers this emitter (for `role: general`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    /// Cross-repo binding target (for `role: specialist` cases where
    /// the emitter lives in a different fleet repo, e.g. aprender).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_repo: Option<String>,
    /// Optional shape filter — for specialists, identifies which
    /// input shapes this emitter handles. Tied to a sub-contract
    /// (e.g., `gemm_fp16_mma_64x128` matches aprender's
    /// `C-COMPUTE-GEMM-FP16-MMA` contract).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_filter: Option<String>,
}

#[cfg(test)]
mod quorum_scaffolding_tests {
    use super::*;

    #[test]
    fn emitter_role_serde_round_trip() {
        let general = EmitterRole::General;
        let s = serde_json::to_string(&general).unwrap();
        assert_eq!(s, "\"general\"");
        let back: EmitterRole = serde_json::from_str(&s).unwrap();
        assert_eq!(back, general);

        let specialist = EmitterRole::Specialist;
        let s = serde_json::to_string(&specialist).unwrap();
        assert_eq!(s, "\"specialist\"");
    }

    #[test]
    fn quorum_policy_diff_exec_carries_tolerance() {
        let policy = QuorumPolicy::DiffExec { tolerance: 1.0e-3 };
        let s = serde_json::to_string(&policy).unwrap();
        assert!(s.contains("diff_exec"));
        assert!(s.contains("0.001"));
        let back: QuorumPolicy = serde_json::from_str(&s).unwrap();
        assert_eq!(back, policy);
    }

    #[test]
    fn quorum_status_multi_records_emitters_and_diff() {
        let status = QuorumStatus::Multi {
            emitters: vec!["rustc_codegen_nvvm".into(), "aprender-gpu".into()],
            diff_exec: Some(DiffExecResult::Match {
                max_abs_diff: 1.3e-4,
            }),
        };
        let s = serde_json::to_string(&status).unwrap();
        assert!(s.contains("multi"));
        assert!(s.contains("rustc_codegen_nvvm"));
        assert!(s.contains("aprender-gpu"));
    }

    #[test]
    fn diff_exec_divergent_carries_both_diff_and_tolerance() {
        let r = DiffExecResult::Divergent {
            max_abs_diff: 0.5,
            tolerance: 0.001,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: DiffExecResult = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn via_entry_general_has_no_specialist_fields() {
        let v = ViaEntry {
            emitter: "rustc_codegen_nvvm".into(),
            role: EmitterRole::General,
            crate_name: Some("xpile-ptx-codegen".into()),
            cross_repo: None,
            shape_filter: None,
        };
        let s = serde_json::to_string(&v).unwrap();
        // Optional None fields skipped from output.
        assert!(!s.contains("cross_repo"));
        assert!(!s.contains("shape_filter"));
        assert!(s.contains("general"));
    }

    #[test]
    fn via_entry_specialist_carries_cross_repo_and_shape_filter() {
        let v = ViaEntry {
            emitter: "aprender-gpu".into(),
            role: EmitterRole::Specialist,
            crate_name: None,
            cross_repo: Some("aprender".into()),
            shape_filter: Some("gemm_fp16_mma_64x128".into()),
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("specialist"));
        assert!(s.contains("aprender"));
        assert!(s.contains("gemm_fp16_mma_64x128"));
    }

    /// PMAT-262: Artifact carries QuorumStatus; default deserialization
    /// gracefully populates Single { emitter: "unknown" } for older JSON
    /// payloads that predate the field.
    #[test]
    fn artifact_quorum_status_defaults_for_older_payloads() {
        let legacy_json = r#"{"primary":"// test","sidecars":[],"citations":[]}"#;
        let a: Artifact = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(
            a.quorum_status,
            QuorumStatus::Single {
                emitter: "unknown".to_string()
            }
        );
    }

    /// PMAT-262: Artifact round-trips QuorumStatus::Single produced by
    /// every single-emitter backend at v0.1.0.
    #[test]
    fn artifact_quorum_status_single_round_trips() {
        let a = Artifact {
            primary: "// test".into(),
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-rust-codegen".to_string(),
            },
        };
        let s = serde_json::to_string(&a).unwrap();
        let back: Artifact = serde_json::from_str(&s).unwrap();
        assert_eq!(back, a);
    }
}
