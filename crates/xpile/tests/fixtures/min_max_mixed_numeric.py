# PMAT-917 (HUNT BM-01 / V14-#9 / V16-#10): every function here mixes a `float`
# and an `int` argument to min()/max(). Python compares numerically but returns
# the WINNING OPERAND with its OWN type — `min(5.5, 2)` is the int `2` (prints
# `2`, not `2.0`), `max(7.5, 4)` is the float `7.5` — so the `-> float` annotation
# is NOT honoured at runtime. A single Rust numeric type cannot represent that, so
# xpile now REJECTS these mixed forms at lowering instead of silently widening to
# f64 (which Display-diverged and leaked the wrong type into arithmetic). This
# fixture exists only to drive that refusal — see
# `min_max_mixed_numeric_is_rejected_not_widened` in transpile_e2e.rs.
def lo(x: float, n: int) -> float:
    return min(x, n)


def hi(x: float, n: int) -> float:
    return max(x, n)


def lo_int_first(n: int, x: float) -> float:
    return min(n, x)


def clamp_hi(x: float, a: int, b: int) -> float:
    return max(x, a, b)
