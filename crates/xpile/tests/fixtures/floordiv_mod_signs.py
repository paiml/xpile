def fdiv(a: int, b: int) -> int:
    # Python floor division: rounds toward negative infinity (sign-aware).
    return a // b


def fmod(a: int, b: int) -> int:
    # Python modulo: result takes the sign of the divisor.
    return a % b


def clock(h: int) -> int:
    # the common positive-divisor case stays correct (regression guard).
    return h % 12
