# PMAT-670: a NEGATIVE constant tuple index t[-k] now resolves at compile time
# to the field access t.(arity - k) (Python from-the-end). The old path fell to
# the list-style runtime wrap (`__lc.len()...`), which is E0599 on a tuple (no
# .len()). Works for heterogeneous tuples too (field access). A non-literal
# (runtime) tuple index is rejected cleanly elsewhere.


def last(t: tuple[int, int, int]) -> int:
    return t[-1]


def secondlast(t: tuple[int, int, int]) -> int:
    return t[-2]


def first_regression(t: tuple[int, int, int]) -> int:
    return t[0]


def mixed_neg(t: tuple[int, str]) -> str:
    return t[-1]
