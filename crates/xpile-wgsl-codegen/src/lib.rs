//! WGSL backend.
//!
//! Lowers Rust meta-HIR to WebGPU Shading Language. Validation via
//! `naga`. Layer 5 compile contracts live under
//! `contracts/compile-rust-to-wgsl-*.yaml` (to author).
//!
//! **Architecture (PMAT-265 / Section 29):** [`WgslBackend`] wraps a
//! [`MultiEmitterBackend`] (same pattern as `xpile_ptx_codegen::PtxBackend`)
//! so emission routes through the general/specialist quorum framework.
//! The general slot holds `RealWgslEmitter` — the production emitter that
//! drives xpile's REAL meta-HIR → WGSL lowering ([`emit_wgsl_module`],
//! PMAT-970) so `xpile transpile --target wgsl` produces actual WGSL, with
//! an honest [`BackendError::Lower`] refusal for any construct outside the
//! scalar/control + storage-buffer subset (never a scaffold comment, never
//! a silent wrong emit). An aprender `aprender-webgpu` could slot into
//! `specialist`.
//!
//! **PMAT-987:** before this slice the general slot held a
//! `ScaffoldWgslEmitter` whose body was `// TODO: lower to WGSL.`, so the
//! production path emitted a placeholder comment even though the real
//! [`emit_wgsl_module`] lowering already existed — only tests called it.
//! This slice wires the real lowering into production.

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, HwProfile, MultiEmitterBackend,
    QuorumPolicy, Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::Module;

mod wgpu_diffexec;
pub use wgpu_diffexec::{
    gpu_probe_env_usable, kernel_module, real_emitted_compute_wgsl, vulkan_adapter_available,
    vulkan_loader_guard, wgpu_adapter_available, WgpuWgslDiffExecEngine, FIXTURE_INPUT,
};

mod wgsl_emit;
pub use wgsl_emit::{emit_wgsl_module, naga_validate_wgsl, NagaValidationError};

/// WGSL backend — `Backend` impl wrapping a [`MultiEmitterBackend`] whose
/// general slot is the production `RealWgslEmitter` (drives the real
/// [`emit_wgsl_module`] lowering); routes through the same quorum framework
/// a future specialist would slot into.
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
            inner: MultiEmitterBackend::new_single(Target::Wgsl, Box::new(RealWgslEmitter)),
        }
    }

    /// PMAT-950 — the executed cross-vendor GPU-witness constructor (§29).
    ///
    /// Sibling of `xpile_ptx_codegen::PtxBackend::new_cuda_diffexec_witness`.
    /// Builds a `WgslBackend` whose `MultiEmitterBackend` carries two REAL
    /// WGSL compute-shader emitters — `WgslRealEmitGeneralEmitter`
    /// (general) and `WgslSaxpySpecialistEmitter` (specialist) — under
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
        self.inner
            .lower(module, config)
            .map(|a| a.with_citations(config.emit_contracts))
    }
}

/// Production WGSL emitter — drives xpile's REAL meta-HIR → WGSL lowering
/// (PMAT-970, [`emit_wgsl_module`]) for the scalar/control + storage-buffer
/// subset. This is the general slot of the production [`WgslBackend`], so
/// `xpile transpile --target wgsl` produces actual WGSL.
///
/// **PMAT-987:** replaces the old `ScaffoldWgslEmitter`, whose body was a
/// `// TODO: lower to WGSL.` placeholder comment. A construct outside the
/// supported subset (str/dict/f64/struct/enum/non-function item/…) is an
/// honest [`BackendError::Lower`] refusal surfaced from `emit_wgsl_module`
/// — never a scaffold comment and never a silently-wrong emit.
struct RealWgslEmitter;

impl TargetEmitter for RealWgslEmitter {
    fn name(&self) -> &str {
        "xpile-wgsl-codegen"
    }

    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        // Hardware shape: WGSL accepts `None` or a `Wgsl` profile; anything
        // else is a configuration fault (the wrapper's `Backend::lower`
        // already screens this, but the emitter stays self-consistent).
        match &config.hardware {
            None | Some(HwProfile::Wgsl { .. }) => {}
            _ => return Some(Err(BackendError::MissingHardware(Target::Wgsl))),
        }
        // Drive the REAL meta-HIR → WGSL lowering. A construct outside the
        // subset comes back as a hard `BackendError::Lower` — propagate it
        // verbatim rather than degrading to a placeholder.
        match emit_wgsl_module(module) {
            Ok(primary) => Some(Ok(EmittedText {
                primary,
                citations: vec![ContractId::new("C-COMPILE-RUST-TO-WGSL")],
            })),
            Err(e) => Some(Err(e)),
        }
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
    use xpile_meta_hir::{Block, Expr, Function, Item, Param, SourceLang, Type};

    /// An empty module — `emit_wgsl_module` honestly refuses it (no fn to
    /// lower / no entry point). Used to exercise the refusal path.
    fn empty_module() -> Module {
        Module {
            name: "test_kernel".into(),
            source_lang: SourceLang::Rust,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        }
    }

    /// A real in-subset scalar module: `fn add(a, b) -> i32 { return a + b; }`.
    /// Lowers through the REAL production emitter.
    fn scalar_module() -> Module {
        Module {
            name: "test_kernel".into(),
            source_lang: SourceLang::Rust,
            items: vec![Item::Function(Function {
                name: "add".into(),
                params: vec![
                    Param {
                        name: "a".into(),
                        ty: Type::I64,
                        mutable: false,
                    },
                    Param {
                        name: "b".into(),
                        ty: Type::I64,
                        mutable: false,
                    },
                ],
                return_type: Type::I64,
                body: Block {
                    stmts: Vec::new(),
                    trailing_return: Expr::BinOp {
                        op: xpile_meta_hir::BinOp::Add,
                        lhs: Box::new(Expr::Ident("a".into())),
                        rhs: Box::new(Expr::Ident("b".into())),
                    },
                },
            })],
            ffi_boundaries: Vec::new(),
        }
    }

    fn wgsl_config(features: Vec<String>) -> BackendConfig {
        BackendConfig {
            emit_contracts: true,
            target: Target::Wgsl,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Wgsl { features }),
        }
    }

    /// PMAT-987 REGRESSION GUARD — the production `WgslBackend::lower` must
    /// drive the REAL `emit_wgsl_module` lowering, NOT the old scaffold.
    ///
    /// This test FAILS on the pre-PMAT-987 `ScaffoldWgslEmitter` (whose
    /// output was `// TODO: lower to WGSL.` with no `fn`/`@`/arithmetic) and
    /// passes on the wired real emitter. It locks in that
    /// `xpile transpile --target wgsl` over a scalar module emits actual
    /// WGSL containing the lowered function body — the load-bearing
    /// behaviour the adversarial finding caught.
    #[test]
    fn wgsl_backend_lowers_real_wgsl_not_scaffold() {
        let backend = WgslBackend::new();
        let wgsl = backend
            .lower(&scalar_module(), &wgsl_config(vec![]))
            .expect("a scalar module lowers")
            .primary;
        // Positive: the REAL lowered function body is present.
        assert!(
            wgsl.contains("fn add(a: i32, b: i32) -> i32"),
            "production WGSL must contain the lowered fn signature:\n{wgsl}"
        );
        assert!(
            wgsl.contains("return (a + b);"),
            "production WGSL must contain the lowered arithmetic:\n{wgsl}"
        );
        // Negative: the scaffold placeholder strings must be GONE.
        assert!(
            !wgsl.contains("TODO: lower to WGSL"),
            "production WGSL must not be the scaffold placeholder:\n{wgsl}"
        );
        assert!(
            !wgsl.contains("scaffold"),
            "production WGSL must not be the scaffold placeholder:\n{wgsl}"
        );
        // It passes the structural well-formedness gate's `fn` check and
        // classifies as real (not the comment placeholder).
        assert!(wgsl_looks_real(&wgsl), "{wgsl}");
        // And it parses + type-checks under the CPU-only naga front-end.
        naga_validate_wgsl(&wgsl)
            .unwrap_or_else(|e| panic!("production-emitted WGSL must naga-validate: {e}\n{wgsl}"));
    }

    #[test]
    fn wgsl_backend_emits_through_multi_emitter() {
        let backend = WgslBackend::new();
        let artifact = backend
            .lower(&scalar_module(), &wgsl_config(vec!["f16".into()]))
            .unwrap();
        // Quorum status comes from the wrapped MultiEmitterBackend — the
        // production emitter name (no longer the scaffold) is propagated.
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "xpile-wgsl-codegen".to_string()
            }
        );
        // The source module name rides in the emitted header comment.
        assert!(artifact.primary.contains("test_kernel"));
    }

    #[test]
    fn wgsl_backend_accepts_no_hardware() {
        // WGSL allows None hardware — defaults to empty feature list and
        // still drives the real lowering.
        let backend = WgslBackend::new();
        let cfg = BackendConfig {
            emit_contracts: true,
            target: Target::Wgsl,
            profile: Profile::RustOut,
            hardware: None,
        };
        let artifact = backend.lower(&scalar_module(), &cfg).unwrap();
        assert!(artifact.primary.contains("fn add(a: i32, b: i32) -> i32"));
    }

    #[test]
    fn wgsl_backend_refuses_unsupported_construct() {
        // An empty module has no function to lower — the real emitter
        // refuses it with a hard `BackendError::Lower` (NOT a scaffold
        // comment, NOT a silent wrong emit).
        let backend = WgslBackend::new();
        let err = backend
            .lower(&empty_module(), &wgsl_config(vec![]))
            .unwrap_err();
        assert!(
            matches!(err, BackendError::Lower(_)),
            "unsupported input must be an honest Lower refusal, got {err:?}"
        );
    }

    #[test]
    fn wgsl_backend_rejects_wrong_hardware() {
        let backend = WgslBackend::new();
        let cfg = BackendConfig {
            emit_contracts: true,
            target: Target::Wgsl,
            profile: Profile::RustOut,
            hardware: Some(HwProfile::Ptx {
                compute_capability: "sm_80".to_string(),
            }),
        };
        let err = backend.lower(&scalar_module(), &cfg).unwrap_err();
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
        // The OLD scaffold-comment placeholder (now never emitted by the
        // production path) must still classify as NOT real — this is the
        // gate that catches a regression back to a comment-only emit.
        let scaffold_placeholder = "// xpile-wgsl-codegen scaffold\n// TODO: lower to WGSL.\n";
        assert!(!wgsl_looks_real(scaffold_placeholder));
    }

    #[test]
    fn validate_wgsl_rejects_scaffold_placeholder() {
        // The structural gate rejects a comment-only placeholder (no
        // `@compute` entry) — the property that flagged the original bug.
        let scaffold_placeholder = "// xpile-wgsl-codegen scaffold\n// TODO: lower to WGSL.\n";
        assert_eq!(
            validate_wgsl(scaffold_placeholder),
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
