# PMAT-868 (HUNT-V31 #1, narrow): a bool initializer in an EXPLICITLY int-annotated
# local (`n: int = True`) widens to i64 (Python's True is an int subtype). xpile
# emitted `let n: i64 = true` (rustc E0308). Done at the annotated-assign site
# where the annotation is explicit (the shared return path must not coerce a
# genuine bool). Cross-checked vs python3.


def from_true() -> int:
    n: int = True
    return n + 1


def from_false() -> int:
    n: int = False
    return n + 10


def from_compare(x: int) -> int:
    flag: int = x > 0
    return flag
