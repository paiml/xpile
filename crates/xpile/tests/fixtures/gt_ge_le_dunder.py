# PMAT-791 (HUNT-V18 #11): a class defining __gt__/__ge__/__le__ WITHOUT __lt__
# emitted raw Rust comparison operators over a PartialEq-only struct (rustc
# E0369 — no PartialOrd), where Python resolves all of </>/<=/>= via reflection.
# The PMAT-769 __lt__->PartialOrd synthesis now generalizes to the highest-priority
# order dunder the class defines (lt > gt > ge > le), driving all four operators
# from one consistent partial_cmp. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class P:
    n: int

    def __gt__(self, other: "P") -> bool:
        return self.n > other.n

    def __ge__(self, other: "P") -> bool:
        return self.n >= other.n

    def __le__(self, other: "P") -> bool:
        return self.n <= other.n


def gt() -> bool:
    return P(5) > P(3)


def lt() -> bool:
    return P(3) < P(5)


def ge() -> bool:
    return P(5) >= P(5)


def le() -> bool:
    return P(2) <= P(9)


def gt_false() -> bool:
    return P(1) > P(8)
