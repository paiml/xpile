import math


def hypotenuse(a: float, b: float) -> float:
    return math.hypot(a, b)


def angle(y: float, x: float) -> float:
    return math.atan2(y, x)


def log_base(x: float, base: float) -> float:
    return math.log(x, base)


def natural_log(x: float) -> float:
    # 1-arg math.log is still natural log (ln).
    return math.log(x)
