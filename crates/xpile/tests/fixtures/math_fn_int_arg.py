# PMAT-783 (HUNT-V17 #12): a math.* builtin over an int arg emitted
# `(n).sqrt()` / `(n).floor()`, which an i64 doesn't have (rustc E0599). Python
# widens int→float into these (`math.sqrt(16)` == 4.0, `math.floor(5)` == 5).
# The math-call lowering now coerces an int/bool arg to f64; a float arg is
# unchanged. Cross-checked vs python3.
import math


def root(n: int) -> float:
    return math.sqrt(n)


def floor_int(n: int) -> int:
    return math.floor(n)


def root_float(x: float) -> float:
    return math.sqrt(x)
