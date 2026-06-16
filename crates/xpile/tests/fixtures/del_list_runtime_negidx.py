# PMAT-712: `del xs[i]` with a runtime-NEGATIVE index emitted `xs.remove((i) as
# usize)`, which underflowed the negative to a huge index → panic. It now wraps
# like Python (`del xs[-1]` removes the last element). Positive indices and the
# literal-negative form (frontend-resolved to len-k) are unchanged.
def del_idx(xs: list[int], i: int) -> int:
    del xs[i]
    return xs[0] + xs[len(xs) - 1]


def del_pos(xs: list[int], i: int) -> int:
    del xs[i]
    return len(xs)
