// PMAT-124 — Runtime witness for Rust → PTX kernel compilation.
//
// Provides a Runtime-stratum vote for:
//   C-COMPILE-RUST-TO-PTX-MMA
//
// Small Rust source carrying a `#[gpu_kernel(mma)]`-marked
// function designed to be lowered to PTX by a future
// xpile-ptx-codegen real-emission path
// (XPILE-COMPILE-PTX-RUNTIME-001). The fixture exercises the
// marker-preservation invariant the contract pins down: a kernel
// marked `#[gpu_kernel(mma)]` must lower to PTX containing at
// least one `mma.sync.aligned.*` instruction, never falling back
// to scalar `fma.rn`.
//
// xpile-ptx-codegen's emission body is scaffold at v0.1.0 — only
// the Layer-5 compile contract (PMAT-074 Lean theorem, PMAT-075
// Kani harness) and the marker-preservation byte-identity
// modelling are in place. The fixture sits ahead of the real
// emission so the contract can move from 3-stratum QUORUM to
// full 4-stratum. A dedicated round-trip test that lowers this
// kernel and greps the emitted PTX for `mma.sync.aligned` is
// XPILE-COMPILE-PTX-RUNTIME-001 future work.

#[gpu_kernel(mma)]
pub fn gemm_16x8x16_fp16(
    a: &[f16; 128],
    b: &[f16; 128],
    c: &mut [f16; 128],
) {
    // The body shape (triple-nested loop over a 16x8x16 tile) is
    // what xpile-ptx-codegen will lower to `mma.sync.aligned`
    // instructions. v0.1.0 codegen returns Unsupported; the
    // fixture is the future-test anchor.
    for i in 0..16 {
        for j in 0..8 {
            for k in 0..16 {
                c[i * 8 + j] += a[i * 16 + k] * b[k * 8 + j];
            }
        }
    }
}
