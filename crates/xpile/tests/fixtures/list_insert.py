# PMAT-502ar (Tranche 2): positional list insertion xs.insert(i, x).
def ins_mid(xs: list[int], x: int) -> int:
    xs.insert(1, x)
    return xs[1]


def ins_front(xs: list[int]) -> int:
    xs.insert(0, 99)
    return xs[0]


def ins_grows(xs: list[int]) -> int:
    xs.insert(1, 7)
    return len(xs)
