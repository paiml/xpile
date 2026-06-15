# PMAT-683: a non-literal chained assignment `a = b = <expr>` over a Copy scalar
# (int/float/bool) now lowers — bound once to a temp, copied to each target.
def chain_int(n: int) -> int:
    a = b = n + 1
    return a + b


def chain_three(n: int) -> int:
    x = y = z = n * 2
    return x + y + z


def chain_float(p: float) -> float:
    a = b = p * 1.5
    return a + b


def chain_bool(n: int) -> bool:
    a = b = n > 0
    return a and b


def chain_literal(n: int) -> int:
    a = b = 5
    return a + b + n
