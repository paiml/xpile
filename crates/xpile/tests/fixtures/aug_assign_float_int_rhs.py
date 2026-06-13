# PMAT-502bu (Tranche 2): float augmented assignment with a non-float rhs.
# `x += 1`, `x /= 2`, `x **= 2` etc. on a float `x` must cast the int rhs to
# f64 (no `f64 <op> i64` mismatch), and `**=` must use powf (not int pow).
def run(x: float) -> float:
    x += 1
    x *= 3
    x /= 2
    x //= 2
    x %= 5
    x **= 2
    return x


def pow_assign(base: float) -> float:
    base **= 3
    return base
