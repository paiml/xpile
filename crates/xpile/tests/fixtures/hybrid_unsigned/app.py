# PMAT-1353: the fixture that makes `xpile hybrid --verify` reach a real,
# production-emitted BUILD FAILURE — and that `--verify --repair` converges on.
#
# THE DEFECT IT CARRIES. The boundary is a C `unsigned int` (meta-HIR
# `Type::CUInt`). `emit_c_shim` gets it right: PMAT-918 makes the safe wrapper
# `bump_shim(x: u32) -> u32`, preserving the signedness across the ABI. The
# PYTHON frontend does not, because it lowers a boundary call before the C side
# is known and defaults an unknown callee to `i64` — so the emitted `main.rs`
# calls `bump(3i64)` into a `u32` slot and `cargo build` fails with E0308. That
# is exactly the call-site retype hole PMAT-931 closed for `double`, still open
# in the UNSIGNED direction. `--emit-workspace` on this fixture exits 0 emitting
# a workspace that does not compile.
#
# AND THE SKIP THAT HID IT. Until PMAT-1353, `ctypes_name` had no `CUInt` arm, so
# `--verify` printed "boundary `bump` has a non-ABI-mappable type — skipping" and
# exited 0. The one check that would have caught the uncompilable emit declined
# to look at it. `unsigned int` <-> `ctypes.c_uint` is the canonical binding the
# shim already speaks, so widening the CHECKED set — not deciding any semantics —
# turns that disclosed green into the true red below.
#
# WHY IT IS THE REPAIR WITNESS. `Symptom::BuildError` carrying E0308 is precisely
# `FfiArgCastRepair`'s domain: it rewrites the call site to `bump(3i64 as u32)`,
# which compiles, runs, and prints 4 — byte-identical to CPython through a
# `c_uint`-bound ctypes call (verified both ways, not assumed). So this fixture
# is the one place the wired repair loop is observed to CONVERGE on a symptom the
# emitter really produces, rather than on an injected one.
#
# COUPLING, on purpose: the day the frontend retypes unsigned call sites the way
# PMAT-931 retyped float ones, this fixture stops failing to build and the two
# tests driving it go red LOUDLY. That is the correct prompt — a repair witness
# whose defect has been fixed must be re-pointed, not quietly kept green.
from ._core import bump


def main() -> None:
    print(bump(3))
