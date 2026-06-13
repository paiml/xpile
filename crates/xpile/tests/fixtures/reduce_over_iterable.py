def sum_range(n: int) -> int:
    # sum(range(...)) — the textbook idiom.
    return sum(range(n))


def sum_range_from(a: int, b: int) -> int:
    return sum(range(a, b))


def sum_unique(xs: list[int]) -> int:
    # sum over a set (de-duplicated).
    return sum(set(xs))


def max_unique(xs: list[int]) -> int:
    return max(set(xs))


def min_unique(xs: list[int]) -> int:
    return min(set(xs))
