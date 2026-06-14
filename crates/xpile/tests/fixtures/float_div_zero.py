def fdiv(a: float, b: float) -> float:
    # Python raises ZeroDivisionError for float `/` by zero (not `inf`).
    return a / b


def ffloor(a: float, b: float) -> float:
    return a // b


def fmod(a: float, b: float) -> float:
    return a % b


def idiv(a: int, b: int) -> float:
    # int true-division also lowers to a float `/` and raises on zero.
    return a / b
