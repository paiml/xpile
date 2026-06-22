from typing import Optional


def sum_skip_none(xs: list[Optional[int]]) -> int:
    # PMAT-893: `if x is None: continue` guard inside a loop narrows x to Some for
    # the rest of the iteration (was E0308 — the loop var stayed Option<i64>).
    total: int = 0
    for x in xs:
        if x is None:
            continue
        total += x
    return total


def first_present_or_zero(xs: list[Optional[int]]) -> int:
    # guard with `break` after a non-None find.
    found: int = 0
    for x in xs:
        if x is None:
            continue
        found = x
        break
    return found
