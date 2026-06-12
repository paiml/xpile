# PMAT-502aa (Tranche 2): min(xs, key=lambda)/max(xs, key=lambda) ->
# min_by_key/max_by_key. Returns the element, not the key; element may be
# any type (only the key needs Ord).
def longest(words: list[str]) -> str:
    return max(words, key=lambda w: len(w))


def shortest(words: list[str]) -> str:
    return min(words, key=lambda w: len(w))


def closest_to_zero(xs: list[int]) -> int:
    return min(xs, key=lambda x: abs(x))
