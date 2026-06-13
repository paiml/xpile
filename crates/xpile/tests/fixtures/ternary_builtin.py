def absval(n: int) -> int:
    return abs(-n) if n < 0 else n


def cap(a: int, b: int) -> int:
    return max(a, b) if a > 0 else b


def sq_or_zero(n: int) -> int:
    return pow(n, 2) if n > 0 else 0
