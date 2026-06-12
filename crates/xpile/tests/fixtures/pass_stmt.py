# PMAT-502bn (Tranche 2): pass (no-op) statement.
def noop() -> None:
    pass


def guard_pass(x: int) -> int:
    if x < 0:
        pass
    return x + 1


def skip_first(xs: list[int]) -> int:
    t = 0
    for x in xs:
        if x == 0:
            pass
        else:
            t = t + x
    return t
