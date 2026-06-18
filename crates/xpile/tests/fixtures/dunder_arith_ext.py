# PMAT-785 (HUNT-V17 #22/26/27): arithmetic/bitwise operators beyond +/-/* over
# a user class defining the matching dunder (`//`→__floordiv__, `%`→__mod__,
# `**`→__pow__, `<<`/`>>`→__lshift__/__rshift__, `&`/`|`/`^`→__and__/__or__/__xor__,
# `/`→__truediv__) emitted the int divmod / checked_pow / bit-op codegen on a
# struct (rustc E0599/E0308). The binop lowering now dispatches each to the user
# method (rhs cloned when reused). Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class M:
    v: int

    def __floordiv__(self, other: "M") -> int:
        return self.v // other.v

    def __mod__(self, other: "M") -> int:
        return self.v % other.v

    def __and__(self, other: "M") -> int:
        return self.v & other.v


@dataclass
class P:
    v: int

    def __pow__(self, other: "P") -> int:
        return self.v + other.v

    def __lshift__(self, other: "P") -> int:
        return self.v * other.v


def use_divmod_and() -> int:
    a = M(17)
    b = M(5)
    return (a // b) + (a % b) + (a & b)


def use_pow_shift() -> int:
    return (P(3) ** P(4)) + (P(3) << P(4))


def plain_int_arith(a: int, b: int) -> int:
    # plain int operators are unchanged (no struct operand)
    return (a // b) + (a % b) + (a & b)
