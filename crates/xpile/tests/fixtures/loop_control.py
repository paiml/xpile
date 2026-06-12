# PMAT-502bk (Tranche 2): continue / break in for-loops.
def sum_pos(xs: list[int]) -> int:
    t = 0
    for x in xs:
        if x < 0:
            continue
        t = t + x
    return t


def first_neg(xs: list[int]) -> int:
    found = 0
    for x in xs:
        if x < 0:
            found = x
            break
    return found


def sum_below_three(n: int) -> int:
    t = 0
    for i in range(n):
        if i == 3:
            break
        t = t + i
    return t
