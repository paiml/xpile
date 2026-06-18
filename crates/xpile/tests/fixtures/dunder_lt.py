# PMAT-769 (HUNT-V16 DD-07): a dataclass with a custom __lt__ emitted `a < b`
# over a struct deriving only PartialEq → rustc E0369 (no PartialOrd), with a
# dead __lt__ method. A generated `impl PartialOrd` now delegates to __lt__, so
# `<`/`>`/`<=`/`>=` use the user's ordering (compares only .x here).
# Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class P:
    x: int
    y: int

    def __lt__(self, other: "P") -> bool:
        return self.x < other.x


def lt() -> int:
    return 1 if P(1, 9) < P(2, 0) else 0


def gt() -> int:
    return 1 if P(3, 0) > P(2, 9) else 0


def le_equal_x() -> int:
    # (1,9) <= (1,0): not less (x equal) and not greater → equal → True
    return 1 if P(1, 9) <= P(1, 0) else 0
