# PMAT-502ck (Tranche 2): for-loops over a call iterable that lowers to a
# list — reversed(xs), sorted(xs), list(range(n)). Previously only range(...)
# calls and bare list names/literals were accepted as for-loop iterables.
def rev_fold(xs: list[int]) -> int:
    t = 0
    for x in reversed(xs):
        t = t * 10 + x
    return t


def sort_fold(xs: list[int]) -> int:
    t = 0
    for x in sorted(xs):
        t = t * 10 + x
    return t


def range_sum(n: int) -> int:
    t = 0
    for x in list(range(n)):
        t = t + x
    return t
