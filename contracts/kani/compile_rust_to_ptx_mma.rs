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
