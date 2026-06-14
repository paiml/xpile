# PMAT-606: math.floor/ceil/trunc convert a float to a Python int. A bare
# `as i64` saturates (since Rust 1.45): a huge float clamps to i64::MAX, inf →
# i64::MAX, nan → 0 — but Python returns an exact bignum for a huge float and
# raises OverflowError(inf)/ValueError(nan). The rounded value is now guarded
# (finite + i64 range) and fails loud, like the int(float) cast.
import math


def fl(x: float) -> int:
    return math.floor(x)


def ce(x: float) -> int:
    return math.ceil(x)


def tr(x: float) -> int:
    return math.trunc(x)


def mkinf() -> float:
    return float("inf")
