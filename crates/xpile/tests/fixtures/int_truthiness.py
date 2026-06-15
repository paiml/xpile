# PMAT-661: int/float truthiness in if/while/elif conditions. Python treats a
# nonzero int/float as truthy; xpile required a Bool condition and rejected
# `if n:` / `if len(xs):` / `while n:`. The condition now coerces int → `n != 0`
# and float → `x != 0.0` (matching Python's float edges: -0.0 falsy, nan truthy).


def if_int(n: int) -> int:
    if n:
        return 1
    return 0


def if_len(xs: list[int]) -> int:
    if len(xs):
        return 1
    return 0


def while_int(n: int) -> int:
    total = 0
    while n:
        total = total + n
        n = n - 1
    return total


def if_float(x: float) -> int:
    if x:
        return 1
    return 0


def elif_int(n: int) -> int:
    if n > 100:
        return 2
    elif n:
        return 1
    return 0


def if_bool_regression(b: bool) -> int:
    if b:
        return 1
    return 0
