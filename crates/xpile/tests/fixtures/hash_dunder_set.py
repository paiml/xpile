# PMAT-808 (HUNT-V22 HASH-01): a dataclass with a custom __hash__ used as a set
# element / dict key derived neither Hash nor Eq (and __hash__ was dead code) →
# rustc E0277/E0599. It now derives Eq + emits an impl Hash delegating to
# __hash__ (a custom __eq__ adds a hand `impl Eq`), so it works as a HashSet
# element / HashMap key. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Pt:
    x: int
    y: int

    def __hash__(self) -> int:
        return self.x * 31 + self.y


@dataclass
class Key:
    a: int
    b: int

    def __eq__(self, other: "Key") -> bool:
        return self.a == other.a

    def __hash__(self) -> int:
        return self.a


def set_dedup() -> int:
    s = set()
    s.add(Pt(1, 2))
    s.add(Pt(1, 2))
    s.add(Pt(3, 4))
    return len(s)


def dict_key() -> int:
    d: dict[Key, int] = {}
    d[Key(1, 9)] = 100
    d[Key(2, 8)] = 200
    return d[Key(1, 5)]
