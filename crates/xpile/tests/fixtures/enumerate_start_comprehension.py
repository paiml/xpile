# PMAT-684: `enumerate(xs, start)` / `enumerate(xs, start=N)` inside a list
# comprehension was rejected (only the for-loop form handled start). The index
# is offset by `start` (matches python3; negative start allowed).
def numbered(xs: list[str]) -> list[str]:
    return [str(i) + ". " + x for i, x in enumerate(xs, 1)]


def kw_start(xs: list[str]) -> list[str]:
    return [str(i) + ":" + x for i, x in enumerate(xs, start=10)]


def no_start(xs: list[str]) -> list[int]:
    return [i for i, x in enumerate(xs)]


def neg_start(xs: list[int]) -> list[int]:
    return [i + v for i, v in enumerate(xs, -1)]
