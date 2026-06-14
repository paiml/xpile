# PMAT-614: Python float `//` is CPython float_divmod (fmod-based), NOT
# (a / b).floor(). The naive floor over-rounds when a/b lands just below an
# integer in float repr (1.0 // 0.1 == 9.0, not 10.0) and mishandles infinite
# operands (inf // 2 == nan, -5.0 // inf == -1.0).
def floordiv(a: float, b: float) -> float:
    return a // b


def steps(total: float, step: float) -> float:
    return total // step
