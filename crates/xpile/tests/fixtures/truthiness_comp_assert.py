# PMAT-713: comprehension filters `[x for x in xs if x]` and `assert x` use Python
# truthiness, like `if x:` (which already works). Coerce the operand to its
# truthiness (int != 0, len != 0, float != 0.0); a Bool predicate is unchanged.
def nonzero_ints(xs: list[int]) -> list[int]:
    return [x for x in xs if x]


def nonempty_strs(xs: list[str]) -> list[str]:
    return [s for s in xs if s]


def assert_int(n: int) -> int:
    assert n
    return n * 2


def assert_list(xs: list[int]) -> int:
    assert xs
    return len(xs)
