from typing import Optional


def sum_present(threshold: int) -> int:
    # PMAT-892: a `list[Optional[int]]` literal mixing bare ints and None — was
    # rejected as a "heterogeneous list literal"; now each element is coerced
    # (bare int -> Some, None -> Option::None).
    xs: list[Optional[int]] = [5, None, threshold, None, 1]
    total: int = 0
    for x in xs:
        if x is not None:
            total += x
    return total


def count_none() -> int:
    xs: list[Optional[int]] = [1, None, 2, None, None]
    n: int = 0
    for x in xs:
        if x is None:
            n += 1
    return n
