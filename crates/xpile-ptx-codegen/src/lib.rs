//! PTX backend.
//!
//! Lowers Rust meta-HIR (the element-wise scalar kernel shape) to NVIDIA PTX
//! text targeting `sm_80`+. Layer 5 compile contract:
//! `contracts/compile-rust-to-ptx-mma-v1.yaml`.
//!
//! **Real emitter (PMAT-961):** [`XpilePtxEmitter`] is a genuine
//! meta-HIR → PTX text lowering ([`emit::emit_kernel`]) — the NVIDIA sibling of
//! `xpile-wasm-codegen`'s hand-emitted WAT. It emits a complete
//! `ptxas`-assemblable `.visible .entry xpile_kernel` module (`.version` /
//! `.target sm_<cc>` derived from [`HwProfile::Ptx`] / `.address_size 64`,
//! `ld.global` → scalar arithmetic → `st.global`) for the scalar element-wise
//! subset, and REFUSES aggregates / control flow Lean-style. It replaces the
//! v0.1.0 `ScaffoldPtxEmitter` comment placeholder (retired).
//!
//! **Architecture (PMAT-264 / Section 29):** [`PtxBackend`] wraps a
//! [`MultiEmitterBackend`] so emission routes through the same
//! general/specialist quorum framework. The real [`XpilePtxEmitter`] is the
//! `general` slot; `aprender-gpu` would slot into the `specialist` position;
//! no changes to [`PtxBackend`]'s public API.
//!
//! **§29 anti-correlation witness (PMAT-961):** [`PtxBackend::new_ptx_diffexec_witness`]
//! installs the [`PtxDiffExecEngine`], which diffs xpile's OWN hand-emitted PTX
//! (run via the CUDA Driver API) against the nvcc-compiled CUDA-C `xpile_kernel`
//! — two **categorically-independent codegen toolchains** for the same kernel,
//! upgrading PMAT-949's two-CUDA-C-kernels-same-nvcc check to a genuinely
//! independent pair. cuda-oxide (PMAT-480) becomes the 3rd independent emitter
//! on top of this pair (its own nightly slice).

use xpile_backend::{
    Artifact, Backend, BackendConfig, BackendError, EmittedText, HwProfile, MultiEmitterBackend,
    QuorumPolicy, Target, TargetEmitter,
};
use xpile_contracts::ContractId;
use xpile_meta_hir::{BinOp, Block, Expr, FloatOp, Function, Item, Module, Param, Stmt, Type};

mod cuda_diffexec;
mod emit;
mod ptx_diffexec;
mod rustc_nvptx;
pub use cuda_diffexec::{cuda_toolchain_available, NvccCudaDiffExecEngine, FIXTURE_INPUT};
pub use emit::{emit_kernel, ptx_version_for, KERNEL_NAME, PTX_VERSION};
pub use ptx_diffexec::PtxDiffExecEngine;
// PMAT-997: the 3rd categorically-independent §29 PTX emitter — nightly rustc's
// nvptx64-nvidia-cuda target (LLVM NVPTX back-end). External subprocess, gated.
pub use rustc_nvptx::{
    emit_rustc_nvptx_ptx, rustc_nvptx_available, NVPTX_TARGET, RUSTC_NVPTX_KERNEL_SRC,
};

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
            inner: MultiEmitterBackend::new_single(Target::Ptx, Box::new(XpilePtxEmitter)),
        }
    }

    /// PMAT-280 — End-to-end validation constructor for Section 29's
    /// multi-emitter routing.
    ///
    /// Builds a `PtxBackend` whose `MultiEmitterBackend` carries the real
    /// [`XpilePtxEmitter`] in the `general` slot AND a
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
                Box::new(XpilePtxEmitter),
                Box::new(MatmulSpecialistEmitter),
                QuorumPolicy::PreferSpecialist,
            ),
        }
    }

    /// PMAT-961 — the TRUE anti-correlation §29 PTX witness constructor.
    ///
    /// Builds a `PtxBackend` whose `MultiEmitterBackend` carries TWO
    /// categorically-independent emitters for the same `out[i] = 2*in[i] + 1`
    /// kernel — [`XpileSaxpyPtxEmitter`] (general: xpile's OWN hand-emitted
    /// PTX) and [`CudaSaxpyGeneralEmitter`] (specialist: nvcc-compilable
    /// CUDA-C) — under `QuorumPolicy::DiffExec`, with a [`PtxDiffExecEngine`]
    /// installed. The engine runs the xpile PTX via the CUDA Driver API and the
    /// nvcc CUDA-C via the Runtime API, both on the GPU, and asserts the
    /// executed outputs agree.
    ///
    /// This is the categorical-independence upgrade of
    /// [`PtxBackend::new_cuda_diffexec_witness`] (PMAT-949): that diffed two
    /// CUDA-C kernels compiled by the SAME nvcc; this diffs two DIFFERENT
    /// codegen toolchains (xpile hand-emit vs nvcc). On a non-GPU host the
    /// engine is NOT installed → the benign `NotRun { no-engine }`, free CI
    /// stays green.
    pub fn new_ptx_diffexec_witness() -> Self {
        let mut inner = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(XpileSaxpyPtxEmitter),
            Box::new(CudaSaxpyGeneralEmitter),
            QuorumPolicy::DiffExec { tolerance: 1.0e-3 },
        );
        if cuda_toolchain_available() {
            inner = inner.with_diff_exec_engine(std::sync::Arc::new(PtxDiffExecEngine::new()));
        }
        Self { inner }
    }

    /// PMAT-962 — the anti-correlation §29 PTX witness for a NEW construct:
    /// **control flow** (`if`/`else` + comparison), not just straight-line
    /// arithmetic.
    ///
    /// The same categorical-independence design as
    /// [`PtxBackend::new_ptx_diffexec_witness`] but over the relu kernel
    /// `out[i] = (in[i] > 0) ? in[i] : 0`:
    ///   - general: [`XpileReluPtxEmitter`] — xpile's OWN hand-emitted PTX, with
    ///     a real `setp.gt.f64` + `@!%p bra` branch + a shared result register
    ///     (the phi-via-register idiom).
    ///   - specialist: [`CudaReluGeneralEmitter`] — nvcc-compiled CUDA-C using a
    ///     C `?:` ternary.
    ///
    /// Two codegen toolchains with NO shared frontend that must agree on the
    /// branchy kernel — exercising the PMAT-962 control-flow lowering end-to-end
    /// on real silicon. Relu over [`FIXTURE_INPUT`] is exactly representable so
    /// the executed outputs agree bit-for-bit. Graceful-skip off-GPU (no engine
    /// installed → benign `NotRun`, free CI green).
    pub fn new_ptx_if_diffexec_witness() -> Self {
        let mut inner = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(XpileReluPtxEmitter),
            Box::new(CudaReluGeneralEmitter),
            QuorumPolicy::DiffExec { tolerance: 1.0e-3 },
        );
        if cuda_toolchain_available() {
            inner = inner.with_diff_exec_engine(std::sync::Arc::new(PtxDiffExecEngine::new()));
        }
        Self { inner }
    }

    /// PMAT-949 — the executed GPU-witness constructor (§29).
    ///
    /// Builds a `PtxBackend` whose `MultiEmitterBackend` carries two
    /// REAL CUDA-C kernel emitters — [`CudaSaxpyGeneralEmitter`]
    /// (general) and [`CudaSaxpySpecialistEmitter`] (specialist) — under
    /// `QuorumPolicy::DiffExec`, with a [`NvccCudaDiffExecEngine`]
    /// installed. Both emitters compute the same semantics
    /// (`out[i] = 2*in[i] + 1`) via *categorically different* CUDA-C
    /// implementations (one `fmaf`, one explicit `mul`+`add`), so the
    /// `DiffExec` quorum runs BOTH on the GPU and asserts they agree —
    /// the falsifying multi-emitter check the §29 design specs.
    ///
    /// This is the constructor that, when run on a CUDA box, produces a
    /// real [`xpile_backend::DiffExecResult::Match`] instead of the
    /// `NotRun { no-engine }` placeholder — closing the audit-design.md
    /// §4 / §62 "Run=1 / DiffExecResult::NotRun" caveat for
    /// `C-COMPILE-RUST-TO-PTX-MMA`.
    ///
    /// On a host without `nvcc` + `nvidia-smi` the engine is NOT
    /// installed (the `MultiEmitterBackend` keeps `diff_exec_engine =
    /// None`), so the backend records the benign `NotRun { no-engine }`
    /// and free CI stays green — the cc/python3 graceful-skip posture.
    pub fn new_cuda_diffexec_witness() -> Self {
        let mut inner = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(CudaSaxpyGeneralEmitter),
            Box::new(CudaSaxpySpecialistEmitter),
            QuorumPolicy::DiffExec { tolerance: 1.0e-3 },
        );
        if cuda_toolchain_available() {
            inner = inner.with_diff_exec_engine(std::sync::Arc::new(NvccCudaDiffExecEngine::new()));
        }
        Self { inner }
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

/// PMAT-961 — the REAL meta-HIR → PTX emitter (general slot).
///
/// Lowers the module's single kernel [`Function`] of the element-wise scalar
/// shape to a complete `ptxas`-assemblable PTX module via
/// [`emit::emit_kernel`]. Retires the v0.1.0 `ScaffoldPtxEmitter` comment
/// placeholder. Refuses (hard `BackendError`) any module that isn't a single
/// scalar element-wise function — never wrong PTX.
struct XpilePtxEmitter;

impl TargetEmitter for XpilePtxEmitter {
    fn name(&self) -> &str {
        "xpile-ptx-codegen"
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
        // The element-wise PTX subset emits exactly one kernel function. An
        // empty / multi-function / non-function module is refused honestly.
        let funcs: Vec<&Function> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Function(f) => Some(f),
                _ => None,
            })
            .collect();
        let f = match funcs.as_slice() {
            [single] => *single,
            [] => {
                return Some(Err(BackendError::Lower(
                    "xpile-ptx-codegen: module has no kernel function (the PTX element-wise \
                     subset emits exactly one scalar element-wise function)"
                        .to_string(),
                )))
            }
            _ => {
                return Some(Err(BackendError::Lower(
                    "xpile-ptx-codegen: module has multiple functions (the PTX element-wise \
                     subset emits exactly one kernel)"
                        .to_string(),
                )))
            }
        };
        match emit::emit_kernel(f, compute_capability) {
            Ok(primary) => Some(Ok(EmittedText {
                primary,
                citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
            })),
            Err(e) => Some(Err(e)),
        }
    }
}

/// PMAT-961 — the anti-correlation witness's *general* emitter: xpile's OWN
/// hand-emitted PTX for the fixed `out[i] = 2*in[i] + 1` saxpy kernel,
/// independent of the module's contents (the witness drives a fixed kernel so
/// the diff is reproducible). Computes `(x + x) + 1.0` over f64 — the exact
/// numeric semantics of the nvcc CUDA-C peer, via a categorically different
/// codegen path (xpile text vs nvcc C++).
struct XpileSaxpyPtxEmitter;

/// The nvcc-compilable CUDA-C `xpile_kernel` for the canonical
/// `out[i] = 2*in[i] + 1` saxpy kernel (the specialist half of the §29 PTX
/// anti-correlation pair). `pub` so the PMAT-963 cross-hardware witness pairs
/// it with xpile's hand-emitted PTX and runs both on the gx10 (GB10 / sm_121)
/// fleet host — the same categorically-independent pair as the local sm_89
/// arm, on a different architecture.
pub const SAXPY_CUDA_C_KERNEL: &str = "\
__global__ void xpile_kernel(const float* in, float* out, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        // general path: explicit multiply then add
        out[i] = 2.0f * in[i] + 1.0f;
    }
}
";

/// Build the canonical `out[i] = 2*in[i] + 1` f64 kernel function the witness
/// emitters share (so xpile PTX and nvcc CUDA-C attest the same semantics).
/// `pub` so the PMAT-963 cross-hardware witness emits xpile's hand-emitted PTX
/// for the gx10 (sm_121) target from the SAME meta-HIR the local sm_89 arm uses.
pub fn saxpy_kernel_fn() -> Function {
    let x_plus_x = Expr::FloatBinOp {
        op: FloatOp::Add,
        lhs: Box::new(Expr::Ident("x".into())),
        rhs: Box::new(Expr::Ident("x".into())),
    };
    Function {
        name: "xpile_kernel".into(),
        params: vec![Param {
            name: "x".into(),
            ty: Type::F64,
            mutable: false,
        }],
        return_type: Type::F64,
        body: Block {
            stmts: Vec::new(),
            trailing_return: Expr::FloatBinOp {
                op: FloatOp::Add,
                lhs: Box::new(x_plus_x),
                rhs: Box::new(Expr::LitFloat(1.0)),
            },
        },
    }
}

impl TargetEmitter for XpileSaxpyPtxEmitter {
    fn name(&self) -> &str {
        "xpile-ptx-hand-emitted"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        let compute_capability = match &config.hardware {
            Some(HwProfile::Ptx { compute_capability }) => compute_capability,
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        };
        match emit::emit_kernel(&saxpy_kernel_fn(), compute_capability) {
            Ok(primary) => Some(Ok(EmittedText {
                primary,
                citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
            })),
            Err(e) => Some(Err(e)),
        }
    }
}

/// PMAT-962 — the control-flow anti-correlation witness's *general* emitter:
/// xpile's OWN hand-emitted PTX for the relu kernel `out[i] = (in[i] > 0) ?
/// in[i] : 0` — exercising the new `if`/`else` + `setp.gt.f64` + `@!%p bra`
/// lowering, the categorical PTX twin of the nvcc CUDA-C `?:` peer.
struct XpileReluPtxEmitter;

/// Build the relu kernel `out[i] = (x > 0) ? x : 0` as meta-HIR — an `if`/`else`
/// over a comparison into a shared local, the shape the PMAT-962 control-flow
/// lowering compiles to `setp`/`@!%p bra`/labels.
fn relu_kernel_fn() -> Function {
    Function {
        name: "xpile_kernel".into(),
        params: vec![Param {
            name: "x".into(),
            ty: Type::F64,
            mutable: false,
        }],
        return_type: Type::F64,
        body: Block {
            stmts: vec![
                Stmt::Let {
                    name: "r".into(),
                    ty: Type::F64,
                    value: Expr::LitFloat(0.0),
                    mutable: true,
                },
                Stmt::If {
                    cond: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::LitFloat(0.0)),
                    },
                    then_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::Ident("x".into()),
                    }],
                    else_body: vec![Stmt::Assign {
                        name: "r".into(),
                        value: Expr::LitFloat(0.0),
                    }],
                },
            ],
            trailing_return: Expr::Ident("r".into()),
        },
    }
}

impl TargetEmitter for XpileReluPtxEmitter {
    fn name(&self) -> &str {
        "xpile-ptx-hand-emitted-if"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        let compute_capability = match &config.hardware {
            Some(HwProfile::Ptx { compute_capability }) => compute_capability,
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        };
        match emit::emit_kernel(&relu_kernel_fn(), compute_capability) {
            Ok(primary) => Some(Ok(EmittedText {
                primary,
                citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
            })),
            Err(e) => Some(Err(e)),
        }
    }
}

/// PMAT-962 — the control-flow witness's *specialist* emitter: a complete
/// nvcc-compilable CUDA-C `xpile_kernel` computing the SAME relu semantics
/// (`out[i] = in[i] > 0 ? in[i] : 0`) via a C `?:` ternary — a categorically
/// independent codegen path from xpile's hand-emitted branch PTX.
struct CudaReluGeneralEmitter;

impl TargetEmitter for CudaReluGeneralEmitter {
    fn name(&self) -> &str {
        "cuda-relu-general"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        match &config.hardware {
            Some(HwProfile::Ptx { .. }) => {}
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        }
        Some(Ok(EmittedText {
            primary: "\
__global__ void xpile_kernel(const float* in, float* out, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        // specialist path: a C ternary (independent of xpile's branch PTX)
        float x = in[i];
        out[i] = x > 0.0f ? x : 0.0f;
    }
}
"
            .to_string(),
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

// ─── PMAT-949: real CUDA-C kernel emitters for the executed GPU witness ──
//
// Two emitters that produce COMPLETE, nvcc-compilable CUDA-C
// `__global__ void xpile_kernel(const float* in, float* out, int n)`
// kernels computing identical semantics — `out[i] = 2*in[i] + 1` — via
// categorically different implementations. The general emitter uses an
// explicit `mul`+`add`; the specialist uses the fused-multiply-add
// intrinsic `fmaf`. Both are run on the GPU under the `DiffExec` quorum
// (see [`PtxBackend::new_cuda_diffexec_witness`]); the engine asserts
// their executed outputs agree within tolerance. The kernel name and
// signature are the harness contract used by [`NvccCudaDiffExecEngine`].

/// General CUDA-C emitter — `out[i] = 2.0f * in[i] + 1.0f` via an
/// explicit multiply-then-add. Emits a complete nvcc-compilable kernel.
struct CudaSaxpyGeneralEmitter;

impl TargetEmitter for CudaSaxpyGeneralEmitter {
    fn name(&self) -> &str {
        "cuda-saxpy-general"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        match &config.hardware {
            Some(HwProfile::Ptx { .. }) => {}
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        }
        Some(Ok(EmittedText {
            primary: SAXPY_CUDA_C_KERNEL.to_string(),
            citations: vec![ContractId::new("C-COMPILE-RUST-TO-PTX-MMA")],
        }))
    }
}

/// Specialist CUDA-C emitter — same semantics (`out[i] = 2*in[i] + 1`)
/// computed via the fused-multiply-add intrinsic `fmaf`. A
/// categorically independent implementation: the `DiffExec` quorum runs
/// both on the GPU and falsifies the contract if they diverge.
struct CudaSaxpySpecialistEmitter;

impl TargetEmitter for CudaSaxpySpecialistEmitter {
    fn name(&self) -> &str {
        "cuda-saxpy-specialist-fma"
    }

    fn try_emit(
        &self,
        _module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        match &config.hardware {
            Some(HwProfile::Ptx { .. }) => {}
            _ => return Some(Err(BackendError::MissingHardware(Target::Ptx))),
        }
        Some(Ok(EmittedText {
            primary: "\
__global__ void xpile_kernel(const float* in, float* out, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        // specialist path: fused multiply-add intrinsic
        out[i] = fmaf(2.0f, in[i], 1.0f);
    }
}
"
            .to_string(),
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
/// **derived, never hard-coded** (PMAT-481). The `ptxas` assemble step uses
/// this so the assembled arch always matches the emitted `.target`.
pub fn ptxas_arch(compute_capability: &str) -> String {
    format!("-arch={compute_capability}")
}

/// `true` when `ptxas` (the offline PTX assembler) is invocable — the gate for
/// the PMAT-961 offline assemble-validation test. Mirrors the
/// cc/wat2wasm/nvcc graceful-skip helpers: absence is a clean skip (free CI
/// has no CUDA toolkit), presence assembles the emitted PTX for real. This is
/// the PTX analog of `wat2wasm`-assembles-WAT / naga-validates-WGSL.
pub fn ptxas_available() -> bool {
    std::process::Command::new("ptxas")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// PMAT-961 — assemble `ptx_text` with the real `ptxas` for `compute_capability`,
/// returning `Ok(())` on a clean assemble or `Err(stderr)` on rejection. The
/// caller gates on [`ptxas_available`] (graceful-skip). This is the offline
/// validation step the WAT lane gets from `wat2wasm` and the WGSL lane from
/// naga — proof the hand-emitted PTX is well-formed for the NVIDIA assembler,
/// not just our own structural [`validate_ptx`] check.
pub fn ptxas_assemble(ptx_text: &str, compute_capability: &str) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    // Unique per call so parallel test threads in one process never collide on
    // the scratch `.ptx`/`.cubin` paths.
    let uniq = format!(
        "{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let dir = std::env::temp_dir().join(format!("xpile-ptxas-{uniq}"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create temp dir: {e}"))?;
    let ptx_path = dir.join("xpile_kernel.ptx");
    let out_path = dir.join("xpile_kernel.cubin");
    std::fs::write(&ptx_path, ptx_text).map_err(|e| format!("write ptx: {e}"))?;
    let out = std::process::Command::new("ptxas")
        .arg(ptxas_arch(compute_capability))
        .arg(&ptx_path)
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("spawn ptxas: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
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
    use xpile_meta_hir::{Block, Expr, FloatOp, Function, Item, Param, SourceLang, Type};

    /// A real element-wise kernel module: `def k(x: f64) -> f64: return
    /// (x + x) + 1.0` — the scalar saxpy shape the real PTX emitter lowers.
    fn dummy_module() -> Module {
        Module {
            name: "test_kernel".into(),
            source_lang: SourceLang::Rust,
            items: vec![Item::Function(kernel_fn("k"))],
            ffi_boundaries: Vec::new(),
        }
    }

    fn kernel_fn(name: &str) -> Function {
        let x_plus_x = Expr::FloatBinOp {
            op: FloatOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("x".into())),
        };
        Function {
            name: name.into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::F64,
                mutable: false,
            }],
            return_type: Type::F64,
            body: Block {
                stmts: Vec::new(),
                trailing_return: Expr::FloatBinOp {
                    op: FloatOp::Add,
                    lhs: Box::new(x_plus_x),
                    rhs: Box::new(Expr::LitFloat(1.0)),
                },
            },
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
    fn ptx_backend_emits_real_ptx_through_multi_emitter() {
        let backend = PtxBackend::new();
        let artifact = backend
            .lower(&dummy_module(), &ptx_config("sm_80"))
            .unwrap();
        // Quorum status comes from the wrapped MultiEmitterBackend; the real
        // emitter name is propagated.
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "xpile-ptx-codegen".to_string()
            }
        );
        // Real PTX, not a placeholder: directives + load/compute/store.
        assert!(artifact.primary.contains(".target sm_80"));
        assert!(artifact.primary.contains(".visible .entry xpile_kernel"));
        assert!(artifact.primary.contains("ld.global.f64"));
        assert!(artifact.primary.contains("st.global.f64"));
        assert!(ptx_looks_real(&artifact.primary));
        assert_eq!(validate_ptx(&artifact.primary, "sm_80"), Ok(()));
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
            items: vec![Item::Function(kernel_fn("k"))],
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

    /// PMAT-280 — Non-matmul modules fall back to the general (real PTX)
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
                emitter: "xpile-ptx-codegen".to_string()
            },
            "non-matching specialist should let general emit; QuorumStatus should reflect general"
        );
        assert!(
            artifact.primary.contains(".visible .entry xpile_kernel"),
            "primary should carry the general real-PTX emission body, got:\n{}",
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
    fn ptx_looks_real_classifies_golden_vs_comment_only() {
        assert!(ptx_looks_real(GOLDEN_PTX_SM80));
        // A comment-only blob (the kind the retired v0.1.0 scaffold emitted)
        // must NOT be treated as real PTX (so PMAT-481 never false-passes).
        let comment_only = "// just a comment\n// no .version directive\n";
        assert!(!ptx_looks_real(comment_only));
    }

    #[test]
    fn real_emitter_output_passes_validate_ptx() {
        // The PMAT-961 real emitter produces PTX that passes the offline
        // well-formedness gate (the retired scaffold's comment placeholder
        // would have failed MissingVersion).
        let ptx = PtxBackend::new()
            .lower(&dummy_module(), &ptx_config("sm_89"))
            .unwrap()
            .primary;
        assert!(ptx_looks_real(&ptx));
        assert_eq!(validate_ptx(&ptx, "sm_89"), Ok(()));
        // A comment-only blob still fails MissingVersion (negative coverage).
        assert_eq!(
            validate_ptx("// comment only\n", "sm_89"),
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
