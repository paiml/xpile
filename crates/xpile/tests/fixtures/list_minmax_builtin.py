# PMAT-502e (Tranche 2): 1-arg min(xs)/max(xs) reduce an int list to its
# element. (The 2-arg min(a, b)/max(a, b) form is a separate NumBuiltin.)
def smallest(xs: list[int]) -> int:
    return min(xs)


def largest(xs: list[int]) -> int:
    return max(xs)


def span(xs: list[int]) -> int:
    return max(xs) - min(xs)
