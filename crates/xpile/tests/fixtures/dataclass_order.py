# PMAT-648: @dataclass(order=True) generates ordering comparisons (fields as a
# tuple). The transpiler now derives PartialOrd so `Inst < Inst` etc. compile.
from dataclasses import dataclass


@dataclass(order=True)
class Point:
    x: int
    y: int


def less(a: int, b: int, c: int, d: int) -> int:
    return 1 if Point(a, b) < Point(c, d) else 0


def ge(a: int, b: int) -> int:
    return 1 if Point(a, 0) >= Point(b, 0) else 0


@dataclass(order=True)
class W:
    v: float


def float_less() -> int:
    return 1 if W(1.5) < W(2.5) else 0
