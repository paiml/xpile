def sorted_by_second(ps: list[tuple[int, int]]) -> int:
    # key indexes a tuple element — p[1] must lower to `.1`, not `[1]`.
    return sorted(ps, key=lambda p: p[1])[0][0]


def max_by_second(ps: list[tuple[int, int]]) -> int:
    return max(ps, key=lambda p: p[1])[0]


def min_by_first(ps: list[tuple[int, int]]) -> int:
    return min(ps, key=lambda p: p[0])[1]


def sorted_desc_by_first(ps: list[tuple[int, int]]) -> int:
    return sorted(ps, key=lambda p: p[0], reverse=True)[0][0]
