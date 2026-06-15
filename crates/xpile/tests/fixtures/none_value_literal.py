# PMAT-690: `None` in value positions over Optional[T]. Two common idioms that
# were rejected ("unsupported constant: Discriminant(0)"):
#   (1) an Optional accumulator `result: Optional[int] = None` + later `result = v`
#       (the bare reassignment wraps in Some);
#   (2) `x == None` / `x != None` (≡ is None / is not None for a singleton).
# (A ternary `v if c else None` branch is a separate follow-up.)
from typing import Optional


def first_even(xs: list[int]) -> Optional[int]:
    result: Optional[int] = None
    for x in xs:
        if x % 2 == 0:
            result = x
    return result


def is_unset(x: Optional[int]) -> bool:
    return x == None


def is_set(x: Optional[int]) -> bool:
    return x != None
