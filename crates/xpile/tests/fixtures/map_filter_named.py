# PMAT-706: map/filter with a bare callable NAME — `list(map(len, xs))`,
# `list(filter(bool, xs))`, `list(filter(None, xs))`, `list(map(myfunc, xs))` —
# were rejected ("produces I64"); only the lambda form worked. Synthesizes
# `name(__x)` as the body (filter requires a Bool predicate; filter(None) keeps
# the truthy elements).
def lens(xs: list[str]) -> list[int]:
    return list(map(len, xs))


def strs(xs: list[int]) -> list[str]:
    return list(map(str, xs))


def keep_truthy(xs: list[int]) -> list[int]:
    return list(filter(bool, xs))


def keep_none(xs: list[str]) -> list[str]:
    return list(filter(None, xs))


def is_pos(n: int) -> bool:
    return n > 0


def keep_pos(xs: list[int]) -> list[int]:
    return list(filter(is_pos, xs))
