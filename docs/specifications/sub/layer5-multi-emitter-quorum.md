# Layer-5 Multi-Emitter Oracle Quorum

**Section 29 of [xpile-spec.md](../xpile-spec.md).** Design for combining a general emitter (e.g., `rustc_codegen_nvvm` for PTX) with a specialist emitter (e.g., aprender-gpu's hand-tuned kernels) into a single contract-driven backend that produces an N-of-M oracle quorum at the Runtime stratum.

## Motivation

xpile's existing Layer-5 contracts (e.g., `C-COMPILE-RUST-TO-PTX-MMA`) have rich Semantic-stratum coverage:

- 4 Diamond theorems at PMAT-218/231/242/248 (bounded-monoid, join-semilattice, meet-semilattice, lattice absorption)
- 4 Platinum theorems on BoundedSmem composition
- Gold-tier BoundedSmem subtype encoding the sm_80 48 KiB budget at the type level
- Kani BMC harness verifying budget invariants symbolically

But the **Runtime stratum** is a single demo-fixture vote (`Run=1`), and `xpile-ptx-codegen` is currently a stub that produces placeholder text. The §14.4 quorum is met (4-stratum minimum) but barely — the Diamond proofs prove things about a `BoundedSmem` model, not about emitted PTX text.

Audit-design.md §4 (Fixture Overfitting) flags this directly: *"deeper Runtime witnesses (Gold tier) replace with property-specific diff_exec fixtures."*

## Design: a+b quorum

Rather than picking ONE emitter (rustc_codegen_nvvm OR aprender-gpu OR third-party reimplementation), xpile-ptx-codegen routes through BOTH and treats them as a §14.4 N-of-M oracle quorum at the Runtime stratum.

### Architecture

```rust
pub struct PtxBackend {
    /// General-purpose emitter — handles ANY #[gpu_kernel]-annotated
    /// Rust function. Currently rustc_codegen_nvvm (rust-cuda project).
    general: Box<dyn PtxEmitter>,

    /// Specialist emitter — hand-tuned kernels for specific shapes.
    /// Currently aprender-gpu (covers GEMM tensor ops). Optional —
    /// degrades gracefully to single-emitter when missing.
    specialist: Option<Box<dyn PtxEmitter>>,

    /// How to combine emitter outputs when both are available.
    quorum_policy: QuorumPolicy,
}

pub enum QuorumPolicy {
    /// If specialist handles the kernel, prefer it. Falls back to
    /// general otherwise. Single-vote Runtime stratum.
    PreferSpecialist,

    /// Emit via BOTH, run BOTH on test inputs, compare numerical
    /// outputs within tolerance. Multi-vote Runtime stratum.
    /// FALSIFIES the contract if outputs diverge.
    DiffExec { tolerance: f64 },

    /// Strict text-equality between PTX outputs (only useful for
    /// regression-locking, not for falsification — different valid
    /// PTX programs commonly produce identical outputs via different
    /// instruction sequences).
    Strict,
}

pub trait PtxEmitter: Send + Sync {
    fn name(&self) -> &'static str;

    /// Emit PTX for the given kernel. Returns None if this emitter
    /// can't handle the input shape (specialist with no matching
    /// kernel template).
    fn try_emit(&self, module: &Module, config: &BackendConfig)
        -> Option<Result<PtxArtifact, EmitterError>>;
}
```

### Output schema

When both emitters fire, the resulting `Artifact` carries two PTX sidecars:

```rust
Artifact {
    primary: general_ptx_text,
    sidecars: vec![
        Sidecar { name: "ptx_specialist", body: specialist_ptx_text },
        Sidecar { name: "diff_exec_report", body: comparison_json },
    ],
    citations: vec![
        ContractId::new("C-COMPILE-RUST-TO-PTX-MMA"),
        ContractId::new("aprender:C-COMPUTE-GEMM-FP16-MMA"),  // cross-repo
    ],
    quorum_status: QuorumStatus::Multi {
        strata: 2,
        emitters: ["rustc_codegen_nvvm", "aprender-gpu"],
        diff_exec: Some(DiffExecResult::Match { max_abs_diff: 1.3e-4 }),
    },
}
```

### Contract YAML schema extension

Extend `compile_targets.via` from a flat list of strings to a list of richer entries:

```yaml
compile_targets:
  - target: ptx
    hardware:
      compute_capability_min: "sm_80"
      compute_capability_max: "sm_90"
    via:
      - emitter: rustc_codegen_nvvm
        role: general
        crate: xpile-ptx-codegen
      - emitter: aprender-gpu
        role: specialist
        cross_repo: aprender
        shape_filter: "gemm_fp16_mma_64x128"  # matches aprender's contract
    quorum_policy:
      kind: DiffExec
      tolerance: 1.0e-3
      fail_on_divergence: true
```

`pv lint` extends to require that:
- At least one `role: general` emitter is declared (mandatory fallback)
- At most one `role: general`, but multiple `role: specialist` allowed
- `quorum_policy.kind` is one of `PreferSpecialist | DiffExec | Strict`
- `cross_repo: <name>` resolves against the fleet manifest

## Anti-correlation guarantee

The §14.4 quorum demands oracle independence — multiple oracles whose failure modes don't cluster. For PTX:

| Emitter | Code path | Failure modes |
|---|---|---|
| `rustc_codegen_nvvm` | meta-HIR → LLVM IR → NVVM IR → PTX (compiler) | LLVM lowering bug, NVVM JIT mismatch, register allocator regression, intrinsic mistranslation |
| `aprender-gpu` | shape-matched template instantiation (hand-tuned) | wrong cp.async pipeline depth, smem tile size miscount, mma operand permutation bug, fp16 rounding-mode mistake |

These failure surfaces are **categorically independent**. A divergence between them at runtime is high-signal — neither a coincidence nor a shared upstream bug.

## How this upgrades the substrate

Falsifies the *in-vacuum* nature of the existing Diamond proofs (PMAT-218/231/242/248):

| Diamond | What it currently proves | What a+b quorum adds |
|---|---|---|
| Bounded-monoid (218) | `BoundedSmem` model is well-formed under sum | **Both** emitters produce PTX with `.shared .align ... .b8 buf[N]` summing to ≤ 48 KiB |
| Join-semilattice (231) | `(BoundedSmem, max)` is a join-semilattice | When two kernels run in parallel and emitter must reserve max — both emitters agree on the max |
| Meet-semilattice (242) | `(BoundedSmem, min)` is a meet-semilattice | Safe over-subscription floor matches across emitters |
| Lattice absorption (248) | `max(a, min(a, b)) = a` | Held by emitted PTX bytecount under both paths |

The proofs are still about the model; the **gate that connects model to emitted PTX** is the diff_exec quorum.

## Phased implementation roadmap

| Phase | PMAT | Scope | Notes |
|---|---|---|---|
| Spec | PMAT-259 (this) | This document + xpile-spec.md §29 link | Documentation only |
| Schema | PMAT-260 | Extend `pv lint` to validate the new `compile_targets.via` shape | `compile_targets.via` from `[String]` to `[ViaEntry]` |
| General emitter | PMAT-261..N | rustc_codegen_nvvm path in xpile-ptx-codegen | Multi-PR — needs nvptx64 target, ptxas, libnvvm wiring |
| Specialist emitter | PMAT-26X..M | Cross-repo binding to aprender-gpu | Surface mapping `#[gpu_kernel(mma)]` → aprender's specialist registry |
| Quorum policy | PMAT-26Y | QuorumPolicy enum + DiffExec engine | `xpile transpile foo.rs --target ptx --quorum diff_exec` |
| Runtime upgrade | PMAT-26Z | `xpile quorum` reports `Run` count from diff_exec votes | Closes audit-design.md §4 "Run=1 demo fixture" caveat |

Each phase is independently shippable. Spec lands first so subsequent PRs have a stable contract.

## Generalization to other Layer-5 contracts

This pattern is **not PTX-specific**. The same shape applies to:

| Target | General emitter | Specialist emitter |
|---|---|---|
| **PTX** | `rustc_codegen_nvvm` | `aprender-gpu` (tensor kernels) |
| **WGSL** | `naga` (Mozilla, general WGSL) | `aprender-cuda-edge` for WebGPU compute tiles (if shipped) |
| **SPIR-V** | `rspirv` (general SPIR-V) | TBD specialist when domain emerges |
| **Shell / POSIX** | `bashrs-backend` (Layer-A general) | `bashrs-realistic` (corpus-tuned, 17k+ patterns) |
| **C extension** | `pyo3` (general FFI) | hand-tuned `cffi` templates for NumPy |

Each gets two independent emitters at the Runtime stratum, falsifies in-vacuum proofs from the Semantic stratum, and matches §14.4 N-of-M architecture.

## Pros vs alternatives

### vs (a) alone: rustc_codegen_nvvm only

| Dimension | a alone | a+b quorum |
|---|---|---|
| Runtime stratum votes | 1 (single emitter) | 2 (general + specialist) |
| Anti-correlation guard | None | Yes — categorically independent failure modes |
| Specialist-tuned correctness | Lost (general compiler may produce suboptimal PTX) | Preserved (aprender's tuned kernels are an oracle vote) |
| Falsifies in-vacuum Diamond proofs | No | Yes — divergence between emitters fails CI |
| Dependency surface | rustc_codegen_nvvm | rustc_codegen_nvvm + aprender-gpu (optional) |
| Implementation cost | Medium | Medium + cross-repo binding glue |

### vs (b) alone: aprender bridge only

| Dimension | b alone | a+b quorum |
|---|---|---|
| Coverage | Only shapes aprender's specialist registry covers | Any `#[gpu_kernel]` — general fallback for unknown shapes |
| xpile remains polyglot workbench | Compromised (aprender-shape-limited) | Yes |
| Reuses §14.4 quorum architecture | No (single oracle) | Yes |

### vs (c) drop PTX from xpile entirely

| Dimension | Drop PTX | a+b quorum |
|---|---|---|
| xpile-spec "polyglot workbench" claim | Compromised | Preserved |
| Contract C-COMPILE-RUST-TO-PTX-MMA | Becomes orphan or moves to aprender | Stays — gets real Runtime vote |
| Diamond proofs (PMAT-218/231/242/248) | Stay but become orphan-vacuum | Gain falsification gate |

## Falsification posture

A future contributor or refactor weakens the substrate if any of the following hold once the spec is implemented:

1. **A `#[gpu_kernel]`-annotated function transpiled to PTX produces text that, when executed, returns results inconsistent with the general+specialist quorum's combined output** — direct contract violation
2. **The specialist emitter is silently dropped from the build with no `quorum_policy: PreferSpecialist` annotation** — degrades multi-vote to single-vote without explicit policy choice
3. **The general emitter is removed**, breaking the fallback for unknown shapes — leaves xpile shape-limited
4. **`pv lint` is weakened to allow `compile_targets.via` without a `role: general` entry** — removes the mandatory fallback guarantee

`xpile quorum` will surface the multi-vote upgrade once implemented (`Run≥2` for contracts with quorum-emitter coverage).

## Cross-references

- **§14.4 N-of-M oracle quorum**: ruchy 5.0 §14 (`/home/noah/src/ruchy/docs/specifications/sub/provability-roadmap.md`)
- **Audit-design.md §4** (Fixture Overfitting): the "Run=1 demo fixture" caveat this design closes
- **`C-COMPILE-RUST-TO-PTX-MMA`**: `contracts/compile-rust-to-ptx-mma-v1.yaml`
- **Existing Diamond proofs**: PMAT-218/231/242/248 in `contracts/lean/CompileRustToPtxMma.lean`
- **Cross-repo binding pattern**: `sub/kaizen-fleet.md` "Cross-repo binding example"
- **bashrs general/specialist precedent**: `sub/bashrs-merger.md` — bashrs-frontend already operates with this duality at v0.1.0
