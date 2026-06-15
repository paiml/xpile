# PMAT-640: a runtime-negative list-index WRITE wraps like Python (`xs[-1] = v`
# targets the last element), the assign-side companion to PMAT-639's read fix.
def set_at(xs: list[int], i: int, v: int) -> int:
    xs[i] = v
    return xs[2]


def aug_at(xs: list[int], i: int) -> int:
    xs[i] += 100
    return xs[2]


# RHS that reads the same list (borrow-then-mutate) still compiles + is correct.
def set_from_self(xs: list[int], i: int) -> int:
    xs[i] = xs[0] + 1
    return xs[2]


# Positive / literal writes are unchanged (regression guard).
def set_positive(xs: list[int]) -> int:
    xs[0] = 9
    return xs[0]
