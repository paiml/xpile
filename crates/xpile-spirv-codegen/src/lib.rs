//! SPIR-V backend — the native Vulkan IR lane (PMAT-960).
//!
//! Lowers Rust meta-HIR to SPIR-V by **reusing** the proven
//! `xpile-wgsl-codegen` WGSL emission and compiling it through
//! `naga`: WGSL → `naga::front::wgsl::parse_str` →
//! `naga::valid::Validator` → `naga::back::spv` → SPIR-V binary words.
//! The SPIR-V is NOT hand-assembled — naga's `spv` backend is the
//! emitter, exactly as the WGSL lane uses naga/wgpu for validation.
//!
//! **Architecture (mirrors `xpile_wgsl_codegen::WgslBackend`, PMAT-950).**
//! [`SpirvBackend`] wraps a [`MultiEmitterBackend`] so emission routes
//! through the §29 general/specialist quorum framework. The single-emitter
//! constructor holds one real [`SpirvSaxpyGeneralEmitter`] in the general
//! slot; the witness constructor holds two REAL emitters
//! ([`SpirvSaxpyGeneralEmitter`] + [`SpirvSaxpySpecialistEmitter`]) that
//! reuse the WGSL `2.0*x + 1.0` / `fma` shaders, compile each to SPIR-V,
//! and (with a Vulkan adapter present) run BOTH on the GPU under the
//! [`SpirvDiffExecEngine`].
//!
//! Layer 5 compile contract: `contracts/compile-rust-to-spirv-v1.yaml`
//! (`C-COMPILE-RUST-TO-SPIRV`), proof lane
//! `contracts/lean/CompileRustToSpirv.lean`. The WGSL sibling — where the
//! WGSL lane attests a *portable* compute abstraction, this lane attests
//! the *native Vulkan IR* the same kernels lower to.

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, HwProfile, MultiEmitterBackend,
    QuorumPolicy, Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

mod spirv_diffexec;
pub use spirv_diffexec::{
    general_metahir_module, general_real_wgsl, vulkan_adapter_available, SpirvDiffExecEngine,
    EXPECTED_OUTPUT, FIXTURE_INPUT,
};

/// The Layer-5 compile contract every emitted SPIR-V artifact cites.
const CONTRACT_ID: &str = "C-COMPILE-RUST-TO-SPIRV";

// ─── WGSL→naga→SPIR-V compilation (the reuse core) ─────────────────────

/// Reasons compiling reused WGSL to SPIR-V via naga fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpirvCompileError {
    /// `naga::front::wgsl::parse_str` rejected the WGSL.
    WgslParse(String),
    /// `naga::valid::Validator` rejected the parsed module.
    Validate(String),
    /// `naga::back::spv` failed to emit SPIR-V words.
    SpvEmit(String),
}

impl std::fmt::Display for SpirvCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WgslParse(e) => write!(f, "WGSL parse failed: {e}"),
            Self::Validate(e) => write!(f, "naga validation failed: {e}"),
            Self::SpvEmit(e) => write!(f, "SPIR-V emission failed: {e}"),
        }
    }
}

impl std::error::Error for SpirvCompileError {}

/// SPIR-V magic number (`0x07230203`) — the first word of every valid
/// SPIR-V binary module. Used by [`spirv_looks_real`] / [`validate_spirv`].
pub const SPIRV_MAGIC: u32 = 0x0723_0203;

/// Compile a WGSL compute-shader source to SPIR-V binary words via naga.
///
/// REUSE path: parse WGSL (`wgsl-in`) → validate → emit SPIR-V
/// (`spv-out`). No hand-written SPIR-V assembler. Targets Vulkan 1.1
/// (SPIR-V 1.3), the floor wgpu's Vulkan backend consumes.
pub fn wgsl_to_spirv_words(wgsl: &str) -> Result<Vec<u32>, SpirvCompileError> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|e| SpirvCompileError::WgslParse(e.to_string()))?;

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator
        .validate(&module)
        .map_err(|e| SpirvCompileError::Validate(format!("{e:?}")))?;

    let options = naga::back::spv::Options {
        lang_version: (1, 3),
        ..Default::default()
    };
    let words = naga::back::spv::write_vec(&module, &info, &options, None)
        .map_err(|e| SpirvCompileError::SpvEmit(e.to_string()))?;
    Ok(words)
}

/// Render a human-readable text summary of an emitted SPIR-V module for
/// [`Artifact::primary`]. SPIR-V is binary; the primary text is a stable,
/// auditable disassembly-lite header (magic, version, word count, the
/// source WGSL kept inline as a `; ` comment for round-trip clarity).
/// The raw binary words go in a sidecar.
pub fn spirv_text_summary(words: &[u32], source_wgsl: &str) -> String {
    let version = words.get(1).copied().unwrap_or(0);
    let major = (version >> 16) & 0xff;
    let minor = (version >> 8) & 0xff;
    let mut out = String::new();
    out.push_str("; SPIR-V\n");
    out.push_str(&format!(
        "; Magic:     0x{:08x}\n",
        words.first().copied().unwrap_or(0)
    ));
    out.push_str(&format!("; Version:   {major}.{minor}\n"));
    out.push_str(&format!("; Words:     {}\n", words.len()));
    out.push_str("; Emitter:   xpile-spirv-codegen (WGSL -> naga -> spv)\n");
    out.push_str("; Source WGSL (reused from xpile-wgsl-codegen):\n");
    for line in source_wgsl.lines() {
        out.push_str(";   ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `true` when `words` is a real SPIR-V binary (begins with the SPIR-V
/// magic number) rather than an empty / scaffold artifact.
pub fn spirv_looks_real(words: &[u32]) -> bool {
    words.first().copied() == Some(SPIRV_MAGIC)
}

/// Structural well-formedness gate on emitted SPIR-V words (CPU-only, no
/// GPU): a non-empty word stream whose first word is the SPIR-V magic and
/// whose declared bound (word 3) is non-zero. Mirrors the WGSL
/// `validate_wgsl` offline gate.
pub fn validate_spirv(words: &[u32]) -> Result<(), SpirvCompileError> {
    if words.first().copied() != Some(SPIRV_MAGIC) {
        return Err(SpirvCompileError::SpvEmit(format!(
            "not SPIR-V: first word 0x{:08x} != magic 0x{SPIRV_MAGIC:08x}",
            words.first().copied().unwrap_or(0)
        )));
    }
    // SPIR-V header is 5 words: magic, version, generator, bound, schema.
    if words.len() < 5 {
        return Err(SpirvCompileError::SpvEmit(format!(
            "truncated SPIR-V header: {} words (need >= 5)",
            words.len()
        )));
    }
    if words[3] == 0 {
        return Err(SpirvCompileError::SpvEmit(
            "SPIR-V id-bound is zero (empty module)".to_string(),
        ));
    }
    Ok(())
}

// ─── Backend ───────────────────────────────────────────────────────────

/// SPIR-V backend — `Backend` impl wrapping a [`MultiEmitterBackend`], so
/// the v0.1.0 path drives through the same §29 routing the witness uses.
pub struct SpirvBackend {
    inner: MultiEmitterBackend,
}

impl Default for SpirvBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SpirvBackend {
    /// Single-emitter constructor: one real SPIR-V emitter (the reused
    /// WGSL `2.0*x + 1.0` compiled to SPIR-V) in the general slot.
    pub fn new() -> Self {
        Self {
            inner: MultiEmitterBackend::new_single(
                Target::Spirv,
                Box::new(SpirvSaxpyGeneralEmitter),
            ),
        }
    }

    /// PMAT-960 — the executed Vulkan SPIR-V-witness constructor (§29).
    ///
    /// Sibling of [`xpile_wgsl_codegen::WgslBackend::new_wgpu_diffexec_witness`].
    /// Builds a `SpirvBackend` whose `MultiEmitterBackend` carries two REAL
    /// SPIR-V emitters — [`SpirvSaxpyGeneralEmitter`] (reused WGSL
    /// `2.0*x + 1.0` → SPIR-V) and [`SpirvSaxpySpecialistEmitter`] (reused
    /// WGSL `fma` → SPIR-V) — under `QuorumPolicy::DiffExec`, with a
    /// [`SpirvDiffExecEngine`] installed when a Vulkan adapter is present.
    /// Both compute `out[i] = 2*in[i] + 1` via *categorically different*
    /// SPIR-V (one an explicit mul+add, one an `OpExtInst Fma`), so the
    /// `DiffExec` quorum runs BOTH SPIR-V modules on the GPU and asserts
    /// they agree — the native-Vulkan-IR sibling of the WGSL witness.
    ///
    /// On a Vulkan host this records a real
    /// [`xpile_backend::DiffExecResult::Match`]; with no adapter the engine
    /// is NOT installed and the backend records the benign
    /// `NotRun { no-engine }` (free CI stays green — the wgpu/nvcc/cc
    /// graceful-skip posture).
    pub fn new_spirv_diffexec_witness() -> Self {
        let mut inner = MultiEmitterBackend::new_with_specialist(
            Target::Spirv,
            Box::new(SpirvSaxpyGeneralEmitter),
            Box::new(SpirvSaxpySpecialistEmitter),
            QuorumPolicy::DiffExec { tolerance: 1.0e-3 },
        );
        if vulkan_adapter_available() {
            inner = inner.with_diff_exec_engine(std::sync::Arc::new(SpirvDiffExecEngine::new()));
        }
        Self { inner }
    }
}

impl Backend for SpirvBackend {
    fn name(&self) -> &'static str {
        "spirv"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Spirv]
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        // SPIR-V accepts `None` hardware (defaulting to Vulkan 1.1) but
        // rejects non-Spirv HwProfiles.
        match &config.hardware {
            None | Some(HwProfile::Spirv { .. }) => {}
            _ => return Err(BackendError::MissingHardware(Target::Spirv)),
        }
        self.inner.lower(module, config)
    }
}

// ─── Emitters ──────────────────────────────────────────────────────────

/// Hardware-shape guard shared by every SPIR-V emitter: accept `None`
/// (default Vulkan 1.1) or `HwProfile::Spirv`, reject anything else.
fn check_hardware(config: &BackendConfig) -> Option<Result<(), BackendError>> {
    match &config.hardware {
        None | Some(HwProfile::Spirv { .. }) => Some(Ok(())),
        _ => Some(Err(BackendError::MissingHardware(Target::Spirv))),
    }
}

/// Emit a SPIR-V artifact from a WGSL source string by compiling it via
/// naga and packaging the text summary + binary-word sidecar.
fn emit_from_wgsl(wgsl: &str) -> Result<EmittedText, BackendError> {
    let words = wgsl_to_spirv_words(wgsl)
        .map_err(|e| BackendError::Lower(format!("WGSL->SPIR-V compile: {e}")))?;
    validate_spirv(&words)
        .map_err(|e| BackendError::Lower(format!("emitted SPIR-V failed validation: {e}")))?;
    Ok(EmittedText {
        primary: spirv_text_summary(&words, wgsl),
        citations: vec![ContractId::new(CONTRACT_ID)],
    })
}

/// General SPIR-V emitter — PMAT-977: drives xpile's REAL emission
/// (`meta-HIR → xpile_wgsl_codegen::emit_wgsl_module → @compute harness`),
/// then compiles the real WGSL to SPIR-V via naga. The `2.0*x + 1.0`
/// arithmetic is xpile's output, not a hardcoded shader.
struct SpirvSaxpyGeneralEmitter;

impl TargetEmitter for SpirvSaxpyGeneralEmitter {
    fn name(&self) -> &str {
        "spirv-saxpy-general"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if let Some(Err(e)) = check_hardware(config) {
            return Some(Err(e));
        }
        // REAL path: xpile lowers the general meta-HIR module to WGSL, then
        // we compile that to SPIR-V. A real-emit failure is a hard refusal.
        let wgsl = match spirv_diffexec::general_real_wgsl() {
            Ok(w) => w,
            Err(e) => {
                return Some(Err(BackendError::Lower(format!(
                    "xpile real WGSL emission (general saxpy) failed: {e}"
                ))))
            }
        };
        Some(emit_from_wgsl(&wgsl))
    }
}

/// Specialist SPIR-V emitter — reuses the WGSL `fma(2.0, inp[i], 1.0)`
/// compute shader (same semantics, categorically different SPIR-V).
struct SpirvSaxpySpecialistEmitter;

impl TargetEmitter for SpirvSaxpySpecialistEmitter {
    fn name(&self) -> &str {
        "spirv-saxpy-specialist-fma"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if let Some(Err(e)) = check_hardware(config) {
            return Some(Err(e));
        }
        Some(emit_from_wgsl(spirv_diffexec::SPECIALIST_WGSL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_backend::{Profile, QuorumStatus};
    use xpile_meta_hir::SourceLang;

    fn dummy_module() -> Module {
        Module {
            name: "spirv_kernel".into(),
            source_lang: SourceLang::Rust,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        }
    }

    fn spirv_config() -> BackendConfig {
        BackendConfig {
            target: Target::Spirv,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Spirv { version: (1, 3) }),
        }
    }

    #[test]
    fn general_real_wgsl_compiles_to_real_spirv() {
        // PMAT-977: the general WGSL is now xpile's REAL emission
        // (meta-HIR → emit_wgsl_module → @compute harness), and it must
        // compile to real SPIR-V via naga.
        let wgsl = spirv_diffexec::general_real_wgsl().expect("real xpile WGSL emit");
        let words =
            wgsl_to_spirv_words(&wgsl).expect("real xpile general WGSL must compile to SPIR-V");
        assert!(spirv_looks_real(&words), "first word must be SPIR-V magic");
        assert_eq!(validate_spirv(&words), Ok(()));
    }

    #[test]
    fn specialist_wgsl_compiles_to_real_spirv() {
        // The specialist stays the hardcoded `fma` trusted reference.
        let words = wgsl_to_spirv_words(spirv_diffexec::SPECIALIST_WGSL)
            .expect("reference specialist WGSL must compile to SPIR-V");
        assert!(spirv_looks_real(&words));
        assert_eq!(validate_spirv(&words), Ok(()));
    }

    #[test]
    fn validate_spirv_rejects_non_spirv() {
        assert!(validate_spirv(&[0xdead_beef, 1, 2, 3, 4]).is_err());
        assert!(validate_spirv(&[]).is_err());
    }

    #[test]
    fn text_summary_carries_magic_and_source() {
        let wgsl = spirv_diffexec::general_real_wgsl().expect("real xpile WGSL emit");
        let words = wgsl_to_spirv_words(&wgsl).unwrap();
        let text = spirv_text_summary(&words, &wgsl);
        assert!(text.contains("SPIR-V"));
        assert!(text.contains(&format!("0x{SPIRV_MAGIC:08x}")));
        assert!(text.contains("@compute"));
    }

    #[test]
    fn backend_emits_real_spirv_through_multi_emitter() {
        let backend = SpirvBackend::new();
        let artifact = backend.lower(&dummy_module(), &spirv_config()).unwrap();
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "spirv-saxpy-general".to_string()
            }
        );
        assert!(artifact.primary.contains("SPIR-V"));
        assert!(artifact.citations.iter().any(|c| c.as_str() == CONTRACT_ID));
    }

    #[test]
    fn backend_accepts_no_hardware() {
        let backend = SpirvBackend::new();
        let cfg = BackendConfig {
            target: Target::Spirv,
            profile: Profile::RustOut,
            hardware: None,
        };
        let artifact = backend.lower(&dummy_module(), &cfg).unwrap();
        assert!(artifact.primary.contains("SPIR-V"));
    }

    #[test]
    fn backend_rejects_wrong_hardware() {
        let backend = SpirvBackend::new();
        let cfg = BackendConfig {
            target: Target::Spirv,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Ptx {
                compute_capability: "sm_80".to_string(),
            }),
        };
        let err = backend.lower(&dummy_module(), &cfg).unwrap_err();
        assert!(matches!(err, BackendError::MissingHardware(Target::Spirv)));
    }

    #[test]
    fn backend_targets_only_spirv() {
        let backend = SpirvBackend::new();
        assert_eq!(backend.targets(), &[Target::Spirv]);
        assert_eq!(backend.name(), "spirv");
    }
}
