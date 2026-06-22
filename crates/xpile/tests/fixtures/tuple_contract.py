def swap(a: int, b: int) -> tuple[int, int]:
    return (b, a)


def first(p: tuple[int, int]) -> int:
    return p[0]


def labelled(n: int, s: str) -> tuple[int, str]:
    # per-element types preserved: (i64, String)
    return (n, s)


def sum_pair(p: tuple[int, int]) -> int:
    a, b = p
    return a + b
