# PMAT-767 (HUNT-V16 DD-04): `obj[i]` over a user class defining `__getitem__`
# fell through to the list-index path (struct can't be indexed → rustc E0608),
# where Python calls `obj.__getitem__(i)`. The subscript-read lowering now
# dispatches to that method when the struct registers a `__getitem__`.
# Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Row:
    a: int
    b: int

    def __getitem__(self, i: int) -> int:
        if i == 0:
            return self.a
        return self.b


def use_index() -> int:
    r = Row(10, 20)
    return r[0] + r[1]


def index_var(i: int) -> int:
    r = Row(7, 8)
    return r[i]
