def is_subset(a: set[int], b: set[int]) -> bool:
    return a.issubset(b)


def is_superset(a: set[int], b: set[int]) -> bool:
    return a.issuperset(b)


def is_disjoint(a: set[int], b: set[int]) -> bool:
    return a.isdisjoint(b)


def subset_op(a: set[int], b: set[int]) -> bool:
    return a <= b


def proper_subset_op(a: set[int], b: set[int]) -> bool:
    return a < b


def superset_op(a: set[int], b: set[int]) -> bool:
    return a >= b


def guard(a: set[int], b: set[int]) -> int:
    if a <= b:
        return 1
    return 0
