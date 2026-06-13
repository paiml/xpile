def is_positive_mag(n: int) -> bool:
    return abs(n) > 0


def max_le(a: int, b: int, c: int) -> bool:
    return max(a, b) <= c


def long_enough(s: str) -> bool:
    return len(s) > 3


def in_range(x: int) -> bool:
    return 0 < abs(x) < 10
