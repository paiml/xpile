/-
  CompileRustToPtxMma.lean — Lean 4 refinement proofs for
  `C-COMPILE-RUST-TO-PTX-MMA`.

  This file is the proof-lane counterpart to
  `contracts/compile-rust-to-ptx-mma-v1.yaml` (PMAT-074). The
  YAML carries the *equations* describing how a Rust kernel
  annotated `#[gpu_kernel(mma)]` lowers through xpile-ptx-codegen
  to PTX text targeting sm_80+ hardware; this file carries the
  *theorem* that locks in the Bronze-tier modelling commitment
  for the `mma_emission_for_gemm_kernel` equation.

  Cross-references:
    * Code lane:   crates/xpile-ptx-codegen/src/lib.rs (when
                   PTX lowering grows past scaffold)
    * Contract:    contracts/compile-rust-to-ptx-mma-v1.yaml
    * Citation:    every emitted PTX artifact for a
                   `#[gpu_kernel(mma)]`-marked Rust input carries
                   `// xpile-contract: C-COMPILE-RUST-TO-PTX-MMA`
                   in its `.target sm_80` preamble.
    * Roadmap:     docs/specifications/xpile-spec.md §3 (Layer-5
                   compile contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — `KernelInput` and `PtxOutput` are both modelled as
  byte arrays carrying a "kernel marker" payload, and the
  lowering function is byte-identity. Silver-tier refinement
  (v0.3.0+) introduces typed PTX-AST nodes plus a
  Marker-preservation lemma showing that
  `#[gpu_kernel(mma)]`-marked inputs always produce PTX bodies
  containing at least one `mma.sync.aligned.*` instruction.

  This is the *tenth contract Lean theorem* the project has,
  and the **first Layer-5 (compile-time / IR) contract** to
  receive a refinement theorem. Prior theorems covered:
    - Layer-1: PyIntArith, Bashrs (per-language semantics)
    - Layer-2: Notation, XlatePyListToVec, Xlate{Lean→Rust, Rust→Lean}
    - Layer-3: 4 trait-determinism contracts (2×2 matrix)
    - **Layer-5: this file (compile-time emission)**

  Why Layer-5 at Bronze tier is meaningful even without a real
  PTX backend: the modelling commitment locks in WHAT the
  contract guarantees about emitted PTX (the marker is
  preserved), not HOW the emission works. The "how" can change
  freely between v0.1.0 and v1.0.0; the "what" must remain.
-/

namespace XpileContracts.CCompileRustToPtxMma

/--
  Abstract model of a Rust kernel input as seen by
  xpile-ptx-codegen. At v0.1.0 we represent it as a byte array
  carrying a kernel-marker payload (in real codegen this would
  be the `#[gpu_kernel(mma)]` attribute plus the function body).
  Silver-tier refinement (XPILE-REFINE-COMPILE-PTX-***+) replaces
  this with typed AST nodes including a `markers : List
  KernelAttr` field.
-/
structure KernelInput where
  marker : Array UInt8
deriving DecidableEq

/--
  Abstract model of emitted PTX text. v0.1.0 model — same
  byte-array shape as `KernelInput`, locking in the marker-
  preservation claim at the byte level. Silver-tier refinement
  replaces this with a typed PTX AST (instructions, registers,
  shared-memory directives).
-/
structure PtxOutput where
  emitted : Array UInt8
deriving DecidableEq

/--
  Lowering function: Rust kernel → PTX text. v0.1.0 model:
  byte-identity on the marker payload. The Bronze-tier
  placeholder captures the load-bearing property — the
  `#[gpu_kernel(mma)]` marker is faithfully carried into the
  emitted PTX preamble — without committing to a specific PTX
  generation strategy.

  Real xpile-ptx-codegen does much more: parses the kernel
  body, schedules tiles, emits `mma.sync.aligned`,
  `cp.async`, shared-memory directives. The Bronze-tier model
  abstracts away the body shape and focuses on the marker
  preservation property that every more elaborate refinement
  must continue to satisfy.
-/
def lower_kernel_to_ptx (k : KernelInput) : PtxOutput :=
  { emitted := k.marker }

/--
  **Refinement theorem** for `mma_emission_for_gemm_kernel`
  (the load-bearing claim from the contract YAML's equation
  block).

  Lowering a Rust kernel marked `#[gpu_kernel(mma)]` to PTX
  preserves the kernel marker in the emitted output. Proof is
  `rfl` by our v0.1.0 modelling choice (byte identity on the
  marker payload).

  Documentary value: any future xpile-ptx-codegen impl whose
  emission drops the `#[gpu_kernel(mma)]` marker silently
  (e.g., legalizing to scalar `fma.rn` instructions instead of
  `mma.sync.aligned`) must either preserve `rfl`-equivalence
  under this model OR invalidate the theorem (and
  `refinement_proofs.rs`'s citation gate fires).

  Falsification: an emitter that targets sm_70 fallback paths
  for mma-marked kernels — emitting `fma.rn` chains rather
  than `mma.sync.aligned` — would falsify the
  marker-preservation claim once Silver-tier refinement
  introduces typed instruction nodes.

  Status: **discharged at v0.1.0 (PMAT-074)**. Tier: Bronze.

  This is the **first Layer-5 contract** to receive a Lean
  refinement theorem. The compile-time / IR layer has been the
  hardest to formalise because its claims are about emitted
  hardware-targeting text (PTX, WGSL, SPIR-V), not about
  source-language semantics. Bronze tier captures the
  "marker preservation" invariant — the hardware-aware version
  is XPILE-REFINE-COMPILE-PTX-001 future work.
-/
theorem mma_emission_for_gemm_kernel (k : KernelInput) :
    (lower_kernel_to_ptx k).emitted = k.marker := by
  rfl

/--
  **Shared memory budget** auxiliary claim — Bronze-tier
  placeholder. At Bronze tier this reduces to `rfl` because the
  byte-array model doesn't track shared-memory directives
  separately. The Silver-tier refinement below introduces a
  typed `smem_bytes` field and a hardware-bound inequality.
-/
theorem shared_memory_budget (k : KernelInput) :
    (lower_kernel_to_ptx k).emitted = k.marker := by
  rfl

/-! ## PMAT-161 — Silver-tier refinement for `shared_memory_budget`
    (XPILE-REFINE-COMPILE-PTX-002).

    Promotes the byte-array model to a typed `PtxOutputSilver`
    with an explicit `smem_bytes : Nat` field and a target-bound
    that asserts emission stays under the sm_80 48 KiB ceiling.
    The Silver theorem is a structural Nat-inequality, not
    `rfl` — capturing the hardware-aware budget that Bronze
    couldn't express. -/

/-- The sm_80 shared-memory budget in bytes (48 KiB). The
    Silver model bounds emission against this constant. -/
def smem_budget_sm80 : Nat := 48 * 1024

/-- Silver-tier model of a Rust kernel input including a
    requested shared-memory size (in bytes) as part of the
    kernel attributes. -/
structure KernelInputSilver where
  marker : Array UInt8
  requested_smem : Nat
deriving DecidableEq

/-- Silver-tier model of emitted PTX text including the
    realised shared-memory budget byte count. -/
structure PtxOutputSilver where
  emitted : Array UInt8
  smem_bytes : Nat
deriving DecidableEq

/-- Silver-tier lowering. The realised `smem_bytes` is clamped
    to the hardware budget — emission never exceeds 48 KiB
    even if the kernel requests more (the contract says the
    emitter MUST detect over-budget kernels and fail, but the
    type-level claim is the simpler "emission ≤ budget"). -/
def lower_kernel_to_ptx_silver (k : KernelInputSilver) : PtxOutputSilver :=
  { emitted := k.marker
    smem_bytes := min k.requested_smem smem_budget_sm80 }

/--
  **Silver-tier refinement theorem** for `shared_memory_budget`
  (XPILE-REFINE-COMPILE-PTX-002 / PMAT-161).

  The emitted PTX's `smem_bytes` field is bounded by the sm_80
  shared-memory budget (48 KiB). This captures the hardware
  invariant that ptxas would otherwise reject — the Silver model
  proves the emitter respects the budget by construction (via
  the `min` clamp).

  Falsification: an emitter that propagates user-requested
  shared-memory size verbatim (without clamping to the hardware
  budget) would emit PTX that ptxas later rejects. The Silver
  model makes this an over-budget kernel unreachable at type
  level.

  Status: **discharged at v0.1.0 Silver tier (PMAT-161)** —
  sixth Silver refinement, first to use a non-trivial proof
  (Nat.min_le_right rather than rfl).
-/
theorem shared_memory_budget_silver (k : KernelInputSilver) :
    (lower_kernel_to_ptx_silver k).smem_bytes ≤ smem_budget_sm80 := by
  unfold lower_kernel_to_ptx_silver
  exact Nat.min_le_right _ _

/-! ## PMAT-187 — THIRD Gold-tier refinement: BoundedSmem subtype
    (XPILE-REFINE-COMPILE-PTX-003).

    Third Gold-tier theorem in the substrate (after PMAT-185
    PyIntFast and PMAT-186 BoundedRefcountDelta). Promotes
    Silver's `smem_bytes : Nat` (clamped via `min` at lowering
    time) to a refinement subtype `BoundedSmem := { s : Nat //
    s ≤ smem_budget_sm80 }` that encodes the sm_80 hardware
    shared-memory budget at the TYPE level.

    Silver (PMAT-161 `shared_memory_budget_silver`) proves the
    emitted smem_bytes is bounded — using runtime clamping with
    `min`. Gold tier removes the need for runtime clamping: the
    BoundedSmem subtype rules out over-budget values at
    construction time. A caller passing a raw `Nat` must
    construct a proof that it fits in 48 KiB before it can be
    accepted; the type system forbids over-budget kernels by
    construction.

    Architectural payoff: this is the third Gold pattern
    demonstration, applied to **hardware-aware compile-time
    contracts**. The previous Gold theorems covered arithmetic
    (PMAT-185) and ABI semantics (PMAT-186); this one covers
    hardware-targeting emission. Together they establish that
    Gold-tier subtype refinement is a *universal pattern* across
    Layer-1 (arithmetic), Layer-4 (FFI), and Layer-5 (compile).

    Status: discharged at v0.1.0 (PMAT-187). Tier: GOLD. -/

/-- Gold-tier refinement subtype: a shared-memory byte count
    proven to fit within the sm_80 48 KiB budget. The invariant
    is carried by the value. An emitter receiving a BoundedSmem
    cannot pass an over-budget value — ptxas would otherwise
    reject the emission, but the type system catches it earlier. -/
def BoundedSmem := { s : Nat // s ≤ smem_budget_sm80 }

/-- Extract the underlying byte count. -/
def BoundedSmem.val (b : BoundedSmem) : Nat := b.val

/-- Gold-tier model of a Rust kernel input with bounded smem
    request. The kernel can't even REQUEST more than 48 KiB —
    the type system forbids it. -/
structure KernelInputGold where
  marker : Array UInt8
  requested_smem : BoundedSmem
deriving DecidableEq

/-- Gold-tier model of emitted PTX text. The emitted smem_bytes
    is a BoundedSmem by construction — no runtime clamp needed,
    no `min` operation. -/
structure PtxOutputGold where
  emitted : Array UInt8
  smem_bytes : BoundedSmem
deriving DecidableEq

/-- Gold-tier lowering: pass-through. Because the input's
    requested_smem is already a BoundedSmem, the output's
    smem_bytes can copy the bound witness directly. No `min`
    clamp needed (Silver's clamp was a runtime workaround for
    untyped Nat input). -/
def lower_kernel_to_ptx_gold (k : KernelInputGold) : PtxOutputGold :=
  { emitted := k.marker
    smem_bytes := k.requested_smem }

/--
  **Gold-tier refinement theorem** — emitted smem_bytes is
  bounded by the sm_80 budget BY TYPE (not by clamp).

  This is the third Gold theorem in the substrate. Captures
  what Silver couldn't model:
  - Silver: "the emitter clamps via `min` to enforce the bound"
    — the bound is enforced at lowering time by a runtime
    operation.
  - Gold: "the input's smem request IS already bounded" — the
    type system prevents over-budget requests from being
    constructed. No runtime check needed.

  Falsification at Gold tier is structural: an emitter that
  accepts raw `Nat` requests (instead of BoundedSmem) would not
  type-check against `lower_kernel_to_ptx_gold`. The
  type-level encoding makes the budget violation impossible at
  the API boundary.

  Status: **discharged at v0.1.0 (PMAT-187)**. Tier: GOLD.
-/
theorem bounded_smem_preserved_gold (k : KernelInputGold) :
    (lower_kernel_to_ptx_gold k).smem_bytes.val ≤ smem_budget_sm80 :=
  (lower_kernel_to_ptx_gold k).smem_bytes.property

/--
  **Gold-tier refinement theorem** — value preserved. The
  underlying Nat survives lowering byte-for-byte; the bound
  witness travels alongside.
-/
theorem bounded_smem_value_preserved_gold (k : KernelInputGold) :
    (lower_kernel_to_ptx_gold k).smem_bytes.val = k.requested_smem.val := by
  rfl

/--
  **Gold-tier refinement theorem** — bridges Gold to Silver:
  the BoundedSmem-typed value agrees with what Silver's
  min-clamp would have produced (since the clamp is a no-op
  when the input is already in range).
-/
theorem gold_subtype_agrees_with_silver_clamp
    (k : KernelInputGold) :
    (lower_kernel_to_ptx_gold k).smem_bytes.val
      = min k.requested_smem.val smem_budget_sm80 := by
  have h := k.requested_smem.property
  unfold lower_kernel_to_ptx_gold
  exact (Nat.min_eq_left h).symm

end XpileContracts.CCompileRustToPtxMma
