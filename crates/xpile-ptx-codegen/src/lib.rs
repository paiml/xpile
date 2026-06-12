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
    QuorumPolicy, Target, TargetEmitter,
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

    /// PMAT-280 — End-to-end validation constructor for Section 29's
    /// multi-emitter routing.
    ///
    /// Builds a `PtxBackend` whose `MultiEmitterBackend` carries the
    /// `ScaffoldPtxEmitter` in the `general` slot AND a
    /// [`MatmulSpecialistEmitter`] in the `specialist` slot under
    /// `QuorumPolicy::PreferSpecialist`. The specialist matches only
    /// modules whose name starts with `matmul_` — the shape filter
    /// real specialists like `aprender-gpu` would use to claim
    /// GEMM-shaped kernels.
    ///
    /// This isn't registered in `default_session()` — production at
    /// v0.1.0+ still uses [`PtxBackend::new`]. The constructor exists
    /// so tests + future integrations can exercise the
    /// `MultiEmitterBackend::new_with_specialist` path against real
    /// production code (not just mock tests). It's the smallest
    /// concrete proof that the §29 routing layer is end-to-end
    /// usable, ahead of the heavy `rustc_codegen_nvvm` / `aprender-gpu`
    /// integrations that will eventually replace these placeholders.
    pub fn new_with_matmul_specialist() -> Self {
        Self {
            inner: MultiEmitterBackend::new_with_specialist(
                Target::Ptx,
                Box::new(ScaffoldPtxEmitter),
                Box::new(MatmulSpecialistEmitter),
                QuorumPolicy::PreferSpecialist,
            ),
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

/// PMAT-280 — Mock GEMM specialist emitter.
///
/// Matches modules whose name starts with `matmul_` — the shape
/// filter real specialists like `aprender-gpu` would use to claim
/// the GEMM/MMA kernel domain. Returns `None` from `try_emit` for
/// non-matching modules, letting the general emitter handle them.
/// For matching modules, emits a distinct PTX text (different from
/// the scaffold) so the `QuorumStatus::Multi` path is exercised
/// under non-trivial divergence.
///
/// This is intentionally not a real GEMM emitter — its job is to
/// prove that the `MultiEmitterBackend::new_with_specialist` routing
/// layer composes correctly with the existing `PtxBackend`. The
/// future `aprender-gpu` integration plugs in via the same trait
/// without touching `PtxBackend`'s public API.
struct MatmulSpecialistEmitter;

impl TargetEmitter for MatmulSpecialistEmitter {
    fn name(&self) -> &str {
        "matmul-specialist-mock"
    }

    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if !module.name.starts_with("matmul_") {
            return None;
        }
        let compute_capability = match &config.hardware {
            Some(HwProfile::Ptx { compute_capability }) => compute_capability,
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        };
        Some(Ok(EmittedText {
            primary: format!(
                "// matmul-specialist scaffold\n// module: {}\n// compute_capability: {}\n// TODO: emit mma.sync.aligned via aprender-gpu shape templates.\n",
                module.name, compute_capability,
            ),
            citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
        }))
    }
}

// ─── PMAT-481: offline PTX well-formedness gate (§30 Track 4) ────────
//
// A *structural* check on emitted PTX text — it does NOT execute
// anything and is not the model→emission gate (that is the `DiffExec`
// slice, PMAT-488). It exists so that the moment a real emitter lands
// (PMAT-485, the `nvptx64` path) its output is gated for well-formedness
// on FREE CI, and the `ptxas`-assembles step (wired with that emitter)
// derives its `-arch` from the same `compute_capability` checked here —
// never a hard-coded `sm_80`. Callers gate on [`ptx_looks_real`] so the
// v0.1.0 scaffold comment placeholder is never treated as real emission.

/// Reasons emitted PTX text fails the [`validate_ptx`] well-formedness gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtxValidationError {
    /// No `.version` directive — not PTX at all (e.g. the scaffold placeholder).
    MissingVersion,
    /// No `.target` directive.
    MissingTarget,
    /// `.target` arch does not match the requested compute capability.
    TargetMismatch { expected: String, found: String },
    /// No `.address_size 64` directive.
    MissingAddressSize,
    /// No `.visible .entry` kernel entry point.
    MissingEntry,
}

impl std::fmt::Display for PtxValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingVersion => write!(f, "PTX is missing a `.version` directive"),
            Self::MissingTarget => write!(f, "PTX is missing a `.target` directive"),
            Self::TargetMismatch { expected, found } => write!(
                f,
                "PTX `.target {found}` does not match requested compute capability `{expected}`"
            ),
            Self::MissingAddressSize => write!(f, "PTX is missing `.address_size 64`"),
            Self::MissingEntry => write!(f, "PTX has no `.visible .entry` kernel"),
        }
    }
}

impl std::error::Error for PtxValidationError {}

/// `true` when `text` looks like real PTX (carries a `.version`
/// directive) rather than the v0.1.0 scaffold comment placeholder.
pub fn ptx_looks_real(text: &str) -> bool {
    directive_present(text, ".version")
}

/// The `ptxas -arch=<…>` value for a PTX `.target` compute capability —
/// **derived, never hard-coded** (PMAT-481). The `ptxas` assemble step
/// (free CI, wired with the real emitter in PMAT-485) uses this so the
/// assembled arch always matches the emitted `.target`.
pub fn ptxas_arch(compute_capability: &str) -> String {
    format!("-arch={compute_capability}")
}

/// PMAT-481 — structural well-formedness check on emitted PTX text:
/// `.version`, `.target` matching `compute_capability`, `.address_size
/// 64`, and at least one `.visible .entry`. Pure text — no GPU, no
/// `ptxas`. Gate on [`ptx_looks_real`] first so the scaffold placeholder
/// is not treated as real emission.
pub fn validate_ptx(text: &str, compute_capability: &str) -> Result<(), PtxValidationError> {
    if !directive_present(text, ".version") {
        return Err(PtxValidationError::MissingVersion);
    }
    let target = ptx_target_arch(text).ok_or(PtxValidationError::MissingTarget)?;
    if target != compute_capability {
        return Err(PtxValidationError::TargetMismatch {
            expected: compute_capability.to_string(),
            found: target,
        });
    }
    if !directive_present(text, ".address_size 64") {
        return Err(PtxValidationError::MissingAddressSize);
    }
    if !text.contains(".visible .entry") {
        return Err(PtxValidationError::MissingEntry);
    }
    Ok(())
}

/// True when a non-comment line starts with `directive`.
fn directive_present(text: &str, directive: &str) -> bool {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .any(|l| l.starts_with(directive))
}

/// Extract the arch token (e.g. `sm_80`) from the `.target` directive.
fn ptx_target_arch(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|l| {
        if l.starts_with("//") {
            return None;
        }
        let rest = l.strip_prefix(".target")?;
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None; // e.g. `.target_foo` — not the directive
        }
        let arch = rest.trim().split([',', ' ']).next().unwrap_or("").trim();
        (!arch.is_empty()).then(|| arch.to_string())
    })
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

    // ─── PMAT-280: Multi-emitter validation tests ───────────────────

    fn matmul_module() -> Module {
        Module {
            name: "matmul_gemm_fp16".into(),
            source_lang: SourceLang::Rust,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        }
    }

    /// PMAT-280 — Matmul-named modules route through the specialist
    /// when the multi-emitter constructor is used. Under
    /// `PreferSpecialist`, the artifact reports the specialist's name
    /// and its emission body, not the scaffold's.
    #[test]
    fn matmul_module_routes_through_specialist_under_multi_emitter() {
        let backend = PtxBackend::new_with_matmul_specialist();
        let artifact = backend
            .lower(&matmul_module(), &ptx_config("sm_80"))
            .unwrap();
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "matmul-specialist-mock".to_string()
            },
            "PreferSpecialist with matching specialist should report Single {{ specialist }}"
        );
        assert!(
            artifact.primary.contains("matmul-specialist"),
            "primary should carry the specialist's emission body, got:\n{}",
            artifact.primary,
        );
    }

    /// PMAT-280 — Non-matmul modules fall back to the general (scaffold)
    /// emitter even when the multi-emitter constructor is used. The
    /// specialist returns `None` for unmatched shapes; the
    /// `MultiEmitterBackend` falls through cleanly.
    #[test]
    fn non_matmul_module_falls_back_to_general_under_multi_emitter() {
        let backend = PtxBackend::new_with_matmul_specialist();
        let artifact = backend
            .lower(&dummy_module(), &ptx_config("sm_80"))
            .unwrap();
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "xpile-ptx-codegen-scaffold".to_string()
            },
            "non-matching specialist should let general emit; QuorumStatus should reflect general"
        );
        assert!(
            artifact.primary.contains("xpile-ptx-codegen scaffold"),
            "primary should carry the general scaffold's emission body, got:\n{}",
            artifact.primary,
        );
    }

    /// PMAT-280 — The multi-emitter constructor still advertises the
    /// same target / name as the single-emitter constructor — the
    /// specialist is internal routing, not a separate Backend.
    #[test]
    fn multi_emitter_constructor_targets_match_single_emitter() {
        let multi = PtxBackend::new_with_matmul_specialist();
        let single = PtxBackend::new();
        assert_eq!(multi.targets(), single.targets());
        assert_eq!(multi.name(), single.name());
    }

    /// PMAT-280 — Same hardware-rejection eagerness regardless of
    /// constructor. The wrapper rejects `None`-hardware inputs before
    /// any emitter fires.
    #[test]
    fn multi_emitter_constructor_rejects_missing_hardware() {
        let backend = PtxBackend::new_with_matmul_specialist();
        let cfg = BackendConfig {
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: None,
        };
        let err = backend.lower(&matmul_module(), &cfg).unwrap_err();
        assert!(matches!(err, BackendError::MissingHardware(Target::Ptx)));
    }

    // ─── PMAT-481: offline PTX well-formedness gate ─────────────────

    /// A minimal but real PTX kernel, the shape `nvptx64-nvidia-cuda`
    /// rustc emits (verified on-box) — what PMAT-485 will produce.
    const GOLDEN_PTX_SM80: &str = "\
//
// Generated by LLVM NVPTX Back-End
//
.version 6.0
.target sm_80
.address_size 64

\t.visible .entry add_one(
\t\t.param .u64 add_one_param_0
\t)
\t{
\t\tret;
\t}
";

    #[test]
    fn validate_ptx_accepts_well_formed_kernel() {
        assert_eq!(validate_ptx(GOLDEN_PTX_SM80, "sm_80"), Ok(()));
    }

    #[test]
    fn ptx_looks_real_classifies_golden_vs_scaffold() {
        assert!(ptx_looks_real(GOLDEN_PTX_SM80));
        // The v0.1.0 scaffold output is comment-only — must NOT be
        // treated as real PTX (so PMAT-481 never false-fails on it).
        let scaffold = PtxBackend::new()
            .lower(&dummy_module(), &ptx_config("sm_80"))
            .unwrap()
            .primary;
        assert!(!ptx_looks_real(&scaffold));
    }

    #[test]
    fn validate_ptx_rejects_scaffold_placeholder() {
        let scaffold = PtxBackend::new()
            .lower(&dummy_module(), &ptx_config("sm_80"))
            .unwrap()
            .primary;
        assert_eq!(
            validate_ptx(&scaffold, "sm_80"),
            Err(PtxValidationError::MissingVersion)
        );
    }

    #[test]
    fn validate_ptx_detects_target_mismatch() {
        // arch is derived from the requested capability, never pinned.
        assert_eq!(
            validate_ptx(GOLDEN_PTX_SM80, "sm_90"),
            Err(PtxValidationError::TargetMismatch {
                expected: "sm_90".into(),
                found: "sm_80".into(),
            })
        );
    }

    #[test]
    fn validate_ptx_requires_address_size_and_entry() {
        let no_addr = ".version 6.0\n.target sm_80\n.visible .entry k() { ret; }\n";
        assert_eq!(
            validate_ptx(no_addr, "sm_80"),
            Err(PtxValidationError::MissingAddressSize)
        );
        let no_entry = ".version 6.0\n.target sm_80\n.address_size 64\n";
        assert_eq!(
            validate_ptx(no_entry, "sm_80"),
            Err(PtxValidationError::MissingEntry)
        );
    }

    #[test]
    fn ptxas_arch_derives_from_capability_not_hardcoded() {
        assert_eq!(ptxas_arch("sm_89"), "-arch=sm_89");
        assert_eq!(ptxas_arch("sm_90"), "-arch=sm_90");
    }
}
