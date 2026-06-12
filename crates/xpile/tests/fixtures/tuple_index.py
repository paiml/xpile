# PMAT-502q (Tranche 2): tuple constant-indexing t[N] -> Rust t.N field access.
def first(t: tuple[int, int]) -> int:
    return t[0]


def second(t: tuple[int, int]) -> int:
    return t[1]


def from_local(a: int, b: int) -> int:
    t = (a, b)
    return t[0] + t[1]
