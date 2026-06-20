# PMAT-848 (HUNT-V27 #12): a comprehension over a set or tuple was rejected,
# completing the for-loop set/tuple support (PMAT-847) for the comprehension
# form. Both comp paths (statement desugar + value-position closure-chain) now
# iterate a set's elements and a homogeneous tuple's elements. vs python3.


def comp_set(s: set[int]) -> int:
    return sum([x * 2 for x in s])


def comp_tuple() -> int:
    return sum([x for x in (5, 6, 7)])


def stmt_set(s: set[int]) -> int:
    ys = [x + 1 for x in s]
    return sum(ys)
