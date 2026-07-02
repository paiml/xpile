# PMAT-1044 (sweep #12): as-let fusion miscompiled intra-arm read-after-write.
# The as-let path models each variable INDEPENDENTLY and emits per-variable
# updates in a fixed order, NOT source order — so `b = a; a = 9` (else arm)
# emitted `a = ..9` then `b = ..a`, and b read the UPDATED a (9) instead of
# the original (1): 9*10+9 vs Python 9*10+1. Now such chains (all names
# pre-bound) divert to the general sequential Stmt::If path. Differentially
# verified vs CPython (MATCH 27/91 + 22/11).
def swap_ish(flag: bool) -> int:
    a: int = 1
    b: int = 2
    if flag:
        a = b
        b = 7
    else:
        b = a
        a = 9
    return a * 10 + b


def chained(flag: bool) -> int:
    a: int = 1
    b: int = 2
    if flag:
        a = b
        b = a
    else:
        b = a
        a = b
    return a * 10 + b
