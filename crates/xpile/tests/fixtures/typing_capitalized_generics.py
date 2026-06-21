# PMAT-864 (HUNT-V30 #10): the capitalized typing generics (List/Dict/Tuple/Set
# from `typing`) were rejected; only lowercase builtin generics were accepted.
# They are now normalized to the lowercase builtins (older Python style, very
# common). Cross-checked vs python3.
from typing import List, Dict, Tuple, Set


def total(xs: List[int]) -> int:
    s: int = 0
    for x in xs:
        s += x
    return s


def count(d: Dict[str, int]) -> int:
    return len(d)


def pair() -> Tuple[int, int]:
    return (1, 2)


def uniq(xs: List[int]) -> int:
    s: Set[int] = set(xs)
    return len(s)
