# PMAT-790 (HUNT-V18 #8/#9/#10): abs(obj)/int(obj)/float(obj) over a user class
# that defines __abs__/__int__/__float__ emitted a generic free call
# (abs(v)/int(v)/float(v)) → rustc E0425 (no such free fn), where Python's
# builtins call the dunder. Each now dispatches to the method (mirror of the
# len()→__len__ dispatch). Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class V:
    n: int

    def __abs__(self) -> int:
        return self.n if self.n >= 0 else -self.n

    def __int__(self) -> int:
        return self.n

    def __float__(self) -> float:
        return float(self.n)


def use_abs() -> int:
    v = V(-7)
    return abs(v)


def use_int() -> int:
    v = V(42)
    return int(v)


def use_float() -> float:
    v = V(5)
    return float(v)
