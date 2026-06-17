# PMAT-751 (HUNT-V14 #5 bool-dict-key-int-keyed): indexing an INT-keyed dict
# with a bool (`d[True]`) — Python's `bool` is an `int` subtype and
# `hash(True) == hash(1)`, so `d[True]` is `d[1]`. xpile emitted `.get(&true)`
# over a `HashMap<i64, _>` → rustc E0308. The bool key is now coerced to i64 when
# the dict key type is int; a genuinely `dict[bool, V]` keeps its bool key.
# Cross-checked vs python3.


def idx_true() -> int:
    d: dict[int, int] = {1: 100, 0: 200}
    return d[True]


def idx_false() -> int:
    d: dict[int, int] = {1: 100, 0: 200}
    return d[False]


def bool_keyed(b: bool) -> int:
    # a dict actually keyed by bool keeps its bool key (no coercion)
    d: dict[bool, int] = {True: 5, False: 9}
    return d[b]
