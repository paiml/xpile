def lo(x: float, n: int) -> float:
    # mixed float/int min — both operands promoted to f64.
    return min(x, n)


def hi(x: float, n: int) -> float:
    return max(x, n)


def lo_int_first(n: int, x: float) -> float:
    # int operand first, float second.
    return min(n, x)


def clamp_hi(x: float, a: int, b: int) -> float:
    # 3-arg mixed max.
    return max(x, a, b)
