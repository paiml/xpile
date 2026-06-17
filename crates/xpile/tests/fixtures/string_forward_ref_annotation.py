# PMAT-758 (HUNT-V15 CME-2): a STRING forward-reference annotation (`-> "Counter"`,
# `x: "Counter"`) — idiomatic for a method returning its own class — was rejected
# ("non-trivial type expression"), and an unannotated classmethod defaulted to
# `()`, so `c = Counter.zero(); c.n` failed. A bare-identifier string annotation
# now resolves via the same name->Type mapping. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Counter:
    n: int

    @classmethod
    def zero(cls) -> "Counter":
        return Counter(0)

    @classmethod
    def of(cls, v: int) -> "Counter":
        return Counter(v)


def make_zero() -> int:
    c = Counter.zero()
    return c.n


def make_of() -> int:
    c = Counter.of(7)
    return c.n


def takes_fwdref(c: "Counter") -> int:
    # a string forward-ref in a PARAMETER position resolves too
    return c.n
