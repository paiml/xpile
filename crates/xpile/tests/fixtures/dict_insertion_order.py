# PMAT-874 (HUNT dict-order): Python dicts iterate in INSERTION order (guaranteed
# since 3.7). xpile emitted std::collections::HashMap → non-deterministic iteration
# (silent-wrong on any dict iteration/keys/values/items). Dicts now lower to
# indexmap::IndexMap (insertion-ordered; shift_remove preserves order on delete).
# Cross-checked vs python3.


def build_order() -> str:
    d: dict[str, int] = {}
    d["z"] = 1
    d["a"] = 2
    d["m"] = 3
    d["b"] = 4
    out: str = ""
    for k in d:
        out = out + k
    return out


def del_keeps_order() -> str:
    d: dict[str, int] = {}
    d["z"] = 1
    d["a"] = 2
    d["m"] = 3
    del d["a"]
    out: str = ""
    for k in d:
        out = out + k
    return out


def overwrite_keeps_pos() -> str:
    d: dict[str, int] = {}
    d["z"] = 1
    d["a"] = 2
    d["z"] = 9
    out: str = ""
    for k in d:
        out = out + k
    return out
