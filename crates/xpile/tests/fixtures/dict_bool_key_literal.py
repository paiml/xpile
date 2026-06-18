# PMAT-787 (HUNT-V17 #24): a dict literal declared `dict[int, V]` with BOOL keys
# (`{True: 10, False: 20}`) emitted `insert(true, ...)` into a `HashMap<i64, V>`
# (rustc E0308), and a `{True: …, 2: …}` mix even rejected as "heterogeneous".
# Python's bool is an int subtype (hash(True)==hash(1)), so the keys are 1/0.
# The let-init type-threading now coerces a bool key to i64 when the declared
# key type is int; a genuine `dict[bool, V]` keeps its bool keys. Cross-checked
# vs python3.


def all_bool() -> int:
    d: dict[int, int] = {True: 10, False: 20}
    return d[1] + d[0]


def mixed_bool_int() -> int:
    d: dict[int, int] = {True: 10, 2: 30}
    return d[1] + d[2]


def genuine_bool_dict() -> int:
    d: dict[bool, int] = {True: 1, False: 0}
    if True in d:
        return d[True]
    return -1
