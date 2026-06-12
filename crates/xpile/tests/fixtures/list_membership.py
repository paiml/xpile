# PMAT-502an (Tranche 2): list membership `x in xs` / `x not in xs`.
def has(xs: list[int], x: int) -> bool:
    return x in xs


def lacks(xs: list[int], x: int) -> bool:
    return x not in xs


def has_str(words: list[str], w: str) -> bool:
    return w in words
