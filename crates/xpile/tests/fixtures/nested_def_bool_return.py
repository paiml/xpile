# PMAT-780 (HUNT-V17 #5): a nested `def f(x: int) -> int: return x > 0` lowered
# to a closure whose body is bool while the registered return type said int, so
# `str(f(5))` rendered Rust's "true" (silent-wrong; Python "True") — even though
# the IDENTICAL code at top level cleanly rejects. The nested-def path now
# applies the same declared-return-vs-body-type check (fail-loud). Correct
# nested defs (matching annotations) are unaffected.
from dataclasses import dataclass


def correct_int(n: int) -> int:
    def dbl(x: int) -> int:
        return x * 2

    return dbl(n)


def correct_bool(n: int) -> int:
    def pos(x: int) -> bool:
        return x > 0

    if pos(n):
        return 1
    return 0
