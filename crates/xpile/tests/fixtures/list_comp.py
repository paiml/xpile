# PMAT-473 (R4): list comprehensions [elem for var in iter]. A
# comprehension is an expression but the meta-HIR has no block-
# expression, so it materialises to `tmp = []` + a for-append loop —
# in return position (hoisted to a temp) and assignment position.
def squares(xs: list[int]) -> list[int]:
    return [x * x for x in xs]


def doubled(xs: list[int]) -> list[int]:
    ys = [x + x for x in xs]
    return ys


def total_sq(xs: list[int]) -> int:
    sq = [x * x for x in xs]
    s = 0
    for v in sq:
        s += v
    return s
