import math


def power(b: float, e: float) -> float:
    return math.pow(b, e)


def power_int_args() -> float:
    # math.pow always returns float, even for int args (2**3 == 8.0).
    return math.pow(2, 3)


def power_expr(b: float, e: float) -> float:
    return math.pow(b, e) + 1.0


def trunc_pos(x: float) -> int:
    return math.trunc(x)


def trunc_neg(x: float) -> int:
    # trunc rounds toward zero, unlike floor.
    return math.trunc(x)
