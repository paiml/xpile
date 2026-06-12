# PMAT-502p (Tranche 2): chained comparisons a OP b OP c -> (a OP b) and (b OP c).
def in_range(lo: int, x: int, hi: int) -> bool:
    return lo <= x <= hi


def strictly_increasing(a: int, b: int, c: int) -> bool:
    return a < b < c


def triple_eq(a: int, b: int, c: int) -> bool:
    return a == b == c
