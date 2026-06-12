# PMAT-502r (Tranche 2): open-ended slices xs[a:], xs[:b], xs[:] (list + str).
def head(xs: list[int], n: int) -> list[int]:
    return xs[:n]


def tail(xs: list[int], n: int) -> list[int]:
    return xs[n:]


def copy_all(xs: list[int]) -> list[int]:
    return xs[:]


def str_prefix(s: str, n: int) -> str:
    return s[:n]


def str_suffix(s: str, n: int) -> str:
    return s[n:]
