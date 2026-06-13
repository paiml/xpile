# PMAT-502ca (Tranche 2): enumerate(xs, start) — the optional start index
# (int literal) offsets the index var. enumerate(xs) (start 0) is unchanged.
def weighted(xs: list[int]) -> int:
    t = 0
    for i, v in enumerate(xs, 1):
        t = t + i * v
    return t


def last_index(xs: list[int]) -> int:
    last = 0
    for i, v in enumerate(xs, 10):
        last = i
    return last
