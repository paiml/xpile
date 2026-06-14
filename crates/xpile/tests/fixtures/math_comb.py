import math


def choose(n: int, k: int) -> int:
    return math.comb(n, k)


def poker_hands() -> int:
    # 52 choose 5.
    return math.comb(52, 5)


def out_of_range(n: int, k: int) -> int:
    # k > n  ->  0 (with non-negative args; negative args raise in Python).
    return math.comb(n, k)


def symmetric(n: int) -> int:
    # comb(n, 0) + comb(n, n) == 2 (exercises the min(k, n-k) path).
    return math.comb(n, 0) + math.comb(n, n)
