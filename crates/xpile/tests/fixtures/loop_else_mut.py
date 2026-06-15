# PMAT-698: a variable bound BEFORE a for/while loop and REASSIGNED only in the
# loop's `else` clause must be emitted `let mut` — walk_counts previously missed
# the else clause (it recursed only into the loop body), so the binding was a
# plain `let` and the else-reassignment hit rustc E0384.
def find_or(xs: list[int], t: int) -> str:
    r = "found"
    for x in xs:
        if x == t:
            break
    else:
        r = "missing"
    return r


def scan(n: int) -> str:
    r = "ok"
    i = 0
    while i < n:
        if i == 3:
            break
        i += 1
    else:
        r = "exhausted"
    return r
