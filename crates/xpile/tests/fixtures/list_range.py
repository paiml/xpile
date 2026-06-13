# PMAT-502cj (Tranche 2): list(range(...)) materialises a range into a list,
# and list(xs) copies a list. Positive literal step only (negative deferred).
def upto(n: int) -> list[int]:
    return list(range(n))


def span(a: int, b: int) -> list[int]:
    return list(range(a, b))


def evens(n: int) -> list[int]:
    return list(range(0, n, 2))


def copy(xs: list[int]) -> list[int]:
    return list(xs)
