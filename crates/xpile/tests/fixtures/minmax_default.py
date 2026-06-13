def min_or_zero(xs: list[int]) -> int:
    return min(xs, default=0)


def max_or_neg1(xs: list[int]) -> int:
    return max(xs, default=-1)


def fmin_or(xs: list[float]) -> float:
    return min(xs, default=9.0)
