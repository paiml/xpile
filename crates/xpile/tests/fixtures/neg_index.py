# PMAT-502s (Tranche 2): negative list index xs[-k] -> xs[len(xs) - k].
def last(xs: list[int]) -> int:
    return xs[-1]


def second_last(xs: list[int]) -> int:
    return xs[-2]


def sum_ends(xs: list[int]) -> int:
    return xs[0] + xs[-1]
