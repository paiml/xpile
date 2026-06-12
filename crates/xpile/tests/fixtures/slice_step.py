# PMAT-502bc (Tranche 2): general slice step xs[a:b:c] over a list (positive c).
def every_other(xs: list[int]) -> list[int]:
    return xs[::2]


def bounded_step(xs: list[int]) -> list[int]:
    return xs[1:8:3]


def from_one_step(xs: list[int]) -> list[int]:
    return xs[1::2]
