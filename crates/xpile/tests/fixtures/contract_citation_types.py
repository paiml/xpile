# PMAT-475 (R6): str/list/dict constructs must cite their (already-existing)
# type-translation contracts. Before this, applicable_contracts() cited only
# C-PY-INT-ARITH, so str/list/dict code shipped UNCITED — the capability-vs-
# contract drift (audit-design.md §6). Signal = the types in play (params,
# return, and local let/loop bindings).


def uses_str(s: str) -> int:
    return len(s)


def uses_list(xs: list[int]) -> int:
    return xs[0]


def uses_dict_param(m: dict[int, int]) -> int:
    return m[0]


def uses_dict_local(n: int) -> int:
    d: dict[int, int] = {}
    return len(d)


def uses_int_only(a: int, b: int) -> int:
    return a + b
