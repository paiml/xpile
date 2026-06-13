# PMAT-502cw (Tranche 2): set(xs) materialises a list into a HashSet
# (de-duplicating). Previously only the empty set() was handled.
def uniq(xs: list[int]) -> set[int]:
    return set(xs)


def has(xs: list[int], x: int) -> bool:
    s = set(xs)
    return x in s
