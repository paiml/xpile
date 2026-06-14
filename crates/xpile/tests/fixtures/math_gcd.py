import math


def gcd2(a: int, b: int) -> int:
    return math.gcd(a, b)


def reduce_fraction(num: int, den: int) -> int:
    # common idiom: divide by the gcd to reduce.
    g = math.gcd(num, den)
    return num // g


def gcd_negative(a: int, b: int) -> int:
    # gcd is over absolute values.
    return math.gcd(a, b)
