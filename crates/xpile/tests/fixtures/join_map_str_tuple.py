# PMAT-825 (HUNT-V24 SF-2): map(str, X) (and sum/filter) over a homogeneous TUPLE
# emitted a bogus free `map(str, t)` call (invalid Rust) — the tuple fell through
# the List guard. lower_arg_materializing_range now materializes a homogeneous
# tuple to a list (literal → its elements; variable → t.0/t.1/... field accesses).
# Cross-checked vs python3.


def join_tuple_var() -> str:
    t = (1, 2, 3)
    return ",".join(map(str, t))


def join_list() -> str:
    xs = [4, 5, 6]
    return "-".join(map(str, xs))


def sum_tuple() -> int:
    t = (5, 10, 15)
    return sum(t)
