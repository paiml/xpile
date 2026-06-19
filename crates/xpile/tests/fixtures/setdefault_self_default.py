# PMAT-843 (HUNT-V27 #1): d.setdefault(k, EXPR) where EXPR reads d (d[x] / d.get /
# len(d)) emitted or_insert(EXPR) with EXPR borrowing d immutably while the entry()
# mutable borrow was live → rustc E0502. The default is now hoisted into a temp
# before .entry() (Python evaluates it eagerly anyway). Cross-checked vs python3.


def reads_dict() -> int:
    d: dict[str, int] = {"a": 5}
    v = d.setdefault("b", d["a"])
    return v + len(d)


def len_default() -> int:
    d: dict[str, int] = {"a": 1, "b": 2}
    v = d.setdefault("c", len(d))
    return v * 100 + len(d)


def present_key() -> int:
    d: dict[str, int] = {"a": 9}
    return d.setdefault("a", 99)


def literal_default() -> int:
    d: dict[str, int] = {}
    return d.setdefault("x", 5) + len(d)
