# PMAT-502bm (Tranche 2): early returns / guard clauses + terminal if/elif/else.
def sign(x: int) -> int:
    if x > 0:
        return 1
    elif x < 0:
        return -1
    else:
        return 0


def abs_val(x: int) -> int:
    if x >= 0:
        return x
    else:
        return -x


def guard(x: int) -> int:
    if x < 0:
        return 0
    return x + 1
