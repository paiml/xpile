# PMAT-502ai (Tranche 2): STANDALONE enumerate(xs)/zip(a, b) (not just the
# for-loop forms) -> materialized lists of tuples; compose with for-loop pair
# destructuring (PMAT-502y) and len (PMAT-502w).
def idx_sum(xs: list[int]) -> int:
    total = 0
    for i, x in list(enumerate(xs)):
        total = total + i * x
    return total


def dot(a: list[int], b: list[int]) -> int:
    total = 0
    for x, y in list(zip(a, b)):
        total = total + x * y
    return total


def n_pairs(a: list[int], b: list[int]) -> int:
    return len(list(zip(a, b)))
