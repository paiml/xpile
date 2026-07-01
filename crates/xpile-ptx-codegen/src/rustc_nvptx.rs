//! PMAT-480/PMAT-997 — the THIRD categorically-independent PTX emitter for the
//! §29 Multi-Emitter Oracle Quorum: **nightly `rustc`'s built-in
//! `nvptx64-nvidia-cuda` target** (Rust → rustc MIR → modern LLVM → LLVM NVPTX
//! back-end → PTX).
//!
//! ## Why this is a genuinely INDEPENDENT emitter (§14.10 anti-correlation)
//!
//! The existing §29 PTX pair already shares NO codegen front-end:
//!   - **general** — xpile's OWN hand-emitted PTX *text* ([`crate::emit_kernel`]);
//!   - **specialist** — the `nvcc`-compiled CUDA-C kernel (C++ front-end → NVVM
//!     = LLVM 7 → ptxas).
//!
//! This adds a THIRD path that fails DIFFERENTLY than both: a real Rust source
//! compiled by nightly `rustc` through **modern LLVM's NVPTX back-end**. Three
//! independent toolchains (hand-written text vs an LLVM-7/NVVM C++ front-end vs
//! a modern-LLVM Rust front-end) computing the SAME `out[i] = 2*in[i] + 1`
//! kernel and agreeing on the GPU is a far stronger anti-correlation witness
//! than any pair — a miscompile would have to corrupt all three identically.
//!
//! ## Toolchain posture — external subprocess, NOT a nightly build lane
//!
//! `rustc` is invoked as an EXTERNAL subprocess (exactly like `nvcc` / `ptxas` /
//! `wat2wasm`), so **xpile itself keeps building on stable** — there is NO
//! nightly requirement for the crate, NO `rustc-dev` component (that is for
//! codegen backends like `rustc_codegen_nvvm`; the built-in `nvptx64` target
//! needs only the target's `rust-std`), and NO new Cargo dependency (so
//! `cargo deny check advisories` is unaffected). The `cuda-oxide` framing in the
//! roadmap (PMAT-480) is superseded: the built-in `nvptx64-nvidia-cuda` target
//! IS the pure-Rust→PTX path, and it is lighter (no rustc-dev, no crate).
//!
//! Gated on [`rustc_nvptx_available`] — absent on a box without nightly + the
//! `nvptx64-nvidia-cuda` target → the witness cleanly skips (never a false
//! green), the same graceful-skip discipline as the `nvcc` / WABT witnesses.

use std::process::Command;
use std::sync::OnceLock;

/// The kernel name the §29 harness looks up (`cuModuleGetFunction`).
pub const KERNEL_NAME: &str = "xpile_kernel";

/// The Rust kernel source compiled to PTX by nightly `rustc`'s NVPTX back-end.
///
/// It computes the SAME element-wise `out[i] = 2*in[i] + 1` kernel the xpile
/// hand-emitter and the `nvcc` CUDA-C specialist compute, with the SAME ABI the
/// §29 driver harness launches — `xpile_kernel(in: *const f64, out: *mut f64,
/// n: i32)` over a `blockIdx.x*blockDim.x + threadIdx.x` global index with an
/// `i < n` guard. `#![no_std]` + a trivial panic handler; the `ptx-kernel` ABI
/// marks it a `.visible .entry`.
pub const RUSTC_NVPTX_KERNEL_SRC: &str = r#"#![no_std]
#![feature(abi_ptx, stdarch_nvptx)]
#![no_main]
use core::arch::nvptx;

#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// out[i] = 2*in[i] + 1  (the §29 anti-correlation kernel), same ABI as the
/// xpile hand-emitted / nvcc CUDA-C `xpile_kernel`.
#[no_mangle]
pub unsafe extern "ptx-kernel" fn xpile_kernel(inp: *const f64, out: *mut f64, n: i32) {
    let i = (nvptx::_block_idx_x() * nvptx::_block_dim_x() + nvptx::_thread_idx_x()) as isize;
    if (i as i32) < n {
        *out.offset(i) = 2.0 * *inp.offset(i) + 1.0;
    }
}
"#;

/// The rustc target triple for NVIDIA PTX (LLVM NVPTX back-end).
pub const NVPTX_TARGET: &str = "nvptx64-nvidia-cuda";

/// Cached one-shot probe: compile [`RUSTC_NVPTX_KERNEL_SRC`] to PTX ONCE per
/// process. Both [`rustc_nvptx_available`] and [`emit_rustc_nvptx_ptx`] read it.
///
/// The probe is a REAL compile — not the cheaper "does `rustc +nightly --print
/// target-list` list the target?" check, which is a false-positive: the
/// `nvptx64-nvidia-cuda` target is ALWAYS listed (it is a known triple), yet its
/// `rust-std` may be ABSENT (e.g. on CI without `rustup target add
/// nvptx64-nvidia-cuda`), in which case the compile fails `can't find crate for
/// core`. So "available" MUST mean "the toolchain actually produced PTX here",
/// or the gated tests panic on CI instead of skipping.
fn probe() -> &'static Result<String, String> {
    static CACHE: OnceLock<Result<String, String>> = OnceLock::new();
    CACHE.get_or_init(compile_rustc_nvptx_ptx)
}

/// The actual `rustc +nightly --target nvptx64-nvidia-cuda` compile of
/// [`RUSTC_NVPTX_KERNEL_SRC`] → PTX text. `Err(reason)` if nightly / the target
/// `rust-std` is missing or the compile fails.
fn compile_rustc_nvptx_ptx() -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!("xpile-rustc-nvptx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create work dir: {e}"))?;
    let src = dir.join("kernel.rs");
    let ptx = dir.join("kernel.ptx");
    std::fs::write(&src, RUSTC_NVPTX_KERNEL_SRC).map_err(|e| format!("write kernel src: {e}"))?;

    let out = Command::new("rustc")
        .args([
            "+nightly",
            "--target",
            NVPTX_TARGET,
            "--crate-type",
            "cdylib",
            "-O",
        ])
        .arg(&src)
        .arg("-o")
        .arg(&ptx)
        .output()
        .map_err(|e| format!("spawn rustc +nightly: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "rustc +nightly --target {NVPTX_TARGET} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let text = std::fs::read_to_string(&ptx).map_err(|e| format!("read emitted PTX: {e}"))?;
    if !text.contains(&format!(".entry {KERNEL_NAME}")) {
        return Err(format!(
            "emitted PTX lacks `.entry {KERNEL_NAME}` — unexpected NVPTX output:\n{text}"
        ));
    }
    Ok(text)
}

/// `true` when nightly `rustc`'s `nvptx64-nvidia-cuda` target can ACTUALLY
/// compile a Rust kernel to PTX on this box (a cached real compile — never just
/// "the target is listed", which is always true even with the `rust-std`
/// absent). Absent → the gated witnesses cleanly skip.
pub fn rustc_nvptx_available() -> bool {
    probe().is_ok()
}

/// The rustc-nvptx PTX (from the cached [`probe`]). `Err(reason)` if the
/// toolchain / target `rust-std` is missing or the compile failed (the witness
/// turns that into a clean skip).
pub fn emit_rustc_nvptx_ptx() -> Result<String, String> {
    probe().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_src_declares_the_ptx_entry_abi() {
        // CONSTRUCT-time invariants that hold with or without the toolchain: the
        // kernel source names `xpile_kernel`, uses the `ptx-kernel` ABI, and
        // computes 2*in+1 with the in/out/n ABI the §29 harness launches.
        assert!(RUSTC_NVPTX_KERNEL_SRC.contains("extern \"ptx-kernel\" fn xpile_kernel"));
        assert!(RUSTC_NVPTX_KERNEL_SRC.contains("2.0 * *inp.offset(i) + 1.0"));
        assert!(RUSTC_NVPTX_KERNEL_SRC.contains("inp: *const f64, out: *mut f64, n: i32"));
    }

    #[test]
    fn emits_real_nvptx_ptx_when_toolchain_present() {
        if !rustc_nvptx_available() {
            eprintln!(
                "PMAT-997: skipping rustc-nvptx emit — nightly rustc + \
                 {NVPTX_TARGET} target absent (install: rustup toolchain install \
                 nightly && rustup target add {NVPTX_TARGET} --toolchain nightly). \
                 The 3rd §29 emitter is the LLVM NVPTX back-end path; on a box \
                 with it, this emits a `.entry xpile_kernel` PTX module."
            );
            return;
        }
        let ptx = emit_rustc_nvptx_ptx().expect("nightly rustc emits NVPTX PTX");
        assert!(
            ptx.contains("Generated by LLVM NVPTX Back-End"),
            "PTX must come from the LLVM NVPTX back-end (independent of xpile \
             hand-emit + nvcc):\n{ptx}"
        );
        assert!(ptx.contains(&format!(".visible .entry {KERNEL_NAME}")));
    }
}
