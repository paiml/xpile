# #2139: a Python call into a C `unsigned long long` boundary is CHECKED by
# `xpile hybrid --verify`, not skipped.
#
# There is no lossless `i64` bridge for it: ctypes' `c_ulonglong` returns
# Python ints above 2^63 (this fixture's second line is one), which no `i64`
# can hold. So plain `--verify` reports the emitted workspace's real E0308
# (the call lowers `big(7i64)` into a `u64` wrapper) and exits NON-ZERO, where
# it used to skip the boundary and exit 0. `--verify --repair` converges: the
# `ffi-arg-cast` rule casts the call site to `u64`, and the result is printed
# as a `u64`, so even the value above `i64::MAX` matches CPython.
from ._core import big


def main() -> None:
    print(big(7))
    print(big(9223372036854775807))
