def at_abs(xs: list[int], i: int) -> int:
    return xs[abs(i)]


def at_clamped(xs: list[int], i: int) -> int:
    return xs[max(0, i)]


def neg_abs(n: int) -> int:
    return -abs(n)


def neg_max(a: int, b: int) -> int:
    return -max(a, b)
