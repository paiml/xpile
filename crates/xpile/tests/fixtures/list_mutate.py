# PMAT-502ap (Tranche 2): in-place list mutators xs.sort()/.reverse()/.clear().
def first_sorted(xs: list[int]) -> int:
    xs.sort()
    return xs[0]


def first_reversed(xs: list[int]) -> int:
    xs.reverse()
    return xs[0]


def first_fsorted(xs: list[float]) -> float:
    xs.sort()
    return xs[0]


def cleared_len(xs: list[int]) -> int:
    xs.clear()
    return len(xs)
