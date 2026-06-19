# PMAT-815 (HUNT-V22 DNI): a unary operator over a struct operand now dispatches
# to the user dunder — `-obj` → obj.__neg__(), `~obj` → obj.__invert__() — instead
# of being rejected ("unary `-` requires an I64 operand"). Mirrors the binop
# dunder dispatch. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class V:
    x: int

    def __neg__(self) -> "V":
        return V(-self.x)

    def __invert__(self) -> "V":
        return V(~self.x)


def probe() -> int:
    a = V(5)
    b = -a
    c = ~a
    return b.x * 100 + c.x


def plain_int(n: int) -> int:
    # regression: plain int unary ops unaffected
    return -n + ~n
