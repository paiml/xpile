from typing import Optional


def maybe(n: int) -> Optional[int]:
    if n > 0:
        return n
    return None


def sum_present(xs: list[Optional[int]]) -> int:
    # PMAT-887: the for-loop var `x` (Optional) is Some-narrowed inside the
    # `if x is not None:` body, so `total += x` unwraps. Was E0308 (the loop var
    # is prescan-mutable, which used to disqualify narrowing).
    total: int = 0
    for x in xs:
        if x is not None:
            total += x
    return total


def sum_truthy(xs: list[Optional[int]]) -> int:
    # `if x:` truthiness over an Optional loop var also narrows.
    total: int = 0
    for x in xs:
        if x:
            total += x
    return total
