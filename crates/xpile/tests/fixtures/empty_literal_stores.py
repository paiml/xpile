# PMAT-1041: EMPTY container literals stored into subscript slots —
# `d[k] = []` (THE grouping idiom's init step), `d[k] = {}`, `g[0] = []`,
# and the field path `self.groups[k] = []` — all refused at
# lower_expr_in_ctx ("requires a type annotation") though the SLOT's
# declared type fully determines the element type. The Subscript-target
# arm of lower_assign now lowers empty List/Dict literals directly to
# ListLit/DictLit (both emitters are inference-friendly: `vec![]` /
# `IndexMap::new()`, typed by the emitted insert/assign site). The
# setdefault alternative already worked; this closes the if-not-in-then-
# init grouping cluster (sweep-#10 residual, filed under PMAT-1039).
# Differentially verified vs CPython (MATCH 23/2/19/2).
class Grouper:
    groups: dict[str, list[int]]

    def __init__(self) -> None:
        self.groups = {}

    def ensure(self, k: str) -> None:
        self.groups[k] = []


def group_by_len() -> int:
    d: dict[int, list[str]] = {}
    words = ["a", "bb", "cc", "ddd"]
    for w in words:
        k = len(w)
        if k not in d:
            d[k] = []
        d[k].append(w)
    return len(d[2]) * 10 + len(d)


def dict_into_dict() -> int:
    d: dict[str, dict[str, int]] = {}
    d["a"] = {}
    d["a"]["x"] = 1
    d["a"]["y"] = 2
    return len(d["a"])


def reset_slot() -> int:
    d: dict[str, list[int]] = {"a": [1, 2, 3]}
    d["a"] = []
    d["a"].append(9)
    return len(d["a"]) * 10 + d["a"][0]


def field_slot() -> int:
    g = Grouper()
    g.ensure("a")
    g.ensure("b")
    return len(g.groups)
