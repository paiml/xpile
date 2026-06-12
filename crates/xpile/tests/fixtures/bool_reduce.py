# PMAT-502j (Tranche 2): all(xs)/any(xs) over a list[bool].
def all_true(flags: list[bool]) -> bool:
    return all(flags)


def any_true(flags: list[bool]) -> bool:
    return any(flags)


def all_of_literals() -> bool:
    return all([True, True, False])
