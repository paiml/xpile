# PMAT-504 (Tranche 2): first-class closure — lambda assigned to a local, then called.
def apply_twice(x: int) -> int:
    inc = lambda y: y + 1
    return inc(inc(x))


def is_positive(x: int) -> bool:
    pos = lambda y: y > 0
    return pos(x)


def scale(x: int) -> int:
    f = lambda y: y * 3
    return f(x)
