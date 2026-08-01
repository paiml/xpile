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
//! constructor holds one real `SpirvGeneralEmitter` in the general
//! slot; the witness constructor holds two REAL emitters
//! (`SpirvGeneralEmitter` + `SpirvSaxpySpecialistEmitter`) that
//! reuse the WGSL `2.0*x + 1.0` / `fma` shaders, compile each to SPIR-V,
//! and (with a Vulkan adapter present) run BOTH on the GPU under the
//! [`SpirvDiffExecEngine`].
//!
//! **PMAT-1388 — the emitted SPIR-V is the CALLER'S program.** Until this
//! slice the general emitter took `_module` and discarded it, always
//! compiling the hardcoded `spirv_saxpy_general` fixture: six categorically
//! different inputs (a Python `add`, a Python `fib`, a Python f64 function,
//! a bitwise C module, …) produced *byte-identical* SPIR-V at exit 0, two of
//! them inputs the WGSL sibling this lane is defined to reuse REFUSES
//! outright. The lane is now exactly as wide as that WGSL subset: what WGSL
//! lowers, this compiles; what WGSL refuses, this refuses with that reason.
//! The `fma` specialist — a hand-written variant of one specific arithmetic —
//! declines to vote on anything but that arithmetic's module, so a user
//! program is never quorum-matched against an unrelated reference shader.
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
    extract_wgsl_from_summary, general_metahir_module, general_real_wgsl, is_general_saxpy_module,
    real_wgsl_for, vulkan_adapter_available, SpirvDiffExecEngine, EXPECTED_OUTPUT, FIXTURE_INPUT,
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

/// The per-line prefix carrying one row of emitted SPIR-V binary words as
/// hex in [`spirv_text_summary`]. Distinct from the `;   ` WGSL-inlining
/// prefix so the two blocks are separable by prefix alone, and `;`-leading
/// so the artifact stays a wholly comment-shaped file.
pub const SUMMARY_BINARY_LINE_PREFIX: &str = ";b ";

/// Words rendered per `;b ` line.
const BINARY_WORDS_PER_LINE: usize = 8;

/// Render a human-readable text summary of an emitted SPIR-V module for
/// [`Artifact::primary`]. SPIR-V is binary; the primary text is a stable,
/// auditable disassembly-lite header (magic, version, word count, the
/// source WGSL kept inline as a `; ` comment for round-trip clarity).
///
/// PMAT-1428: the header is followed by the module's actual binary words,
/// hex-encoded one `;b `-prefixed row at a time. Until this slice the words
/// were computed, [`validate_spirv`]-checked, and then **discarded** — this
/// function's doc claimed "the raw binary words go in a sidecar" and
/// `emit_from_wgsl`'s claimed it packaged one, but `EmittedText` has no
/// sidecar channel to package into and none was ever constructed. The CLI
/// prints `Artifact::primary` and nothing else, so `--target spirv` was the
/// only target whose artifact was not the thing the target names: a header
/// asserting `; Words: 63` for a payload no caller could obtain. The words
/// are now IN the artifact and recoverable via
/// [`extract_spirv_words_from_summary`], which is what makes that header
/// count checkable against the module it describes.
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
    out.push_str(
        "; Binary:    the emitted module, one `;b ` row per 8 words, each word\n\
         ;            an 8-digit hex u32. This is the ONLY channel carrying\n\
         ;            these bytes: `--target spirv` prints text. Recover with\n\
         ;            `sed -n 's/^;b //p' <file> | tr -d ' \\n' | xxd -r -p`\n\
         ;            (word-swapped) or `extract_spirv_words_from_summary`.\n",
    );
    for row in words.chunks(BINARY_WORDS_PER_LINE) {
        out.push_str(SUMMARY_BINARY_LINE_PREFIX);
        for (i, w) in row.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&format!("{w:08x}"));
        }
        out.push('\n');
    }
    out.push_str("; Source WGSL (reused from xpile-wgsl-codegen):\n");
    for line in source_wgsl.lines() {
        out.push_str(";   ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Recover the emitted SPIR-V binary words from a [`spirv_text_summary`]
/// artifact — the inverse of the `;b ` block that function writes.
///
/// This is the property that makes the summary's `; Words: N` header a
/// CHECKABLE claim rather than a self-report: the recovered stream must
/// both equal the words that were compiled and satisfy [`validate_spirv`].
/// Returns `Err` when the artifact carries no `;b ` block (an artifact that
/// only *describes* a module) or when a row is not 8-hex-digit words.
pub fn extract_spirv_words_from_summary(summary: &str) -> Result<Vec<u32>, String> {
    let mut words = Vec::new();
    let mut saw_row = false;
    for line in summary.lines() {
        let Some(row) = line.strip_prefix(SUMMARY_BINARY_LINE_PREFIX) else {
            continue;
        };
        saw_row = true;
        for tok in row.split_whitespace() {
            if tok.len() != 8 {
                return Err(format!(
                    "SPIR-V summary binary row has a {}-digit word `{tok}`; \
                     every word is 8 hex digits",
                    tok.len()
                ));
            }
            let w = u32::from_str_radix(tok, 16)
                .map_err(|e| format!("SPIR-V summary binary row word `{tok}` is not hex: {e}"))?;
            words.push(w);
        }
    }
    if !saw_row {
        return Err(format!(
            "SPIR-V summary carries no `{SUMMARY_BINARY_LINE_PREFIX}` block — the \
             artifact describes a module without containing it, so its `; Words:` \
             header cannot be checked. Got:\n{summary}"
        ));
    }
    Ok(words)
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
    /// Single-emitter constructor: one real SPIR-V emitter — the caller's
    /// module lowered to WGSL and compiled to SPIR-V — in the general slot.
    pub fn new() -> Self {
        Self {
            inner: MultiEmitterBackend::new_single(Target::Spirv, Box::new(SpirvGeneralEmitter)),
        }
    }

    /// PMAT-960 — the executed Vulkan SPIR-V-witness constructor (§29).
    ///
    /// Sibling of [`xpile_wgsl_codegen::WgslBackend::new_wgpu_diffexec_witness`].
    /// Builds a `SpirvBackend` whose `MultiEmitterBackend` carries two REAL
    /// SPIR-V emitters — `SpirvGeneralEmitter` (reused WGSL
    /// `2.0*x + 1.0` → SPIR-V) and `SpirvSaxpySpecialistEmitter` (reused
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
            Box::new(SpirvGeneralEmitter),
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
        self.inner
            .lower(module, config)
            .map(|a| a.with_citations(config.emit_contracts))
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
/// naga and packaging the validated words into the text summary.
///
/// PMAT-1428: this said "packaging the text summary + binary-word sidecar".
/// [`EmittedText`] has no sidecar field — a `TargetEmitter` structurally
/// cannot return one — so no sidecar was ever built and the words died
/// here. They now travel in the summary's `;b ` block.
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
/// then compiles the real WGSL to SPIR-V via naga. The arithmetic is
/// xpile's output, not a hardcoded shader.
///
/// PMAT-1388: **of the CALLER'S module.** Until this slice `try_emit` bound
/// the module to `_module` and discarded it, always compiling the hardcoded
/// `spirv_saxpy_general` fixture — so `xpile transpile <anything> --target
/// spirv` exited 0 emitting SPIR-V for `2.0*x + 1.0`, a program the user
/// never wrote, even for inputs the WGSL sibling REFUSES. The SPIR-V lane is
/// now exactly as wide as the WGSL subset it is defined to reuse: what WGSL
/// lowers, this compiles; what WGSL refuses, this refuses with that reason.
struct SpirvGeneralEmitter;

impl TargetEmitter for SpirvGeneralEmitter {
    fn name(&self) -> &str {
        "spirv-general"
    }

    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if let Some(Err(e)) = check_hardware(config) {
            return Some(Err(e));
        }
        // REAL path: xpile lowers THIS module to WGSL, then we compile that
        // to SPIR-V. A real-emit failure is a hard refusal.
        let wgsl = match spirv_diffexec::real_wgsl_for(module) {
            Ok(w) => w,
            Err(e) => {
                return Some(Err(BackendError::Lower(format!(
                    "xpile-spirv-codegen emits by compiling this module's own WGSL \
                     lowering (WGSL -> naga -> spv), and that lowering refused it: {e}"
                ))))
            }
        };
        Some(emit_from_wgsl(&wgsl))
    }
}

/// Specialist SPIR-V emitter — reuses the WGSL `fma(2.0, inp[i], 1.0)`
/// compute shader (same semantics, categorically different SPIR-V).
///
/// PMAT-1388: it is a HAND-WRITTEN variant of ONE specific arithmetic, so it
/// can only cast a quorum vote on the module that computes that arithmetic.
/// For any other module it declines (`None`) — the documented `TargetEmitter`
/// protocol for "my shape filter does not match" — and the quorum honestly
/// reports `Single` instead of pairing the user's program against an
/// unrelated reference shader and calling the two a Match.
struct SpirvSaxpySpecialistEmitter;

impl TargetEmitter for SpirvSaxpySpecialistEmitter {
    fn name(&self) -> &str {
        "spirv-saxpy-specialist-fma"
    }

    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>> {
        if !spirv_diffexec::is_general_saxpy_module(module) {
            return None;
        }
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

    /// A REAL single-function module. PMAT-1388: this used to be an
    /// item-LESS module, which was harmless only because the emitter
    /// discarded its input — every assertion below passed on the hardcoded
    /// saxpy fixture no matter what was handed in. With the emitter reading
    /// its input, the fixture has to be something a compiler could compile.
    fn dummy_module() -> Module {
        use xpile_meta_hir::{Block, Expr, Function, Item, Param, Type};
        Module {
            name: "spirv_kernel".into(),
            source_lang: SourceLang::Rust,
            items: vec![Item::Function(Function {
                name: "triple".into(),
                params: vec![Param {
                    name: "x".into(),
                    ty: Type::I64,
                    mutable: false,
                }],
                return_type: Type::I64,
                body: Block {
                    stmts: vec![],
                    trailing_return: Expr::BinOp {
                        op: xpile_meta_hir::BinOp::Mul,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::LitInt(3)),
                    },
                },
            })],
            ffi_boundaries: Vec::new(),
        }
    }

    fn spirv_config() -> BackendConfig {
        BackendConfig {
            emit_contracts: true,
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
                emitter: "spirv-general".to_string()
            }
        );
        assert!(artifact.primary.contains("SPIR-V"));
        assert!(artifact.citations.iter().any(|c| c.as_str() == CONTRACT_ID));
        // PMAT-1388: the summary inlines the WGSL that was compiled. It must
        // be THIS module's lowering, not the saxpy fixture's.
        assert!(
            artifact.primary.contains("fn triple(") && artifact.primary.contains("(x * i32(3))"),
            "emitted SPIR-V must be compiled from the CALLER's module, got:\n{}",
            artifact.primary
        );
        assert!(
            !artifact.primary.contains("saxpy"),
            "the hardcoded saxpy fixture leaked into an unrelated module's emission:\n{}",
            artifact.primary
        );
    }

    /// PMAT-1388: the `fma` specialist is a hand-written variant of ONE
    /// arithmetic. Pairing it against an unrelated user module and reporting
    /// `Multi` would claim a two-emitter agreement that was never computed,
    /// so it declines and the quorum honestly degrades to `Single`.
    #[test]
    fn specialist_declines_to_vote_on_an_unrelated_module() {
        let backend = SpirvBackend::new_spirv_diffexec_witness();
        let artifact = backend.lower(&dummy_module(), &spirv_config()).unwrap();
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "spirv-general".to_string()
            },
            "an unrelated module must not be quorum-paired with the saxpy reference"
        );
        // …and it DOES vote on the module it is a variant of (without which
        // the decline above would be vacuous — it would pass if the
        // specialist never fired at all).
        let saxpy = backend
            .lower(&general_metahir_module(), &spirv_config())
            .unwrap();
        match saxpy.quorum_status {
            QuorumStatus::Multi { ref emitters, .. } => {
                assert_eq!(emitters.len(), 2, "got {emitters:?}");
            }
            other => panic!("saxpy module must still pair both emitters, got {other:?}"),
        }
    }

    /// PMAT-1388: a construct the WGSL lane refuses must REFUSE here, not
    /// exit 0 with a canned shader. `f64` is the WGSL lane's own documented
    /// refusal (WGSL core has no 64-bit float).
    #[test]
    fn backend_refuses_what_the_wgsl_lowering_refuses() {
        use xpile_meta_hir::{Block, Expr, Function, Item, Param, Type};
        let m = Module {
            name: "f64_kernel".into(),
            source_lang: SourceLang::Rust,
            items: vec![Item::Function(Function {
                name: "widen".into(),
                params: vec![Param {
                    name: "x".into(),
                    ty: Type::F64,
                    mutable: false,
                }],
                return_type: Type::F64,
                body: Block {
                    stmts: vec![],
                    trailing_return: Expr::Ident("x".into()),
                },
            })],
            ffi_boundaries: Vec::new(),
        };
        let err = SpirvBackend::new().lower(&m, &spirv_config()).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("WGSL") && msg.contains("f64"),
            "refusal must name the WGSL lowering and the offending construct, got: {msg}"
        );
    }

    /// PMAT-1388 anti-regression: the witness's general-side WGSL is now
    /// produced by the module-taking `real_wgsl_for`, and must be
    /// byte-identical to what the no-argument helper produced before, or the
    /// GPU witness is executing something new without saying so.
    #[test]
    fn saxpy_general_wgsl_is_the_real_wgsl_for_the_saxpy_module() {
        let via_module = real_wgsl_for(&general_metahir_module()).unwrap();
        let via_helper = general_real_wgsl().unwrap();
        assert_eq!(via_module, via_helper);
        assert!(via_helper.contains("fn saxpy(x: f32) -> f32"));
        assert!(via_helper.contains("outp[i] = saxpy(inp[i]);"));
        assert!(via_helper.contains("array<f32>"));
    }

    #[test]
    fn backend_accepts_no_hardware() {
        let backend = SpirvBackend::new();
        let cfg = BackendConfig {
            emit_contracts: true,
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
            emit_contracts: true,
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
