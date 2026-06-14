import math


def lcm2(a: int, b: int) -> int:
    return math.lcm(a, b)


def lcm_coprime(a: int, b: int) -> int:
    return math.lcm(a, b)


def lcm_zero(a: int, b: int) -> int:
    # lcm with a zero operand is 0.
    return math.lcm(a, b)


def lcm_negative(a: int, b: int) -> int:
    # lcm is over absolute values.
    return math.lcm(a, b)
