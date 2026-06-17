# PMAT-752 companion: the frozen-mutation reject must NOT over-reject — reading
# a frozen dataclass field is fine, and a NON-frozen dataclass can still be
# mutated. Cross-checked vs python3 (5, 7).
from dataclasses import dataclass


@dataclass
class M:
    x: int


@dataclass(frozen=True)
class F:
    x: int


def mut_ok() -> int:
    m = M(1)
    m.x = 5  # non-frozen — mutation allowed
    return m.x


def frozen_read() -> int:
    f = F(7)
    return f.x  # reading a frozen field is fine
