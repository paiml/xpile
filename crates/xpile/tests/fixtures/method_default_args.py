# PMAT-820 (HUNT-V24 MDA): a method/staticmethod default argument was dropped and
# an omitted-arg call site left under-supplied → rustc E0061. The free-function
# default-fill machinery existed but wasn't routed through method calls. Instance
# methods now register under a distinct Class#method signature key; both instance
# and static/classmethod call sites fill omitted trailing args with the defaults
# (coerced to the param type — int->f64 widening, Some-wrap). Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class C:
    base: int

    def m(self, x: int = 5, y: int = 10) -> int:
        return self.base + x + y


@dataclass
class Pt:
    x: int

    @staticmethod
    def make(a: int = 3, b: int = 4) -> int:
        return a * 10 + b


@dataclass
class Cfg:
    base: int

    def scaled(self, factor: float = 2.0) -> float:
        return self.base * factor


def methods() -> int:
    c = C(100)
    return c.m() + c.m(1) + c.m(1, 2)  # 115 + 111 + 103


def statics() -> int:
    return Pt.make() + Pt.make(7)  # 34 + 74


def float_default() -> float:
    c = Cfg(10)
    return c.scaled() + c.scaled(3.0)  # 20.0 + 30.0
