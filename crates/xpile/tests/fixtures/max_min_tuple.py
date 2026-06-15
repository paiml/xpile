# PMAT-692: keyless max()/min() over a list of tuples — a tuple of Ord elements
# is itself Ord (Rust derives lexicographic Ord, matching Python's tuple compare).
# Was rejected ("produces I64"); sorted/max(key=) over the same list already work.
def maxpair(xs: list[tuple[int, int]]) -> tuple[int, int]:
    return max(xs)


def minpair(xs: list[tuple[int, int]]) -> tuple[int, int]:
    return min(xs)


def max_str_tuple(xs: list[tuple[str, int]]) -> tuple[str, int]:
    return max(xs)


def max_int_regression(xs: list[int]) -> int:
    return max(xs)
