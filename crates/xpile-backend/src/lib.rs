//! Backend trait — code-lane emission abstraction.
//!
//! Every target language in xpile (Rust, Ruchy, PTX, WGSL, SPIR-V, WASM,
//! Lean, Shell, forjar.yaml) provides one type implementing [`Backend`].
//! That enumeration is the whole [`Target`] roster, and
//! `crate_metadata_honesty.rs` (XPILE-CRATEMETA-001) reds if it drifts from
//! it — through v0.1.617 this sentence quantified over EVERY target language
//! and then named six of the nine. The trait is intentionally narrow: take
//! meta-HIR and a config, return an [`Artifact`].
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
    /// SPIR-V — the native Vulkan IR. Implemented by `xpile-spirv-codegen`
    /// (PMAT-960). REUSES the WGSL emission and compiles it WGSL→naga→spv
    /// (NOT a hand-written SPIR-V assembler); the §29 cross-emitter witness
    /// RUNS the emitted SPIR-V on a real wgpu Vulkan adapter. The native-IR
    /// sibling of `Target::Wgsl`.
    Spirv,
    /// WebAssembly Text format (WAT). Implemented by `xpile-wasm-codegen`.
    /// The EMIT half of first-class bidirectional native WASM (PMAT-951) —
    /// lowers the meta-HIR scalar/control subset directly to WAT text, NOT
    /// via the Ruchy `WasmEmitter` hop. See `project-bidirectional-wasm`.
    Wasm,
    /// Lean 4 executable code (def, partial def, inductive, ...).
    /// Implemented by `xpile-lean-codegen`. The proof-lane Lean (theorems)
    /// goes through `xpile-lean-contract-backend` instead.
    Lean,
    /// POSIX shell (sh / bash / zsh) — the bashrs merger domain.
    /// Implemented by `bashrs-backend`, which renders REAL POSIX shell:
    /// commands, pipelines and assignment with quoting, variables and
    /// command substitution, plus the shell lane's control-flow
    /// statements. PMAT-037 / XPILE-BASHRS-MERGER-001; see
    /// `sub/bashrs-merger.md`. Through v0.1.617 this doc deferred the
    /// real emit to a later release (PMAT-1465); `crate_metadata_honesty.rs`
    /// now LOWERS a shell module through the registered backend and reds
    /// while any such deferral is published here.
    Shell,
    /// forjar.yaml IaC manifest text. Implemented by
    /// `xpile-forjar-codegen` (PMAT-953). The BACKEND-ONLY forjar
    /// integration (NOT merge/federate) — a declarative ops/deployment
    /// output lane, peer to bashrs-backend's shell lane. Lowers a
    /// SHELL-origin meta-HIR `Module` (the `Stmt::Cmd` / `Stmt::Pipeline`
    /// command sequence) to forjar `type: file` / `type: task` resources.
    /// REFUSES non-shell modules and any shell idempotence guard /
    /// conditional — forjar has no conditional resource kind to lower a
    /// `Stmt::ShellIf` INTO — so only unconditional resources emit, and
    /// forjar re-adds convergence at apply time. See
    /// `project-forjar-output-backend`.
    ForjarYaml,
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
    /// PMAT-956: whether emitted code is annotated with its `// xpile-contract:`
    /// citations across the applicable L1–L5 taxonomy layers. `true` by
    /// default, which annotates each construct that HAS an applicable
    /// contract; "every construct under a cited contract" is the NORTH-STAR,
    /// not the current state — `applicable_contracts()` is empty for a
    /// comparison-only or call-only body and those emit no citation line.
    /// (Through v0.1.617 this read "every emitted construct is cited";
    /// `audit-design.md`'s capability-vs-contract case study records the same
    /// slogan as falsified in practice. PMAT-1447.) Set `false` for
    /// annotation-free output. Every `Backend::lower` honours it, so a library
    /// caller controls citation emission through the config, exactly as the
    /// CLI's `--contracts off` does.
    #[serde(default = "default_emit_contracts")]
    pub emit_contracts: bool,
}

fn default_emit_contracts() -> bool {
    true
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

/// PMAT-956: the comment/annotation forms every backend uses to emit a
/// `xpile-contract` citation. Rust/Ruchy/PTX/WGSL/SPIR-V use `//`, WAT uses
/// `;;`, shell/forjar `#`, one lane `--`, the Lean CODE lane a docstring
/// (`/-- xpile-contract: … -/`, PMAT-1405) and the Lean CONTRACT-RENDERING lane
/// the `@[xpile_contract …]` attribute. Used by [`strip_contract_citations`].
///
/// PMAT-1405: `/-- xpile-contract` must be listed even though `-- xpile-contract`
/// is a substring of it. [`strip_contract_citations`] cuts at the EARLIEST
/// marker on the line and drops the line only when nothing but whitespace
/// precedes that cut; matching `-- xpile-contract` at index 1 of
/// `/-- xpile-contract: C-X -/` would take the INLINE branch and leave a stray
/// `/` line behind. Pinned by `lean_docstring_citation_is_stripped_whole`.
const CITATION_MARKERS: &[&str] = &[
    "// xpile-contract",
    ";; xpile-contract",
    "# xpile-contract",
    "/-- xpile-contract",
    "-- xpile-contract",
    "@[xpile_contract",
];

/// PMAT-956 (provable-model-as-code / optional contract emission): return
/// `text` with every emitted `xpile-contract` citation removed — for callers
/// who want annotation-free output. Contract citation is ON by default, which
/// cites each emitted construct that HAS an applicable contract across the
/// L1–L5 taxonomy layers — frequently none, since `applicable_contracts()` is
/// empty for a comparison-only or call-only body. (Through v0.1.617 this said
/// "every emitted construct is cited", the same universal `xpile transpile
/// --help` carried; PMAT-1447.) This is the library counterpart of the CLI's
/// `--contracts off`, so BOTH the library and the binary can optionally
/// suppress citations.
///
/// Handles the two shapes emitted across the nine backends:
///   * a STANDALONE citation line (Rust/Ruchy/shell/…, e.g.
///     `// xpile-contract: C-PY-INT-ARITH`; the Lean CODE lane's
///     `/-- xpile-contract: … -/` docstring; and `@[xpile_contract "…"]`,
///     which since PMAT-1405 only the contract-RENDERING lane produces and
///     which is handled here defensively) — the whole line is dropped;
///   * an INLINE trailing citation (WAT, e.g.
///     `(func $f (param …) (result i64) ;; xpile-contract: …`) — only the
///     trailing comment is trimmed, keeping the code before it.
///
/// A line whose citation is not at a comment/annotation boundary (i.e. real
/// content that merely mentions the string) is left untouched.
pub fn strip_contract_citations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let trailing_newline = text.ends_with('\n');
    let mut first = true;
    for line in text.lines() {
        // Earliest citation-marker position on this line, if any.
        let cut = CITATION_MARKERS.iter().filter_map(|m| line.find(m)).min();
        match cut {
            // A standalone citation line (only whitespace before the marker):
            // drop it entirely.
            Some(idx) if line[..idx].trim().is_empty() => continue,
            // An inline citation: keep the code before it, trim the comment.
            Some(idx) => {
                if !first {
                    out.push('\n');
                }
                out.push_str(line[..idx].trim_end());
                first = false;
            }
            // No citation: keep the line verbatim.
            None => {
                if !first {
                    out.push('\n');
                }
                out.push_str(line);
                first = false;
            }
        }
    }
    if trailing_newline && !out.is_empty() {
        out.push('\n');
    }
    out
}

impl Artifact {
    /// PMAT-956: honour a [`BackendConfig::emit_contracts`] flag on this
    /// artifact — a no-op when `emit` is `true` (the default: keep every
    /// citation), or strips the `xpile-contract` citations from `primary` when
    /// `false`. Every `Backend::lower` calls this at its return point, so the
    /// optional-emission control is config-driven and uniform across backends.
    #[must_use]
    pub fn with_citations(mut self, emit: bool) -> Self {
        if !emit {
            self.primary = strip_contract_citations(&self.primary);
        }
        self
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

// ─── PMAT-263 / Section 29: TargetEmitter trait + MultiEmitterBackend ────
//
// Routing layer for multi-emitter backends. A [`MultiEmitterBackend`]
// composes a general emitter (mandatory fallback) + an optional
// specialist emitter under a [`QuorumPolicy`]. Single-emitter and
// multi-emitter cases produce explicit `QuorumStatus` on the emitted
// [`Artifact`].

/// Plain text emitted by a single [`TargetEmitter`] before the
/// multi-emitter routing decides what to put in the final [`Artifact`].
/// Doesn't carry `QuorumStatus` (that's chosen by the wrapper).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmittedText {
    pub primary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<ContractId>,
}

/// Single-emitter trait — sub-trait of a multi-emitter backend.
/// One [`TargetEmitter`] handles emission for one logical path
/// (general vs specialist). Each [`MultiEmitterBackend`] wraps
/// one mandatory general emitter and optionally one specialist.
pub trait TargetEmitter: Send + Sync {
    /// Human-readable emitter name (used in [`QuorumStatus`]).
    fn name(&self) -> &str;

    /// Attempt to emit for this input. Specialists return `None`
    /// when their shape filter doesn't match — the wrapper then
    /// uses only the general emitter. General emitters should
    /// always return `Some(...)` for any contract-conforming input.
    fn try_emit(
        &self,
        module: &Module,
        config: &BackendConfig,
    ) -> Option<Result<EmittedText, BackendError>>;
}

/// PMAT-486 (§30 Track 4): engine that executes two emitted programs on
/// contract-fixture inputs and numerically compares the outputs within a
/// tolerance — the Runtime-stratum half of the §29 quorum. The trait +
/// hook land here (free CI); the real CUDA / Vulkan implementations
/// (PMAT-488 / PMAT-490) run out-of-band on self-hosted GPU runners.
///
/// Error posture (per the §30 Track-4 review): with **no engine
/// installed** the `DiffExec` policy records `NotRun { reason: no-engine }`
/// (benign — free CI stays green). An **installed** engine that returns
/// `Err` propagates a hard [`BackendError`] that fails the job — a broken
/// GPU run must NOT masquerade as "not run".
pub trait DiffExecEngine: Send + Sync {
    /// Execute `general_text` and `specialist_text` on the contract's
    /// fixture inputs and compare. `Ok(Match|Divergent)` records the
    /// vote; `Err(msg)` is a hard failure (e.g. driver fault / launch
    /// error) the caller turns into a `BackendError`.
    fn execute_and_compare(
        &self,
        general_text: &str,
        specialist_text: &str,
        module: &Module,
        config: &BackendConfig,
        tolerance: f64,
    ) -> Result<DiffExecResult, String>;
}

/// Multi-emitter backend wrapper. Composes a general emitter
/// (mandatory) + an optional specialist under a [`QuorumPolicy`].
/// Implements [`Backend`] so it slots into the existing dispatch
/// without touching `TranspileSession`.
pub struct MultiEmitterBackend {
    /// Single target this multi-emitter backend serves
    /// (e.g., [`Target::Ptx`]).
    pub target: Target,
    /// Mandatory general emitter. Must handle any
    /// contract-conforming input as a fallback.
    pub general: Box<dyn TargetEmitter>,
    /// Optional specialist emitter. Returns `None` from `try_emit`
    /// when its shape filter doesn't match the input.
    pub specialist: Option<Box<dyn TargetEmitter>>,
    /// How to combine outputs when both emitters fire.
    pub quorum_policy: QuorumPolicy,
    /// PMAT-486: optional `DiffExec` execution engine. `None` (the
    /// default) records `NotRun { no-engine }` under `QuorumPolicy::
    /// DiffExec`; `Some(engine)` runs the Runtime-stratum comparison.
    pub diff_exec_engine: Option<std::sync::Arc<dyn DiffExecEngine>>,
    /// PMAT-1006: names of ADDITIONAL categorically-independent emitters the
    /// installed `DiffExec` engine runs INTERNALLY beyond `general`+`specialist`
    /// (e.g. the PTX §29 3-way quorum's `rustc-nvptx` arm, self-generated inside
    /// the engine). When the engine reports a `Match`/`Divergent` (i.e. it
    /// actually executed), these are appended to the reported `emitters` list so
    /// `QuorumStatus::Multi` HONESTLY names every toolchain that voted. Empty for
    /// every other backend (no behaviour change).
    pub diff_exec_extra_emitters: Vec<String>,
}

impl MultiEmitterBackend {
    pub fn new_single(target: Target, general: Box<dyn TargetEmitter>) -> Self {
        Self {
            target,
            general,
            specialist: None,
            quorum_policy: QuorumPolicy::PreferSpecialist,
            diff_exec_engine: None,
            diff_exec_extra_emitters: Vec::new(),
        }
    }

    pub fn new_with_specialist(
        target: Target,
        general: Box<dyn TargetEmitter>,
        specialist: Box<dyn TargetEmitter>,
        quorum_policy: QuorumPolicy,
    ) -> Self {
        Self {
            target,
            general,
            specialist: Some(specialist),
            quorum_policy,
            diff_exec_engine: None,
            diff_exec_extra_emitters: Vec::new(),
        }
    }

    /// PMAT-486: install a `DiffExec` engine (builder style). The real
    /// CUDA / Vulkan engines (PMAT-488 / PMAT-490) plug in here on the
    /// self-hosted GPU runners; on free CI the engine stays `None`.
    pub fn with_diff_exec_engine(mut self, engine: std::sync::Arc<dyn DiffExecEngine>) -> Self {
        self.diff_exec_engine = Some(engine);
        self
    }

    /// PMAT-1006: declare additional emitters the installed `DiffExec` engine
    /// runs internally (beyond general+specialist), so an EXECUTED `Multi`
    /// result names every toolchain that voted (the PTX 3-way §29 quorum).
    pub fn with_diff_exec_extra_emitters(mut self, names: Vec<String>) -> Self {
        self.diff_exec_extra_emitters = names;
        self
    }
}

impl Backend for MultiEmitterBackend {
    fn name(&self) -> &'static str {
        // The wrapper name is generic; per-emitter names live in
        // QuorumStatus on each emitted Artifact for audit recovery.
        "multi-emitter"
    }

    fn targets(&self) -> &[Target] {
        std::slice::from_ref(&self.target)
    }

    fn lower(&self, module: &Module, config: &BackendConfig) -> Result<Artifact, BackendError> {
        let general_result = self.general.try_emit(module, config).ok_or_else(|| {
            BackendError::Lower(format!(
                "general emitter {} must always match contract-conforming input",
                self.general.name()
            ))
        })??;

        let specialist_result = self.specialist.as_ref().and_then(|s| {
            s.try_emit(module, config)
                .map(|r| (s.name().to_string(), r))
        });

        match specialist_result {
            None => {
                // Only general fired — Single-vote Runtime stratum.
                Ok(Artifact {
                    primary: general_result.primary,
                    sidecars: Vec::new(),
                    citations: general_result.citations,
                    quorum_status: QuorumStatus::Single {
                        emitter: self.general.name().to_string(),
                    },
                })
            }
            Some((specialist_name, specialist_emit)) => {
                let specialist_text = specialist_emit?;
                match &self.quorum_policy {
                    QuorumPolicy::PreferSpecialist => {
                        // Use specialist's output; general was emitted for
                        // sanity but isn't reported as a vote.
                        Ok(Artifact {
                            primary: specialist_text.primary,
                            sidecars: Vec::new(),
                            citations: specialist_text.citations,
                            quorum_status: QuorumStatus::Single {
                                emitter: specialist_name,
                            },
                        })
                    }
                    QuorumPolicy::Strict => {
                        // Text-equality check.
                        let diff_exec = if general_result.primary == specialist_text.primary {
                            Some(DiffExecResult::Match { max_abs_diff: 0.0 })
                        } else {
                            Some(DiffExecResult::Divergent {
                                max_abs_diff: f64::INFINITY,
                                tolerance: 0.0,
                            })
                        };
                        Ok(Artifact {
                            primary: general_result.primary.clone(),
                            sidecars: vec![(
                                "specialist_emission".to_string(),
                                specialist_text.primary.into_bytes(),
                            )],
                            citations: general_result.citations,
                            quorum_status: QuorumStatus::Multi {
                                emitters: vec![self.general.name().to_string(), specialist_name],
                                diff_exec,
                            },
                        })
                    }
                    QuorumPolicy::DiffExec { tolerance } => {
                        // PMAT-486: run the installed engine, or record
                        // NotRun{no-engine} when none is installed (free
                        // CI). An installed engine that errors propagates
                        // a hard BackendError — a broken GPU run must NOT
                        // masquerade as "not run".
                        let diff_exec = match &self.diff_exec_engine {
                            Some(engine) => engine
                                .execute_and_compare(
                                    &general_result.primary,
                                    &specialist_text.primary,
                                    module,
                                    config,
                                    *tolerance,
                                )
                                .map_err(|e| {
                                    BackendError::Lower(format!(
                                        "DiffExec engine for {:?} failed: {e}",
                                        self.target
                                    ))
                                })?,
                            None => DiffExecResult::NotRun {
                                reason: format!(
                                    "no DiffExec engine installed (tolerance was {tolerance})"
                                ),
                            },
                        };
                        // PMAT-1006: the base voters are general + specialist.
                        // When the engine actually EXECUTED (Match/Divergent, not
                        // NotRun), append the extra toolchains it ran internally
                        // (e.g. the PTX 3-way `rustc-nvptx` arm) so the reported
                        // quorum HONESTLY names every voter. A non-executed run
                        // (NotRun, no GPU) reports only the emitters that fired.
                        let mut emitters = vec![self.general.name().to_string(), specialist_name];
                        if matches!(
                            diff_exec,
                            DiffExecResult::Match { .. } | DiffExecResult::Divergent { .. }
                        ) {
                            emitters.extend(self.diff_exec_extra_emitters.iter().cloned());
                        }
                        Ok(Artifact {
                            primary: general_result.primary.clone(),
                            sidecars: vec![(
                                "specialist_emission".to_string(),
                                specialist_text.primary.into_bytes(),
                            )],
                            citations: general_result.citations,
                            quorum_status: QuorumStatus::Multi {
                                emitters,
                                diff_exec: Some(diff_exec),
                            },
                        })
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod strip_citation_tests {
    use super::strip_contract_citations;

    #[test]
    fn drops_standalone_comment_citation_lines() {
        let src = "// xpile-contract: C-PY-INT-ARITH\npub fn f() {}\n";
        assert_eq!(strip_contract_citations(src), "pub fn f() {}\n");
    }

    #[test]
    fn drops_lean_attribute_citation_lines() {
        let src = "@[xpile_contract \"C-PY-INT-ARITH\"]\ndef f := 1\n";
        assert_eq!(strip_contract_citations(src), "def f := 1\n");
    }

    /// PMAT-1405: the Lean CODE lane cites via a docstring. The whole line must
    /// go — not just the part from `-- xpile-contract` onward, which would leave
    /// a stray `/` behind. This is the regression that the `/-- xpile-contract`
    /// entry in `CITATION_MARKERS` exists to prevent: remove it and this test
    /// reds with `"/\ndef f := 1"`.
    #[test]
    fn lean_docstring_citation_is_stripped_whole() {
        let src = "/-- xpile-contract: C-PY-INT-ARITH -/\ndef f := 1\n";
        assert_eq!(strip_contract_citations(src), "def f := 1\n");
    }

    /// PMAT-1405: multiple applicable contracts share ONE docstring (Lean
    /// rejects stacked `/-- … -/` blocks), so the multi-id form must strip
    /// whole too.
    #[test]
    fn lean_docstring_citation_with_multiple_ids_is_stripped_whole() {
        let src = "/-- xpile-contract: C-PY-INT-ARITH, C-CONST-TRANSLATION -/\ndef f := 1\n";
        assert_eq!(strip_contract_citations(src), "def f := 1\n");
    }

    #[test]
    fn trims_inline_wat_citation_keeping_the_code() {
        // WAT emits the citation INLINE on the func line — strip must keep the
        // func declaration and drop only the trailing `;;` comment.
        let src =
            "  (func $f (result i64) ;; xpile-contract: C-COMPILE-RUST-TO-WASM\n    i64.const 1)\n";
        let out = strip_contract_citations(src);
        assert!(
            out.contains("(func $f (result i64)"),
            "keeps the func decl:\n{out}"
        );
        assert!(
            !out.contains("xpile-contract"),
            "drops the citation:\n{out}"
        );
    }

    #[test]
    fn keeps_content_that_merely_mentions_a_contract() {
        // A panic message referencing a contract is NOT a citation comment.
        let src = "    .expect(\"overflow; contract C-PY-INT-ARITH slow path\");\n";
        assert_eq!(strip_contract_citations(src), src);
    }

    #[test]
    fn preserves_uncited_code_verbatim() {
        let src = "pub fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n";
        assert_eq!(strip_contract_citations(src), src);
    }
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

    // ─── PMAT-263: MultiEmitterBackend routing tests with mock emitters ─

    /// Mock emitter returning a fixed primary string and always matching
    /// (use for `general` role). Cloneable to construct two copies for
    /// match cases.
    struct MockGeneral {
        name: &'static str,
        body: String,
    }
    impl TargetEmitter for MockGeneral {
        fn name(&self) -> &str {
            self.name
        }
        fn try_emit(
            &self,
            _module: &Module,
            _config: &BackendConfig,
        ) -> Option<Result<EmittedText, BackendError>> {
            Some(Ok(EmittedText {
                primary: self.body.clone(),
                citations: Vec::new(),
            }))
        }
    }

    /// Mock specialist emitter — matches conditionally and returns a
    /// configurable body.
    struct MockSpecialist {
        name: &'static str,
        matches: bool,
        body: String,
    }
    impl TargetEmitter for MockSpecialist {
        fn name(&self) -> &str {
            self.name
        }
        fn try_emit(
            &self,
            _module: &Module,
            _config: &BackendConfig,
        ) -> Option<Result<EmittedText, BackendError>> {
            if self.matches {
                Some(Ok(EmittedText {
                    primary: self.body.clone(),
                    citations: Vec::new(),
                }))
            } else {
                None
            }
        }
    }

    fn dummy_module() -> Module {
        Module {
            name: "test".into(),
            source_lang: xpile_meta_hir::SourceLang::Rust,
            items: Vec::new(),
            ffi_boundaries: Vec::new(),
        }
    }

    fn dummy_config() -> BackendConfig {
        BackendConfig {
            emit_contracts: true,
            target: Target::Ptx,
            profile: Profile::RustOut,
            hardware: None,
        }
    }

    #[test]
    fn multi_emitter_specialist_missing_falls_back_to_general() {
        let backend = MultiEmitterBackend::new_single(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "general output".into(),
            }),
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        assert_eq!(artifact.primary, "general output");
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "general".to_string()
            }
        );
    }

    #[test]
    fn multi_emitter_specialist_unmatched_falls_back_to_general() {
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "general output".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: false,
                body: "specialist output".into(),
            }),
            QuorumPolicy::DiffExec { tolerance: 1e-3 },
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        assert_eq!(artifact.primary, "general output");
        // Specialist returned None → Single vote.
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "general".to_string()
            }
        );
    }

    #[test]
    fn multi_emitter_prefer_specialist_uses_specialist_output() {
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "general".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "specialist tuned".into(),
            }),
            QuorumPolicy::PreferSpecialist,
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        assert_eq!(artifact.primary, "specialist tuned");
        assert_eq!(
            artifact.quorum_status,
            QuorumStatus::Single {
                emitter: "specialist".to_string()
            }
        );
    }

    #[test]
    fn multi_emitter_strict_match_records_zero_diff() {
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "same output".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "same output".into(),
            }),
            QuorumPolicy::Strict,
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec,
            } => {
                assert_eq!(emitters, vec!["general", "specialist"]);
                assert_eq!(diff_exec, Some(DiffExecResult::Match { max_abs_diff: 0.0 }));
            }
            _ => panic!("expected Multi quorum status"),
        }
        // Specialist's output recorded as sidecar for audit trail.
        assert_eq!(artifact.sidecars.len(), 1);
        assert_eq!(artifact.sidecars[0].0, "specialist_emission");
    }

    #[test]
    fn multi_emitter_strict_divergence_records_infinity() {
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "general output".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "different output".into(),
            }),
            QuorumPolicy::Strict,
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi { diff_exec, .. } => {
                assert!(matches!(diff_exec, Some(DiffExecResult::Divergent { .. })));
            }
            _ => panic!("expected Multi quorum status"),
        }
    }

    #[test]
    fn multi_emitter_diff_exec_records_not_run_until_engine_plugged_in() {
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "rustc_codegen_nvvm",
                body: "ptx general".into(),
            }),
            Box::new(MockSpecialist {
                name: "aprender-gpu",
                matches: true,
                body: "ptx specialist".into(),
            }),
            QuorumPolicy::DiffExec { tolerance: 1e-3 },
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi {
                emitters,
                diff_exec,
            } => {
                assert_eq!(emitters, vec!["rustc_codegen_nvvm", "aprender-gpu"]);
                // DiffExec engine isn't plugged in yet — should record NotRun.
                assert!(matches!(diff_exec, Some(DiffExecResult::NotRun { .. })));
            }
            _ => panic!("expected Multi quorum status"),
        }
    }

    // ─── PMAT-266: Adversarial invariants for MultiEmitterBackend ───
    //
    // These tests pin down security-relevant contract behavior that the
    // PMAT-263 happy-path tests don't cover: citation provenance, error
    // propagation, hidden-divergence documentation, and observability of
    // the `NotRun` reason. They guard against silent regressions in the
    // routing layer that would weaken the Section 29 oracle.

    /// Mock emitter with configurable citations — used to verify which
    /// emitter's citations end up in the final Artifact.
    struct MockGeneralWithCitations {
        body: String,
        citations: Vec<ContractId>,
    }
    impl TargetEmitter for MockGeneralWithCitations {
        fn name(&self) -> &str {
            "general-with-cites"
        }
        fn try_emit(
            &self,
            _module: &Module,
            _config: &BackendConfig,
        ) -> Option<Result<EmittedText, BackendError>> {
            Some(Ok(EmittedText {
                primary: self.body.clone(),
                citations: self.citations.clone(),
            }))
        }
    }

    /// Mock specialist that always matches and carries configurable
    /// citations distinct from `MockGeneralWithCitations`.
    struct MockSpecialistWithCitations {
        body: String,
        citations: Vec<ContractId>,
    }
    impl TargetEmitter for MockSpecialistWithCitations {
        fn name(&self) -> &str {
            "specialist-with-cites"
        }
        fn try_emit(
            &self,
            _module: &Module,
            _config: &BackendConfig,
        ) -> Option<Result<EmittedText, BackendError>> {
            Some(Ok(EmittedText {
                primary: self.body.clone(),
                citations: self.citations.clone(),
            }))
        }
    }

    /// Mock emitter that always fails — used to verify error
    /// propagation from each role.
    struct MockFailingEmitter {
        name: &'static str,
        err: String,
    }
    impl TargetEmitter for MockFailingEmitter {
        fn name(&self) -> &str {
            self.name
        }
        fn try_emit(
            &self,
            _module: &Module,
            _config: &BackendConfig,
        ) -> Option<Result<EmittedText, BackendError>> {
            Some(Err(BackendError::Lower(self.err.clone())))
        }
    }

    /// Mock emitter that returns `None` — for `general`, this is a
    /// contract violation (general MUST match contract-conforming
    /// input); the wrapper must surface it as a hard `BackendError`.
    struct MockNoneEmitter;
    impl TargetEmitter for MockNoneEmitter {
        fn name(&self) -> &str {
            "always-none"
        }
        fn try_emit(
            &self,
            _module: &Module,
            _config: &BackendConfig,
        ) -> Option<Result<EmittedText, BackendError>> {
            None
        }
    }

    #[test]
    fn strict_divergence_preserves_general_citations_not_specialist() {
        // Security invariant: under `Strict`, citations come from
        // `general`. The proof lane relies on this — citations identify
        // which contracts the artifact was authored against. A
        // specialist that disagrees with general must not be able to
        // silently swap its own citations into the audit trail.
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneralWithCitations {
                body: "general output".into(),
                citations: vec![ContractId::new("C-GENERAL-CITED")],
            }),
            Box::new(MockSpecialistWithCitations {
                body: "different output".into(),
                citations: vec![ContractId::new("C-SPECIALIST-CITED")],
            }),
            QuorumPolicy::Strict,
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        assert_eq!(artifact.citations.len(), 1);
        assert_eq!(artifact.citations[0].as_str(), "C-GENERAL-CITED");
        // Specialist's body is still recoverable from the sidecar even
        // though its citations are dropped.
        assert_eq!(artifact.sidecars.len(), 1);
        assert_eq!(
            artifact.sidecars[0].1,
            b"different output".to_vec(),
            "specialist body should be preserved in sidecar"
        );
    }

    #[test]
    fn prefer_specialist_hides_divergence_by_design() {
        // Documented trade-off: `PreferSpecialist` is the
        // single-vote-runtime stratum — it intentionally does NOT
        // compare general vs specialist. Use `Strict` or `DiffExec`
        // when divergence detection matters. This test pins down the
        // behavior so a future "helpful" refactor can't accidentally
        // turn this into a quiet divergence detector.
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneralWithCitations {
                body: "general thinks the answer is 42".into(),
                citations: vec![ContractId::new("C-GENERAL")],
            }),
            Box::new(MockSpecialistWithCitations {
                body: "specialist thinks the answer is 99".into(),
                citations: vec![ContractId::new("C-SPECIALIST")],
            }),
            QuorumPolicy::PreferSpecialist,
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        // Specialist's body wins; specialist's citations win;
        // QuorumStatus reports Single (no divergence captured).
        assert!(artifact.primary.contains("99"));
        assert_eq!(artifact.citations[0].as_str(), "C-SPECIALIST");
        match artifact.quorum_status {
            QuorumStatus::Single { emitter } => {
                assert_eq!(emitter, "specialist-with-cites");
            }
            other => panic!("expected Single quorum status, got {other:?}"),
        }
        // No sidecar — general's emission isn't even captured.
        assert!(artifact.sidecars.is_empty());
    }

    #[test]
    fn general_emitter_failure_propagates() {
        // If `general` returns Some(Err(...)), the wrapper must
        // propagate the error — never silently fall through to
        // specialist. General is the mandatory fallback; its failure
        // is the whole backend's failure.
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockFailingEmitter {
                name: "general-broken",
                err: "general blew up".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "specialist output".into(),
            }),
            QuorumPolicy::PreferSpecialist,
        );
        let err = backend.lower(&dummy_module(), &dummy_config()).unwrap_err();
        match err {
            BackendError::Lower(msg) => assert!(msg.contains("general blew up")),
            other => panic!("expected Lower error, got {other:?}"),
        }
    }

    #[test]
    fn specialist_emitter_failure_propagates_when_matched() {
        // If `specialist` matches and then errors, propagate. This is
        // a real partial-failure mode for shape-tuned emitters
        // (matched on shape but failed during lowering).
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "general output".into(),
            }),
            Box::new(MockFailingEmitter {
                name: "specialist-broken",
                err: "specialist blew up after matching".into(),
            }),
            QuorumPolicy::Strict,
        );
        let err = backend.lower(&dummy_module(), &dummy_config()).unwrap_err();
        match err {
            BackendError::Lower(msg) => {
                assert!(msg.contains("specialist blew up after matching"))
            }
            other => panic!("expected Lower error, got {other:?}"),
        }
    }

    #[test]
    fn general_returning_none_is_a_hard_contract_violation() {
        // `general` returning `None` from `try_emit` means it refused
        // to handle contract-conforming input — that's a hard error.
        // (Specialists are allowed to return None; general isn't.)
        let backend = MultiEmitterBackend::new_single(Target::Ptx, Box::new(MockNoneEmitter));
        let err = backend.lower(&dummy_module(), &dummy_config()).unwrap_err();
        match err {
            BackendError::Lower(msg) => {
                assert!(
                    msg.contains("always-none"),
                    "error should name the offending emitter; got: {msg}"
                );
                assert!(msg.contains("must always match"));
            }
            other => panic!("expected Lower error, got {other:?}"),
        }
    }

    #[test]
    fn diff_exec_not_run_reason_records_tolerance_for_observability() {
        // The `NotRun` reason is the user-facing breadcrumb pointing
        // at "DiffExec engine not yet wired" — and it must carry the
        // configured tolerance so debug output is actionable.
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "g".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "s".into(),
            }),
            QuorumPolicy::DiffExec { tolerance: 2.5e-4 },
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi {
                diff_exec: Some(DiffExecResult::NotRun { reason }),
                ..
            } => {
                assert!(
                    reason.contains("0.00025")
                        || reason.contains("2.5e-4")
                        || reason.contains("0.000250"),
                    "tolerance should appear in NotRun reason; got: {reason}"
                );
            }
            other => panic!("expected Multi NotRun status, got {other:?}"),
        }
    }

    #[test]
    fn diff_exec_does_not_short_circuit_on_text_equality() {
        // Architectural invariant: even when general and specialist
        // emit byte-identical text, `DiffExec` policy must still
        // record `NotRun` (because the real engine compares numerical
        // outputs after execution, not source text). A future
        // optimization that says "skip diff if text matches" would
        // break this invariant — the engine's job is to check the
        // RUNTIME behavior, and identical source could still produce
        // divergent runtime values on different hardware.
        let backend = MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "byte identical".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "byte identical".into(),
            }),
            QuorumPolicy::DiffExec { tolerance: 1e-6 },
        );
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi { diff_exec, .. } => {
                assert!(
                    matches!(diff_exec, Some(DiffExecResult::NotRun { .. })),
                    "DiffExec policy must NOT short-circuit on text equality \
                     — engine compares runtime values, not source text"
                );
            }
            other => panic!("expected Multi quorum status, got {other:?}"),
        }
    }

    // ─── PMAT-486: DiffExecEngine trait + hook ──────────────────────

    /// Stub engine returning a fixed result (or a hard error).
    struct StubEngine {
        result: Result<DiffExecResult, String>,
    }
    impl DiffExecEngine for StubEngine {
        fn execute_and_compare(
            &self,
            _g: &str,
            _s: &str,
            _m: &Module,
            _c: &BackendConfig,
            _tol: f64,
        ) -> Result<DiffExecResult, String> {
            self.result.clone()
        }
    }

    fn diff_exec_backend() -> MultiEmitterBackend {
        MultiEmitterBackend::new_with_specialist(
            Target::Ptx,
            Box::new(MockGeneral {
                name: "general",
                body: "g".into(),
            }),
            Box::new(MockSpecialist {
                name: "specialist",
                matches: true,
                body: "s".into(),
            }),
            QuorumPolicy::DiffExec { tolerance: 1e-6 },
        )
    }

    /// PMAT-486: an installed engine's `Ok(Match)` becomes the recorded
    /// Runtime vote (replacing NotRun).
    #[test]
    fn diff_exec_engine_records_match() {
        let backend = diff_exec_backend().with_diff_exec_engine(std::sync::Arc::new(StubEngine {
            result: Ok(DiffExecResult::Match { max_abs_diff: 0.0 }),
        }));
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi {
                diff_exec: Some(DiffExecResult::Match { .. }),
                ..
            } => {}
            other => panic!("expected Multi Match, got {other:?}"),
        }
    }

    /// PMAT-486: an installed engine that errors propagates a hard
    /// `BackendError` — it must NOT be swallowed into `NotRun`.
    #[test]
    fn diff_exec_engine_error_is_a_hard_failure() {
        let backend = diff_exec_backend().with_diff_exec_engine(std::sync::Arc::new(StubEngine {
            result: Err("driver fault: CUDA_ERROR_LAUNCH_FAILED".into()),
        }));
        let err = backend
            .lower(&dummy_module(), &dummy_config())
            .expect_err("engine error must surface as a hard BackendError");
        assert!(matches!(err, BackendError::Lower(_)));
    }

    /// PMAT-486: with no engine installed, the policy still records the
    /// benign `NotRun { no-engine }` (free CI stays green).
    #[test]
    fn diff_exec_no_engine_records_not_run() {
        let backend = diff_exec_backend();
        let artifact = backend.lower(&dummy_module(), &dummy_config()).unwrap();
        match artifact.quorum_status {
            QuorumStatus::Multi {
                diff_exec: Some(DiffExecResult::NotRun { reason }),
                ..
            } => assert!(reason.contains("no DiffExec engine"), "got: {reason}"),
            other => panic!("expected Multi NotRun, got {other:?}"),
        }
    }
}
