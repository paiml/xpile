# PMAT-1042 (sweep-#10 branch-parity residual, filed under PMAT-1039):
# assignment-only if/elif/else arms assigning DIFFERENT names refused
# ("every branch must assign the same names") even when every assigned name
# was PRE-BOUND — `x = 0; y = 0; if flag: x = 1 else: y = 2` reassigns
# scope-safely, so the general Stmt::If lowers it exactly; the as-let
# parity rule exists for FRESH bindings (each name needs a value from every
# arm). The dispatch diverts ONLY the would-refuse case (parity fails + all
# pre-bound): parity-holding chains keep the as-let emission byte-identical
# and fresh-name chains keep the precise parity refusal.
# Differentially verified vs CPython (MATCH 10/20/21/20).
def one_side_each() -> int:
    x = 0
    y = 0
    flag = True
    if flag:
        x = 1
    else:
        y = 2
    return x * 10 + y


def elif_chain() -> int:
    a = 0
    b = 0
    c = 0
    n = 5
    if n < 3:
        a = 1
    elif n < 10:
        b = 2
    else:
        c = 3
    return a * 100 + b * 10 + c


def uneven_arms() -> int:
    lo = 0
    hi = 0
    n = 7
    if n > 5:
        lo = n
        hi = n * 2
    else:
        lo = 1
    return lo + hi


def parity_fresh_still_as_let() -> int:
    flag = False
    if flag:
        v = 10
    else:
        v = 20
    return v
