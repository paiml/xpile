# PMAT-804 (HUNT-V20 SET-BOOL-MEMBERSHIP): a bool needle in an int set
# (True in {1, 2, 3}) emitted s.contains(&true) over a HashSet<i64> (rustc
# E0308), where Python's bool is an int subtype (True == 1) so the membership is
# True. The needle is now coerced to i64 (mirror of the dict bool-key coercion).
# Cross-checked vs python3.


def bool_in_intset() -> bool:
    s = {1, 2, 3}
    return True in s


def false_in_intset() -> bool:
    s = {0, 5}
    return False in s


def bool_notin() -> bool:
    s = {2, 3}
    return False not in s
