# PMAT-2138 (was PMAT-1353's repair witness): a Python call into a C
# `unsigned int` boundary (meta-HIR `Type::CUInt`) must build and agree with
# CPython under plain `xpile hybrid --verify`.
#
# THE DEFECT THIS FIXTURE USED TO CARRY. The Python frontend lowers a boundary
# call before the C side is known and defaults an unknown callee to `i64`, so
# `bump(3)` became `bump(3i64)`. The PMAT-918 safe wrapper is
# `bump_shim(x: u32) -> u32`, so the emitted workspace failed E0308, and
# `--emit-workspace` exited 0 emitting it. PMAT-1353 made `--verify` report that
# failure instead of skipping it, and used this fixture as the one place the
# `--repair` loop converged on a real emitter symptom.
#
# THE FIX. The hybrid workspace now bridges a CUInt boundary with an adapter,
# `fn bump(a0: impl Into<i64>) -> i64 { ffi_shims::bump_shim(a0.into() as u32)
# as i64 }`, which casts exactly as ctypes' `c_uint` binding does. The lines below pin that
# equivalence where it can differ: an argument below zero and one above 2^32
# (both truncate mod 2^32), a result used in arithmetic (it must arrive as a
# Python int, not a u32), and a bool argument (lowered `true`, not `1i64`;
# ctypes' c_uint(True) is 1), which is why the adapter takes `impl Into<i64>`.
#
# WHAT THAT COST. This fixture stopped being the repair loop's convergence
# witness; hybrid_bool_arg was until #2145, and hybrid_ulong is now. See
# hybrid_repair.rs.
from ._core import bump


def main() -> None:
    print(bump(3))
    print(bump(-1))
    print(bump(4294967301))
    x = bump(41)
    print(x * 2 + 1)
    print(bump(True))
