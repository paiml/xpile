# PMAT-500b (Tranche 2): set .add() mutation -> s.insert(x).
def has_after_add(extra: int, q: int) -> bool:
    s = {1, 2}
    s.add(extra)
    return q in s


def loop_contains(xs: list[int], q: int) -> bool:
    seen = {0}
    for x in xs:
        seen.add(x)
    return q in seen
