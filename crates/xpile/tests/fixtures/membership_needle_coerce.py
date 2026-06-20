# PMAT-855 (HUNT-V28 #8): a bool needle into an int list — [1,2].count(True) /
# xs.index(True) — emitted `**__e == true` (i64 == bool) → rustc E0308. Python
# True==1, so the bool coerces to 1 (the membership `in` path already did this).
# count/index now coerce a bool needle to i64. Cross-checked vs python3.


def count_bool(xs: list[int]) -> int:
    return xs.count(True)


def index_bool(xs: list[int]) -> int:
    return xs.index(True)


def int_needle(xs: list[int]) -> int:
    return xs.count(2)


def bool_list(xs: list[bool]) -> int:
    return xs.count(True)
