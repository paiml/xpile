# PMAT-617: Python bool is an int subtype, so comparing a bool with an int is
# valid (True == 1, True < 2). xpile emitted a bare `bool OP i64`, which rustc
# rejects (E0308). The bool side is now coerced to i64 — the comparison half of
# the bool-as-int story (PMAT-565 handled the arithmetic half). Works for the
# simple and chained forms (a chained `__cmpN` temp is coerced via its known type).
def beq(a: bool, b: int) -> bool:
    return a == b


def ieqb(a: int, b: bool) -> bool:
    return a == b


def blt(a: bool, b: int) -> bool:
    return a < b


def blit(flag: bool) -> bool:
    return flag == 1


def chained(a: bool, b: int, c: int) -> bool:
    return a <= b < c


def both_bool(a: bool, b: bool) -> bool:
    return a < b
