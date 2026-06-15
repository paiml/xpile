# PMAT-664: round(x) of a non-finite float raises in Python (OverflowError on
# inf, ValueError on nan), and returns a bigint for an out-of-i64 magnitude. A
# bare `as i64` cast saturated/garbage-cast silently. The emit now guards
# finiteness + i64 range (fails loud), mirroring the int()/math.floor guards.


def round_normal(x: float) -> int:
    return round(x)


def round_inf() -> int:
    return round(float("inf"))


def round_nan() -> int:
    return round(float("nan"))
