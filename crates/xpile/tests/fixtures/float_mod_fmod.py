# PMAT-591: Python float `%` is CPython `float_rem` — fmod(a, b) with the
# result adjusted to the divisor's sign, and copysign(0.0, b) for a zero
# remainder. The earlier floor formula `a - b*(a/b).floor()` introduced an
# extra rounding step (last-ULP divergence) and always produced +0.0.
def rem(a: float, b: float) -> float:
    return a % b
