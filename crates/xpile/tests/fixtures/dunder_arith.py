# PMAT-768 (HUNT-V16 DD-05): an arithmetic operator over a user class defining
# the matching dunder (`obj1 + obj2` → __add__, `-` → __sub__, `*` → __mul__)
# emitted the i64 `(a).checked_add(b)` (rustc E0599 — a struct has no
# checked_add), where Python resolves `a + b` to `a.__add__(b)`. The binop
# lowering now dispatches to the dunder when the LHS is a struct that defines it.
# Plain int/float arithmetic is unchanged. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Money:
    cents: int

    def __add__(self, other: "Money") -> int:
        return self.cents + other.cents

    def __sub__(self, other: "Money") -> int:
        return self.cents - other.cents

    def __mul__(self, other: "Money") -> int:
        return self.cents * other.cents


def add_use() -> int:
    return Money(100) + Money(50)


def sub_use() -> int:
    return Money(100) - Money(30)


def mul_use() -> int:
    return Money(4) * Money(5)


def plain_int(a: int, b: int) -> int:
    return a + b
