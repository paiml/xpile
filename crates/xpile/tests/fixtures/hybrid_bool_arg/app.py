# #2145: a Python `bool` or `int` argument to a C `int` / `double` boundary
# builds and agrees with CPython under plain `xpile hybrid --verify`.
#
# THE DEFECT THIS FIXTURE USED TO CARRY. The Python frontend lowers `inc(True)`
# as `inc(true)`, and `half(3)` as `half(3i64)`, while the safe wrappers take
# `i64` and `f64`, so the emitted workspace failed E0308 while
# `--emit-workspace` exited 0. PMAT-2138 made this fixture the repair loop's
# convergence witness for that reason.
#
# THE FIX. The hybrid workspace bridges every scalar slot with an adapter:
# an int slot takes `impl Into<i64>` and a double slot `impl IntoCDouble`,
# converting exactly as ctypes does (`c_int(True)` is 1, `c_double(3)` is 3.0,
# `c_double(True)` is 1.0). The lines below pin each argument kind into each
# slot kind. The convergence witness moved to hybrid_ulong; see
# hybrid_repair.rs.
from ._core import half, inc


def main() -> None:
    print(inc(True))
    print(inc(3))
    print(half(True))
    print(half(3))
    print(half(2.5))
