import math


def arrange(n: int, k: int) -> int:
    return math.perm(n, k)


def license_plates() -> int:
    # 10 distinct digits taken 3 at a time, order matters: 10*9*8.
    return math.perm(10, 3)


def all_of(n: int) -> int:
    # One-arg form: perm(n) == n! (lowers to factorial).
    return math.perm(n)


def out_of_range(n: int, k: int) -> int:
    # k > n  ->  0 (with non-negative args; negative args raise in Python).
    return math.perm(n, k)


def empty_pick(n: int) -> int:
    # perm(n, 0) == 1 (empty product).
    return math.perm(n, 0)
