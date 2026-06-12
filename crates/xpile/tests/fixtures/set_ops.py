# PMAT-502g (Tranche 2): set algebra on set[int] operands.
# `|` union, `&` intersection, `-` difference, `^` symmetric difference.
# Each returns a new set (operands unchanged).
def union_op(a: set[int], b: set[int]) -> set[int]:
    return a | b


def intersect_op(a: set[int], b: set[int]) -> set[int]:
    return a & b


def diff_op(a: set[int], b: set[int]) -> set[int]:
    return a - b


def symdiff_op(a: set[int], b: set[int]) -> set[int]:
    return a ^ b
