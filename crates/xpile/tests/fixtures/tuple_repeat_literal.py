# PMAT-792 (HUNT-V18 #12): a tuple literal repeated by an int literal (`(0,) * 3`,
# `(1, 2) * 2`) mis-lowered as scalar int multiplication → rustc E0599
# (checked_mul on a tuple). A Python tuple repeat is a fixed-arity tuple, so it
# is now expanded at lowering when the count is a compile-time literal (Rust
# tuples aren't variadic; a runtime count remains a documented limitation).
# Cross-checked vs python3.


def zeros() -> tuple[int, int, int]:
    return (0,) * 3


def pair_rep() -> tuple[int, int, int, int]:
    return (1, 2) * 2


def sum_zeros() -> int:
    t = (0,) * 3
    return t[0] + t[1] + t[2]
