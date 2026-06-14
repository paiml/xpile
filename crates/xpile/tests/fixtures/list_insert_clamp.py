# PMAT-590: list.insert clamps out-of-range and negative indices to CPython
# `ins1` semantics (listobject.c) instead of panicking like raw Vec::insert.
#   i > len     -> clamp to len (append)
#   i < 0       -> normalize to len + i, clamp to 0 if still negative
def ins_oob(xs: list[int]) -> int:
    xs.insert(100, 88)
    return xs[3]


def ins_neg(xs: list[int]) -> int:
    xs.insert(-1, 77)
    return xs[2]


def ins_neg_far(xs: list[int]) -> int:
    xs.insert(-100, 5)
    return xs[0]


def ins_at_len(xs: list[int]) -> int:
    xs.insert(3, 9)
    return xs[3]
