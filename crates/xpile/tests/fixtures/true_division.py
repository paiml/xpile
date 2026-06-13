# PMAT-502bs (Tranche 2): Python 3 true division `/` always yields a float,
# even for two int operands (7 / 2 == 3.5). Non-float operands are cast to
# f64; `//` floor-division stays integer.
def div(a: int, b: int) -> float:
    return a / b


def avg(a: int, b: int) -> float:
    return (a + b) / 2


def half(x: float) -> float:
    return x / 2
