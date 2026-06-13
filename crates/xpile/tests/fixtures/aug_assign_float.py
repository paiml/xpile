# PMAT-502bq (Tranche 2): augmented assignment over a float.
# `+= -= *= /=` on a float lower to plain infix (FloatBinOp), not the
# i64-only checked_* path. `/=` (true division) works in aug position too.
def accum(x: float, y: float) -> float:
    x += y
    x -= 1.0
    x *= 2.0
    x /= 4.0
    return x


def scale_first(xs: list[float], v: float) -> float:
    xs[0] *= v
    return xs[0]
