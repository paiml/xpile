# #2145: the repair loop's convergence witness (re-pointed here by PMAT-2138).
#
# THE DEFECT IT CARRIES, on purpose. A Python `bool` passed to a C `int`
# boundary lowers as `inc(true)`, while the safe wrapper takes `i64`, so the
# emitted workspace fails E0308 and plain `xpile hybrid --verify` exits 1.
# `--verify --repair` converges: FfiArgCastRepair rewrites the call site to
# `inc(true as i64)`, which compiles and prints 2 and 4, matching CPython
# through a ctypes `c_int` binding (`c_int(True)` is 1).
#
# COUPLING, on purpose: the day a bool argument to an int boundary builds
# without repair, this fixture stops failing and hybrid_repair.rs's
# convergence tests go red. Re-point them then (see that file's docs).
from ._core import inc


def main() -> None:
    print(inc(True))
    print(inc(3))
