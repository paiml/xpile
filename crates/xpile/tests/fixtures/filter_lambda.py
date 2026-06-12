# PMAT-502ab (Tranche 2): filter(lambda p: pred, xs) -> materialized list of
# elements where the Bool predicate holds (order-preserving).
def positives(xs: list[int]) -> list[int]:
    return list(filter(lambda x: x > 0, xs))


def evens(xs: list[int]) -> list[int]:
    return list(filter(lambda x: x % 2 == 0, xs))


def nonempty(words: list[str]) -> list[str]:
    return list(filter(lambda w: len(w) > 0, words))
