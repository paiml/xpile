# PMAT-645: starred unpacking `n0, ..., *star = xs` (star LAST) over a list —
# the head/tail destructuring idiom. Desugars to `let n_i = xs[i]` + the rest as
# a slice `xs[p:]`.
def head_tail(xs: list[int]) -> int:
    a, *rest = xs
    return a * 100 + sum(rest)


def two_prefix(xs: list[int]) -> int:
    a, b, *rest = xs
    return a + b + len(rest)


def star_only(xs: list[int]) -> int:
    (*rest,) = xs
    return sum(rest)


# Source variable is not consumed (indexing borrows): xs is still usable.
def keeps_source(xs: list[int]) -> int:
    a, *rest = xs
    return a + len(rest) + len(xs)
