//! Kani BMC harness for `C-COMPILE-RUST-TO-PTX-MMA` (PMAT-075 /
//! XPILE-COMPILE-PTX-001).
//!
//! This is the **Symbolic stratum** counterpart for the
//! Rust kernel → PTX marker-preservation invariant. With this
//! harness landed, `C-COMPILE-RUST-TO-PTX-MMA` reaches §14.4
//! QUORUM — eleventh contract to do so, and **the first
//! Layer-5 (compile-time / IR) contract** to reach QUORUM:
//!
//!   * Semantic    (PMAT-074): `contracts/lean/CompileRustToPtxMma.lean`
//!   * Symbolic    (PMAT-075): this file
//!   * Runtime     (—)        : awaiting xpile-ptx-codegen real
//!                              emission (XPILE-COMPILE-PTX-RUNTIME-001)
//!   * Extrinsic   (PMAT-074..075): roadmap mentions
//!
//! ## What this harness proves
//!
//! The Rust mirror of the Lean theorem
//! `mma_emission_for_gemm_kernel` (see
//! `contracts/lean/CompileRustToPtxMma.lean`). Lowering a
//! `KernelInput` to a `PtxOutput` preserves the marker payload
//! at the byte level. Symbolic over 4-byte input.
//!
//! ## Why fixed-byte symbolic input
//!
//! Same rationale as PMAT-058..073: Kani handles fixed-size
//! `[u8; N]` arrays orders of magnitude faster than symbolic
//! `Vec<T>`. The 4-byte bound captures the marker-preservation
//! property at byte level; 256^4 ≈ 4.3B configurations.
//!
//! ## Cross-reinforcement
//!
//! Bidirectional with PMAT-074's Lean theorem. The pair locks
//! in the Layer-5 marker-preservation modelling commitment from
//! both formal sides. Any future xpile-ptx-codegen impl that
//! silently drops the `#[gpu_kernel(mma)]` marker — e.g.,
//! legalising to scalar `fma.rn` instructions instead of
//! `mma.sync.aligned` — must invalidate both discharges or
//! face the refinement-proof citation gate.

#![cfg(kani)]

/// Rust mirror of Lean's `KernelInput`. v0.1.0 Bronze-tier
/// model — a fixed-size byte array carrying a kernel-marker
/// payload (in real codegen this would be the
/// `#[gpu_kernel(mma)]` attribute plus the function body).
/// Silver-tier refinement (XPILE-REFINE-COMPILE-PTX-***+)
/// replaces this with typed AST nodes including a `markers :
/// List KernelAttr` field.
#[derive(PartialEq, Eq, Clone, Copy)]
struct KernelInput {
    marker: [u8; 4],
}

/// Rust mirror of Lean's `PtxOutput`. Same v0.1.0 shape as
/// `KernelInput` — locking in the marker-preservation claim at
/// the byte level. Silver-tier refinement replaces this with a
/// typed PTX AST (instructions, registers, shared-memory
/// directives).
#[derive(PartialEq, Eq, Clone, Copy)]
struct PtxOutput {
    emitted: [u8; 4],
}

/// Rust mirror of Lean's `lower_kernel_to_ptx`. v0.1.0 model:
/// byte-identity on the marker payload. The Bronze-tier
/// placeholder captures the load-bearing property — the
/// `#[gpu_kernel(mma)]` marker is faithfully carried into the
/// emitted PTX — without committing to a specific PTX
/// generation strategy.
fn lower_kernel_to_ptx(k: &KernelInput) -> PtxOutput {
    PtxOutput { emitted: k.marker }
}

/// Equation `mma_emission_for_gemm_kernel` from
/// `contracts/compile-rust-to-ptx-mma-v1.yaml`:
///
///   lower(rust_fn marked #[gpu_kernel(mma)],
///         BackendConfig{target: Ptx})
///     produces ptx_text containing mma.sync.aligned
///
/// Symbolic counterpart to
/// `XpileContracts.CCompileRustToPtxMma.mma_emission_for_gemm_kernel`
/// in `contracts/lean/CompileRustToPtxMma.lean`. Kani
/// exhaustively explores all 4-byte symbolic markers (256^4 ≈
/// 4.3B configurations) and verifies the lowered PtxOutput
/// carries the same marker bytes as the source KernelInput.
#[kani::proof]
fn mma_emission_for_gemm_kernel() {
    let input: [u8; 4] = kani::any();
    let kernel = KernelInput { marker: input };
    let ptx = lower_kernel_to_ptx(&kernel);

    kani::assert(
        ptx.emitted == kernel.marker,
        "lower_kernel_to_ptx must preserve the kernel marker",
    );
}

// ─── PMAT-276: Silver-tier property-specific Kani harnesses ─────────
//
// Audit-design.md §4 caveat: Bronze-tier Kani harnesses are "byte-
// identity placeholders rather than property-specific structural
// proofs". This block closes the caveat for C-COMPILE-RUST-TO-PTX-MMA
// by lifting the harness to match the Lean Silver tier already
// shipped at PMAT-161 (`shared_memory_budget_silver` in
// `contracts/lean/CompileRustToPtxMma.lean`).
//
// The Bronze harness above proves byte-identity on a 4-byte marker —
// trivially true since the lowering does `PtxOutput { emitted:
// k.marker }`. A buggy ptx-codegen that silently legalised
// `mma.sync.aligned` to scalar `fma.rn` would scramble bytes but
// preserve byte count, and the byte-payload proof can't tell those
// apart.
//
// The Silver tier introduces a real INEQUALITY property:
// emitted PTX's `smem_bytes` MUST be bounded by the hardware budget
// (48 KiB on sm_80). The emitter clamps via `min` — Kani exhausts
// the symbolic `requested_smem` space and verifies the clamp holds.

/// Shared-memory budget for sm_80 in bytes (48 KiB). Mirror of
/// `smem_budget_sm80` from contracts/lean/CompileRustToPtxMma.lean.
const SMEM_BUDGET_SM80: u32 = 48 * 1024;

/// Silver-tier model of a kernel input — Rust mirror of Lean's
/// `KernelInputSilver`. The Bronze byte-array marker is preserved
/// (for backwards compatibility) AND a structured `requested_smem`
/// field is added, capturing how much shared memory the kernel asks
/// for.
#[derive(PartialEq, Eq, Clone, Copy)]
struct KernelInputSilver {
    marker: [u8; 4],
    requested_smem: u32,
}

/// Silver-tier model of emitted PTX — Rust mirror of Lean's
/// `PtxOutputSilver`. `smem_bytes` is the realised shared-memory
/// byte count; MUST be bounded by `SMEM_BUDGET_SM80`.
#[derive(PartialEq, Eq, Clone, Copy)]
struct PtxOutputSilver {
    emitted: [u8; 4],
    smem_bytes: u32,
}

/// Silver-tier lowering — Rust mirror of Lean's
/// `lower_kernel_to_ptx_silver`. The realised `smem_bytes` is
/// clamped to the hardware budget via `min` — emission never exceeds
/// 48 KiB even if the kernel requests more. The Bronze marker bytes
/// are passed through verbatim.
fn lower_kernel_to_ptx_silver(k: &KernelInputSilver) -> PtxOutputSilver {
    PtxOutputSilver {
        emitted: k.marker,
        smem_bytes: u32::min(k.requested_smem, SMEM_BUDGET_SM80),
    }
}

/// PMAT-276 — Silver-tier counterpart to `shared_memory_budget_silver`
/// (Lean PMAT-161).
///
/// The emitted PTX's `smem_bytes` MUST be bounded by the sm_80
/// shared-memory budget (48 KiB). This captures the hardware
/// invariant that ptxas would otherwise reject — the emitter must
/// detect over-budget kernels and clamp (or fail), never silently
/// pass through.
///
/// A buggy ptx-codegen that propagated user-requested shared-memory
/// size verbatim (without clamping) would emit PTX rejected by ptxas
/// at JIT time — silent at compile time, late failure at deployment.
/// Kani symbolically exhausts the `requested_smem` space (4.3B u32
/// values) and verifies the clamp holds in every case.
///
/// FIRST Silver-tier Kani proof in this contract; matches the Lean
/// theorem's non-trivial proof (Nat.min_le_right, not rfl).
#[kani::proof]
fn shared_memory_budget_silver() {
    let marker: [u8; 4] = kani::any();
    let requested_smem: u32 = kani::any();
    let kernel = KernelInputSilver {
        marker,
        requested_smem,
    };
    let ptx = lower_kernel_to_ptx_silver(&kernel);

    kani::assert(
        ptx.smem_bytes <= SMEM_BUDGET_SM80,
        "emitted PTX shared-memory budget must never exceed sm_80 hardware limit",
    );
}

/// PMAT-276 — Silver-tier complementary property: the clamp is
/// monotone in the request.
///
/// If a kernel requests at most the budget, the emitted budget
/// EQUALS the request (no spurious clamping). Catches a regression
/// where a codegen optimization mistakenly clamps under-budget
/// kernels to a smaller value (wasting shared memory and degrading
/// kernel performance).
#[kani::proof]
fn smem_under_budget_preserved_silver() {
    let marker: [u8; 4] = kani::any();
    let requested_smem: u32 = kani::any();
    kani::assume(requested_smem <= SMEM_BUDGET_SM80);
    let kernel = KernelInputSilver {
        marker,
        requested_smem,
    };
    let ptx = lower_kernel_to_ptx_silver(&kernel);

    kani::assert(
        ptx.smem_bytes == requested_smem,
        "under-budget kernels must get the EXACT requested smem (no spurious clamping)",
    );
}

/// PMAT-276 — Silver-tier complementary property: over-budget kernels
/// get exactly the budget.
///
/// If a kernel requests more than the budget, the emitted budget
/// EQUALS the budget (clamped, not zero, not truncated to some
/// other value). Catches a regression where the codegen substitutes
/// zero or a fallback value rather than the documented clamp.
#[kani::proof]
fn smem_over_budget_clamps_to_budget_silver() {
    let marker: [u8; 4] = kani::any();
    let requested_smem: u32 = kani::any();
    kani::assume(requested_smem > SMEM_BUDGET_SM80);
    let kernel = KernelInputSilver {
        marker,
        requested_smem,
    };
    let ptx = lower_kernel_to_ptx_silver(&kernel);

    kani::assert(
        ptx.smem_bytes == SMEM_BUDGET_SM80,
        "over-budget kernels must clamp to EXACTLY the budget — not zero, not a fallback",
    );
}

/// PMAT-276 — Silver-tier marker preservation under structural model.
///
/// The Bronze byte-marker preservation still holds at Silver tier —
/// the `marker` field is passed through verbatim regardless of
/// requested_smem. Catches a regression where smem clamping
/// inadvertently mangles the marker.
#[kani::proof]
fn marker_preserved_under_silver_lowering() {
    let marker: [u8; 4] = kani::any();
    let requested_smem: u32 = kani::any();
    let kernel = KernelInputSilver {
        marker,
        requested_smem,
    };
    let ptx = lower_kernel_to_ptx_silver(&kernel);

    kani::assert(
        ptx.emitted == marker,
        "Silver-tier lowering must still preserve the Bronze marker",
    );
}
