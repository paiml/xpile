# PMAT-853 (HUNT-V28 #13): xs.count(x) / xs.index(x) were gated to list[int]; a
# list of str/float/bool was rejected. The ListQuery codegen now compares by place
# (**__e == x) instead of destructuring by copy (|&&__e|), so a non-Copy element
# (String) works too. Cross-checked vs python3.


def count_str(xs: list[str], t: str) -> int:
    return xs.count(t)


def count_float(xs: list[float]) -> int:
    return xs.count(1.5)


def index_str(xs: list[str], t: str) -> int:
    return xs.index(t)


def count_int(xs: list[int]) -> int:
    return xs.count(2)
