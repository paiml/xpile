# PMAT-612: round(int, n) → int. Previously emitted a bare `round(x, n)` call
# → E0425 (cannot find function `round`). Python: n >= 0 is the identity (an int
# has no fractional part); n < 0 rounds to the nearest 10^(-n) using
# round-half-to-even (banker's rounding). round(12350, -2) == 12400,
# round(12250, -2) == 12200.
def to_hundred(x: int) -> int:
    return round(x, -2)


def keep(x: int) -> int:
    return round(x, 2)


def by_var(x: int, n: int) -> int:
    return round(x, n)
