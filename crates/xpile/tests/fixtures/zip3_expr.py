# PMAT-707: 3-way zip(a, b, c) in EXPRESSION position — `list(zip(a, b, c))` —
# was rejected/mis-typed as I64 (only 2-way worked in expr position; 3-way only
# worked in a for-loop). Flattened via a nested Zip + a re-tupling Map → a flat
# list of 3-tuples.
def z3_len(a: list[int], b: list[int], c: list[int]) -> int:
    return len(list(zip(a, b, c)))


def z3_first(a: list[int], b: list[str], c: list[int]) -> str:
    pairs = list(zip(a, b, c))
    return pairs[0][1]


def z2_regression(a: list[int], b: list[int]) -> int:
    return len(list(zip(a, b)))
