def n_unique(xs: list[int]) -> int:
    return len({x for x in xs})


def n_pairs(xs: list[int]) -> int:
    return len({x: x * 2 for x in xs})


def n_positive_unique(xs: list[int]) -> int:
    return len({x for x in xs if x > 0})
