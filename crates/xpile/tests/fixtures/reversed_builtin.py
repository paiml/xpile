# PMAT-502d (Tranche 2): reversed(xs) returns a new reversed list
# (input unchanged). The supported subset materializes Python's lazy
# `reversed` iterator as a Vec; `list(reversed(xs))` unwraps to the same.
def flip(xs: list[int]) -> list[int]:
    return reversed(xs)


def flip_str(words: list[str]) -> list[str]:
    return list(reversed(words))
