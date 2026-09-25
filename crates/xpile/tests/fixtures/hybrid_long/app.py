# #2139: a Python call into a C `long long` boundary is CHECKED by
# `xpile hybrid --verify`, not skipped as "non-ABI-mappable" (the workspace
# always built; the skip meant nothing compared its output). ctypes'
# `c_longlong` and the shim's `i64` wrapper agree across the full range.
#
# #2150: a `bool` argument into the `long long` slot converts as ctypes does
# (`c_longlong(True)` is 1), so `triple(True)` is 3 and `triple(False)` is 0.
from ._core import triple


def main() -> None:
    print(triple(-7))
    print(triple(3074457345618258602))
    print(triple(True))
    print(triple(False))
