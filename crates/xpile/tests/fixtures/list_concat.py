# PMAT-502bg (Tranche 2): list concatenation xs + ys.
def cat(a: list[int], b: list[int]) -> list[int]:
    return a + b


def cat_lit() -> list[int]:
    return [1, 2] + [3, 4]


def cat_len(a: list[int], b: list[int]) -> int:
    return len(a + b)
