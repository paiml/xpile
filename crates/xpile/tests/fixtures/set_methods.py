def union_size(a: set[int], b: set[int]) -> int:
    return len(a.union(b))


def intersection_size(a: set[int], b: set[int]) -> int:
    return len(a.intersection(b))


def difference_size(a: set[int], b: set[int]) -> int:
    return len(a.difference(b))


def sym_diff_size(a: set[int], b: set[int]) -> int:
    return len(a.symmetric_difference(b))


def union_contains(a: set[int], b: set[int], x: int) -> bool:
    return x in a.union(b)
