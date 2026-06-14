import math


def isqrt_floor(n: int) -> int:
    return math.isqrt(n)


def is_perfect_square(n: int) -> bool:
    # common isqrt idiom: r*r == n iff n is a perfect square.
    r = math.isqrt(n)
    return r * r == n


def isqrt_big(n: int) -> int:
    return math.isqrt(n)
