# PMAT-812 (HUNT-V22 #10 CC-2): set()/frozenset() only accepted a list arg —
# set("abc") and set((1,2,3)) were rejected. Python iterates any iterable: a
# string yields its chars, a tuple its elements. The set ctor now materialises a
# string to its chars (StrChars) and a tuple literal to a list of its elements
# before SetFromList. Cross-checked vs python3.


def from_str() -> int:
    s = set("hello")
    return len(s)


def from_tuple() -> int:
    s = set((1, 2, 2, 3))
    return len(s)


def frozen_str() -> int:
    s = frozenset("aabbc")
    return len(s)
