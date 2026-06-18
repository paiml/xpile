# PMAT-766 (HUNT-V16 DD-03): `len(obj)` over a user class defining `__len__`
# emitted `obj.len()` — but a user struct has no `.len()` method (rustc E0599).
# Python's `len()` calls `obj.__len__()`; the call now dispatches to that method
# when the struct registers a `__len__`. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Bag:
    n: int

    def __len__(self) -> int:
        return self.n


def use_len() -> int:
    b = Bag(5)
    return len(b)


def len_in_expr() -> int:
    b = Bag(3)
    return len(b) + 1
