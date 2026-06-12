# PMAT-502ba (Tranche 2): list comprehension over range(...).
def squares(n: int) -> list[int]:
    return [x * x for x in range(n)]


def odd_squares(n: int) -> list[int]:
    return [x * x for x in range(n) if x > 0]


def from_one(n: int) -> list[int]:
    return [x for x in range(1, n)]


def assign_form(n: int) -> int:
    ys = [x for x in range(n)]
    return len(ys)
