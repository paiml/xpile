# PMAT-745 (HUNT-V13 intfloat-cmp-precision): Python compares an `int` and a
# `float` EXACTLY — it never rounds the int operand to f64. xpile previously
# cast the int side to f64 before comparing, which loses precision above 2^53
# (consecutive integers stop being distinct in f64) and could even INVERT the
# result: `9007199254740993 == 9007199254740992.0` wrongly became True, and
# `9007199254740993 > 9007199254740992.0` wrongly became False.
#
# The fix emits an exact comparison (compare `n as f64` for strict ordering,
# break the equality tie in i128). One node covers all six operators in either
# operand order. Cross-checked vs python3.


def eq_if(n: int, f: float) -> int:
    if n == f:
        return 1
    return 0


def ne_if(n: int, f: float) -> int:
    if n != f:
        return 1
    return 0


def lt_if(n: int, f: float) -> int:
    if n < f:
        return 1
    return 0


def gt_if(n: int, f: float) -> int:
    if n > f:
        return 1
    return 0


def ge_if(n: int, f: float) -> int:
    if n >= f:
        return 1
    return 0


# float on the LEFT — the operator is flipped during lowering so the int is the
# conceptual left operand (`f <= n` is emitted as `n >= f`).
def fle_n(f: float, n: int) -> int:
    if f <= n:
        return 1
    return 0
