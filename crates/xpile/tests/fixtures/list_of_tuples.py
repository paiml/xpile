# PMAT-502cp (Tranche 2): tuple literals as list elements `[(1, 2), (3, 4)]`.
# A tuple literal in context-free position (list element) now lowers to a
# TupleLit, so list[tuple[...]] literals — and iterating them — work.
def make() -> list[tuple[int, int]]:
    return [(1, 2), (3, 4)]


def dot(pairs: list[tuple[int, int]]) -> int:
    t = 0
    for a, b in pairs:
        t = t + a * b
    return t
