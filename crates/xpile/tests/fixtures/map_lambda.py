# PMAT-502ac (Tranche 2): map(lambda p: e, xs) -> materialized list of the
# transformed elements; result element type = the body's type.
def doubled(xs: list[int]) -> list[int]:
    return list(map(lambda x: x * 2, xs))


def lengths(words: list[str]) -> list[int]:
    return list(map(lambda w: len(w), words))


def to_floats(xs: list[int]) -> list[float]:
    return list(map(lambda x: float(x), xs))
