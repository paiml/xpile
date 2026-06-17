# PMAT-759 (HUNT-V15 ONF-4): Optional flow-narrowing was not propagated through
# a compound `x is not None and <rest>` condition — `<rest>` (e.g. `x > 0`) kept
# x as Option<i64> (rustc E0308 comparing Option to int), and the then-body kept
# x un-narrowed (Option has no checked_add). The `is not None` conjunct now
# narrows x for the later `and` operands (Python/Rust both short-circuit) AND for
# the then-body. Cross-checked vs python3.
from typing import Optional


def guard_and_use(x: Optional[int]) -> int:
    if x is not None and x > 0:
        return x + 1
    return -1


def guard_chain(x: Optional[int]) -> int:
    if x is not None and x > 0 and x < 100:
        return x * 2
    return -1


def simple_still_works(x: Optional[int]) -> int:
    if x is not None:
        return x + 10
    return 0
