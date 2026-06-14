# PMAT-598: `s = set()` (empty) infers its element type from the subsequent
# `.add(...)`. The empty set defaulted to HashSet<i64>, so adding a struct/str
# was an i64-vs-actual mismatch (E0308). The annotation is now suppressed so
# rustc infers the element type from the insert.
from dataclasses import dataclass


@dataclass(frozen=True)
class Coord:
    x: int
    y: int


def count_unique_coords() -> int:
    s = set()
    s.add(Coord(1, 2))
    s.add(Coord(1, 2))
    s.add(Coord(3, 4))
    return len(s)


def count_unique_words() -> int:
    s = set()
    s.add("a")
    s.add("a")
    s.add("b")
    return len(s)


def count_ints() -> int:
    s = set()
    s.add(1)
    s.add(1)
    s.add(2)
    return len(s)
