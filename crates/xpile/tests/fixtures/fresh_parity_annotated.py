# PMAT-1050 (sweep #12): an if-arm that ANNOTATES its fresh target
# (`if flag: y: int = 10 else: y = 20`) used the AnnAssign statement form,
# which the as-let shape check (plain-Assign only) rejected — so a FRESH `y`
# fell to the general path and emitted a bare `y = …` with no prior `let`
# (rustc E0425). Annotated `name: T = value` if-arms are now normalized to the
# plain `name = value` form so the mixed chain lowers as-let uniformly.
# Verified vs CPython.
def one_annotated(flag: bool) -> int:
    if flag:
        y: int = 10
    else:
        y = 20
    return y


def both_annotated(flag: bool) -> int:
    if flag:
        z: int = 3
    else:
        z: int = 4
    return z


def elif_annotated(n: int) -> int:
    if n < 3:
        y: int = 1
    elif n < 10:
        y = 2
    else:
        y = 3
    return y
