import math


def root(x: float) -> float:
    return math.sqrt(x)


def floor_of(x: float) -> int:
    return math.floor(x)


def ceil_of(x: float) -> int:
    return math.ceil(x)


def hypot(a: float, b: float) -> float:
    # math.sqrt composed in a larger expression.
    return math.sqrt(a * a + b * b)


def floor_neg(x: float) -> int:
    return math.floor(x)
