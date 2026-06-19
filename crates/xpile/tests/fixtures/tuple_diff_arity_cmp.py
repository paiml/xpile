# PMAT-836 (HUNT-V26 #6): two tuples of statically-different arity are never equal
# in Python ((1,2) == (1,2,3) is False, != is True), but Rust can't compare the
# mismatched tuple types → E0308. A single ==/!= between different-arity tuples
# now folds to the constant result; same-arity stays structural. Cross-checked vs python3.


def diff_arity() -> int:
    a = (1, 2)
    b = (1, 2, 3)
    return (1 if a == b else 0) + (10 if a != b else 0)


def same_arity() -> int:
    a = (1, 2)
    b = (1, 2)
    c = (1, 3)
    return (1 if a == b else 0) + (10 if a == c else 0) + (100 if a != c else 0)
