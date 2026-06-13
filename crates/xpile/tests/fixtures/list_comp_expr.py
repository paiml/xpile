def sum_squares(xs: list[int]) -> int:
    return sum([x * x for x in xs])


def max_abs(xs: list[int]) -> int:
    return max([abs(x) for x in xs])


def count_positive(xs: list[int]) -> int:
    return len([x for x in xs if x > 0])
