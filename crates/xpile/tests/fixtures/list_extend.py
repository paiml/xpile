# PMAT-502aq (Tranche 2): in-place list concatenation xs.extend(ys).
def grow(xs: list[int], ys: list[int]) -> int:
    xs.extend(ys)
    return len(xs)


def grow_lit(xs: list[int]) -> int:
    xs.extend([4, 5])
    return xs[3]


def sum_after(xs: list[int], ys: list[int]) -> int:
    xs.extend(ys)
    total = 0
    for v in xs:
        total = total + v
    return total
