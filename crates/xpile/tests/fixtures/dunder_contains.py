# PMAT-771 (HUNT-V16 DD-08): `x in obj` over a user class defining
# `__contains__` was rejected ("unsupported comparison operator: In"), where
# Python's membership test calls `obj.__contains__(x)`. The In/NotIn comparison
# lowering now dispatches to that method (negated for `not in`) when the RHS is
# a struct that defines it. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Range2:
    lo: int
    hi: int

    def __contains__(self, x: int) -> bool:
        return self.lo <= x and x <= self.hi


def inside() -> int:
    return 1 if 5 in Range2(1, 10) else 0


def outside() -> int:
    return 1 if 50 in Range2(1, 10) else 0


def not_in() -> int:
    return 1 if 50 not in Range2(1, 10) else 0
