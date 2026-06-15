# PMAT-636: `int ** <negative int literal>` is a float in Python (2 ** -1 == 0.5),
# not an integer. A negative-literal exponent takes the float-power path.
def half() -> float:
    return 2 ** -1  # 0.5


def milli() -> float:
    return 10 ** -3  # 0.001


def recip_sq(b: int) -> float:
    return b ** -2  # 1 / b**2


def sum_neg_powers() -> float:
    return 2 ** -1 + 2 ** -2  # 0.75


# Non-negative integer power stays an integer (regression guard).
def pos_power() -> int:
    return 2 ** 10  # 1024
