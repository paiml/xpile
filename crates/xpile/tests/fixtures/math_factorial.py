import math


def fact(n: int) -> int:
    return math.factorial(n)


def fact_zero(n: int) -> int:
    # factorial(0) == 1.
    return math.factorial(n)


def binomial(n: int, k: int) -> int:
    # n choose k = n! / (k! * (n-k)!) — exercises factorial in arithmetic.
    return math.factorial(n) // (math.factorial(k) * math.factorial(n - k))
