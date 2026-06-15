# PMAT-687: Python's `for … else:` / `while … else:` — the `else` runs iff the
# loop completed WITHOUT `break`. Desugared via a `__brokeN` flag set before each
# break, then `if !__brokeN { else }`. Was a clean reject.
def find(xs: list[int], t: int) -> str:
    for x in xs:
        if x == t:
            return "found@" + str(x)
    else:
        return "not found"
    return "unreachable"


def has_break(xs: list[int], t: int) -> str:
    for x in xs:
        if x == t:
            break
    else:
        return "completed"
    return "broke"


def while_else(n: int) -> str:
    i = 0
    while i < n:
        if i == 3:
            break
        i += 1
    else:
        return "ran out"
    return "hit 3"
