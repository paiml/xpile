# PMAT-502m (Tranche 2): numeric conversions int(x) / float(x).
# int() truncates toward zero (like Python); float() widens int -> f64.
def to_float(n: int) -> float:
    return float(n)


def to_int(x: float) -> int:
    return int(x)


def half(n: int) -> float:
    return float(n) / 2.0
