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
    QuorumPolicy, Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

mod wgpu_diffexec;
pub use wgpu_diffexec::{
    kernel_module, real_emitted_compute_wgsl, vulkan_adapter_available, wgpu_adapter_available,
    WgpuWgslDiffExecEngine, FIXTURE_INPUT,
};

mod wgsl_emit;
pub use wgsl_emit::{emit_wgsl_module, naga_validate_wgsl, NagaValidationError};

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

    /// PMAT-950 — the executed cross-vendor GPU-witness constructor (§29).
    ///
    /// Sibling of [`xpile_ptx_codegen::PtxBackend::new_cuda_diffexec_witness`].
    /// Builds a `WgslBackend` whose `MultiEmitterBackend` carries two REAL
    /// WGSL compute-shader emitters — [`WgslRealEmitGeneralEmitter`]
    /// (general) and [`WgslSaxpySpecialistEmitter`] (specialist) — under
    /// `QuorumPolicy::DiffExec`, with a [`WgpuWgslDiffExecEngine`]
    /// installed. Both emitters compute the same semantics
    /// (`out[i] = 2*in[i] + 1`); the GENERAL side is produced by lowering a
    /// meta-HIR module through xpile's REAL [`emit_wgsl_module`]
    /// (PMAT-970/975) and the SPECIALIST is the trusted `fma` builtin
    /// reference. So the `DiffExec` quorum proves the chain
    /// `meta-HIR → emit_wgsl_module → run → correct` on a real
    /// Vulkan/Metal/DX12 adapter — not `hardcoded shader → run`.
    ///
    /// On a host with a wgpu adapter this produces a real
    /// [`xpile_backend::DiffExecResult::Match`] instead of the
    /// `NotRun { no-engine }` placeholder — closing the WGSL §29 lane's
    /// long-standing "on-hardware Vulkan `DiffExec`" caveat (PMAT-490).
    ///
    /// On a host with no adapter the engine is NOT installed (the
    /// `MultiEmitterBackend` keeps `diff_exec_engine = None`), so the
    /// backend records the benign `NotRun { no-engine }` and free CI
    /// stays green — the `nvcc`/cc/python3 graceful-skip posture.
    pub fn new_wgpu_diffexec_witness() -> Self {
        let mut inner = MultiEmitterBackend::new_with_specialist(
            Target::Wgsl,
            Box::new(WgslRealEmitGeneralEmitter),
            Box::new(WgslSaxpySpecialistEmitter),
            QuorumPolicy::DiffExec { tolerance: 1.0e-3 },
        );
        if wgpu_adapter_available() {
            inner = inner.with_diff_exec_engine(std::sync::Arc::new(WgpuWgslDiffExecEngine::new()));
        }
        Self { inner }
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

// ─── PMAT-950/975: real WGSL compute-shader emitters for the executed witness ──
//
// Two emitters that produce COMPLETE, naga-validatable WGSL compute
// shaders computing identical semantics — `out[i] = 2*in[i] + 1`.
//
// PMAT-975 rewires the GENERAL emitter to drive xpile's REAL
// `emit_wgsl_module` lowering (PMAT-970): it builds a small meta-HIR
// module for `saxpy(x) = 2.0*x + 1.0`, lowers it through the production
// emitter, and wraps the emitted `fn saxpy` in the harness `@compute`
// entry point. So the load-bearing arithmetic the GPU runs is the bytes
// xpile EMITTED — the witness now proves `meta-HIR → emit_wgsl_module →
// run → correct`, not `hardcoded shader → run`. The specialist stays the
// trusted independent `fma` builtin reference.
//
// Both are run on a real wgpu adapter under the `DiffExec` quorum (see
// [`WgslBackend::new_wgpu_diffexec_witness`]); the engine asserts the real
// emitted shader's executed output matches the CPython-equivalent vector
// AND agrees with the `fma` reference within tolerance.
//
// The shared harness contract (driven by [`WgpuWgslDiffExecEngine`]):
//   - `@compute @workgroup_size(64)` entry point named `main`,
//   - `@group(0) @binding(0) var<storage, read>       …: array<f32>` (in),
//   - `@group(0) @binding(1) var<storage, read_write>  …: array<f32>` (out).
// Both shaders satisfy [`validate_wgsl`] and are classified real by
// [`wgsl_looks_real`].

/// General WGSL emitter — produces the REAL emitted shader: it lowers a
/// meta-HIR `saxpy(x) = 2.0*x + 1.0` module through xpile's production
/// [`emit_wgsl_module`] and wraps the emitted `fn` in the harness
/// `@compute` entry point (see [`real_emitted_compute_wgsl`]). The GPU
/// therefore executes xpile's actual emission.
struct WgslRealEmitGeneralEmitter;

impl TargetEmitter for WgslRealEmitGeneralEmitter {
    fn name(&self) -> &str {
        "wgsl-real-emit-general"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        match &config.hardware {
            None | Some(HwProfile::Wgsl { .. }) => {}
            _ => return Some(Err(BackendError::MissingHardware(Target::Wgsl))),
        }
        // Drive xpile's REAL meta-HIR → WGSL lowering (PMAT-970/975). A
        // lowering failure is a hard backend error — the emitter must not
        // silently fall back to a hardcoded shader.
        let primary = match real_emitted_compute_wgsl() {
            Ok(wgsl) => wgsl,
            Err(e) => {
                return Some(Err(BackendError::Lower(format!(
                    "xpile real WGSL emission for the witness failed: {e}"
                ))))
            }
        };
        Some(Ok(EmittedText {
            primary,
            citations: vec![ContractId::new("C-COMPILE-RUST-TO-WGSL")],
        }))
    }
}

/// Specialist WGSL emitter — same semantics (`out[i] = 2*in[i] + 1`)
/// computed via the `fma` builtin. A categorically independent
/// implementation: the `DiffExec` quorum runs both on the GPU and
/// falsifies the contract if they diverge.
struct WgslSaxpySpecialistEmitter;

impl TargetEmitter for WgslSaxpySpecialistEmitter {
    fn name(&self) -> &str {
        "wgsl-saxpy-specialist-fma"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        match &config.hardware {
            None | Some(HwProfile::Wgsl { .. }) => {}
            _ => return Some(Err(BackendError::MissingHardware(Target::Wgsl))),
        }
        Some(Ok(EmittedText {
            primary: "\
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> outp: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&inp)) {
        // specialist path: fused multiply-add builtin
        outp[i] = fma(2.0, inp[i], 1.0);
    }
}
"
            .to_string(),
            citations: vec![ContractId::new("C-COMPILE-RUST-TO-WGSL")],
        }))
    }
}

// ─── PMAT-482: offline WGSL well-formedness gate (§30 Track 4) ───────
//
// Mirrors the PMAT-481 PTX gate for the WGSL/SPIR-V lane: a structural,
// CPU-only check on emitted WGSL text. The deeper `naga` validation +
// `spirv-val` CI step (also CPU, no GPU) wires in alongside the real
// WGSL emitter — exactly as the `ptxas`-assembles step does for PTX in
// PMAT-485. Gate on [`wgsl_looks_real`] so the v0.1.0 scaffold comment
// placeholder is never treated as real emission. This is a structural
// gate, not the model→emission gate (that is the on-hardware AMD-Vulkan
// `DiffExec` slice, PMAT-490).

/// Reasons emitted WGSL text fails the [`validate_wgsl`] well-formedness gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WgslValidationError {
    /// No `@compute` entry attribute (e.g. the scaffold placeholder).
    MissingComputeEntry,
    /// No `@workgroup_size(...)` — required on a compute entry point.
    MissingWorkgroupSize,
    /// No `fn` declaration (the entry-point body).
    MissingFn,
}

impl std::fmt::Display for WgslValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingComputeEntry => write!(f, "WGSL has no `@compute` entry attribute"),
            Self::MissingWorkgroupSize => {
                write!(f, "WGSL compute entry is missing `@workgroup_size(...)`")
            }
            Self::MissingFn => write!(f, "WGSL has no `fn` entry-point declaration"),
        }
    }
}

impl std::error::Error for WgslValidationError {}

/// `true` when `text` looks like real WGSL (carries a `@compute`
/// attribute or an `fn` declaration) rather than the v0.1.0 scaffold
/// comment placeholder.
pub fn wgsl_looks_real(text: &str) -> bool {
    noncomment_lines(text).any(|l| l.contains("@compute") || l.contains("fn "))
}

/// PMAT-482 — structural well-formedness check on emitted WGSL text: a
/// `@compute` entry, a `@workgroup_size(...)`, and an `fn` declaration.
/// Pure text — no GPU. Gate on [`wgsl_looks_real`] first so the scaffold
/// placeholder is not treated as real emission.
pub fn validate_wgsl(text: &str) -> Result<(), WgslValidationError> {
    let has = |needle: &str| noncomment_lines(text).any(|l| l.contains(needle));
    if !has("@compute") {
        return Err(WgslValidationError::MissingComputeEntry);
    }
    if !has("@workgroup_size(") {
        return Err(WgslValidationError::MissingWorkgroupSize);
    }
    if !has("fn ") {
        return Err(WgslValidationError::MissingFn);
    }
    Ok(())
}

/// Non-empty, non-`//`-comment lines, trimmed.
fn noncomment_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.is_empty())
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

    // ─── PMAT-482: offline WGSL well-formedness gate ────────────────

    /// A minimal but real WGSL compute shader — the shape a real WGSL
    /// emitter will produce (PMAT-490 territory).
    const GOLDEN_WGSL: &str = "\
// generated
@compute @workgroup_size(64)
fn add_one(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
}
";

    #[test]
    fn validate_wgsl_accepts_well_formed_compute_shader() {
        assert_eq!(validate_wgsl(GOLDEN_WGSL), Ok(()));
    }

    #[test]
    fn wgsl_looks_real_classifies_golden_vs_scaffold() {
        assert!(wgsl_looks_real(GOLDEN_WGSL));
        let scaffold = WgslBackend::new()
            .lower(&dummy_module(), &wgsl_config(vec![]))
            .unwrap()
            .primary;
        assert!(!wgsl_looks_real(&scaffold));
    }

    #[test]
    fn validate_wgsl_rejects_scaffold_placeholder() {
        let scaffold = WgslBackend::new()
            .lower(&dummy_module(), &wgsl_config(vec![]))
            .unwrap()
            .primary;
        assert_eq!(
            validate_wgsl(&scaffold),
            Err(WgslValidationError::MissingComputeEntry)
        );
    }

    #[test]
    fn validate_wgsl_requires_workgroup_size_and_fn() {
        let no_wg = "@compute\nfn k() {}\n";
        assert_eq!(
            validate_wgsl(no_wg),
            Err(WgslValidationError::MissingWorkgroupSize)
        );
        let no_fn = "@compute @workgroup_size(1)\n";
        assert_eq!(validate_wgsl(no_fn), Err(WgslValidationError::MissingFn));
    }
}
