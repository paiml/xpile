# PMAT-819 (HUNT-V22 INDEX): a list index typing as a struct with __index__ was
# rejected ("only int indices supported"). Python calls i.__index__() to coerce
# an object index to an int; the index now dispatches to that method (mirrors
# the abs/len dunder dispatch), then flows through the normal int-index path.
# Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Idx:
    n: int

    def __index__(self) -> int:
        return self.n


def at(i: int) -> int:
    xs = [10, 20, 30, 40]
    return xs[Idx(i)]
