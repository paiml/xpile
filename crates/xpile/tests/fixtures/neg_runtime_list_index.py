# PMAT-639: a runtime-negative list index wraps like Python (`xs[-1]` is the last
# element), not the `i as usize` underflow panic. Found by a differential hunt.
def at(xs: list[int], i: int) -> int:
    return xs[i]


def last_and_first(xs: list[int]) -> int:
    # computed indices: len-1 (positive) and -len (negative)
    return xs[len(xs) - 1] + xs[-len(xs)]


def neg_nested(grid: list[list[int]], i: int) -> int:
    return grid[i][0]


# Positive / literal indices are unchanged (regression guard).
def literal(xs: list[int]) -> int:
    return xs[0] + xs[2]
