# PMAT-502az (Tranche 2): filtered dict + set comprehensions {... if cond}.
def pos_map(xs: list[int]) -> dict[int, int]:
    return {x: x * x for x in xs if x > 0}


def pos_set(xs: list[int]) -> set[int]:
    return {x for x in xs if x > 0}


def dc_assign(xs: list[int]) -> int:
    d = {x: x for x in xs if x > 5}
    return len(d)
