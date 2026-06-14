# PMAT-592: a @dataclass(frozen=True) is hashable in Python, so it may be used
# as a set element or dict key. The struct must derive Eq + Hash (a bare
# #[derive(Clone, Debug, PartialEq)] struct is rejected as a HashSet element /
# HashMap key — E0277/E0599).
from dataclasses import dataclass


@dataclass(frozen=True)
class Coord:
    x: int
    y: int


def count_unique() -> int:
    s = {Coord(1, 2), Coord(1, 2), Coord(3, 4)}
    return len(s)


def dict_key_lookup() -> int:
    d: dict[Coord, int] = {}
    d[Coord(1, 2)] = 100
    d[Coord(3, 4)] = 200
    return d[Coord(1, 2)]
