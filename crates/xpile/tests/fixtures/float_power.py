# PMAT-502bt (Tranche 2): Python `**` with a float operand → float power
# `(a).powf(b)` (both operands cast to f64). Unlike int `**`, negative and
# fractional exponents are fine (`2.0 ** -1 == 0.5`, `9 ** 0.5 == 3.0`).
def square(x: float) -> float:
    return x ** 2


def powf(x: float, y: float) -> float:
    return x ** y


def root(n: int) -> float:
    return n ** 0.5
