# PMAT-607: pow() with a bool base/exp. Python bool is an int subtype
# (pow(True, n) == pow(1, n)), but the pow builtin only handled int/float bases
# and fell through to a bare `pow(...)` call (rustc E0425). The bool operand is
# now coerced to i64 like the operator paths (`flag ** n`).
def bp2(flag: bool, n: int) -> int:
    return pow(flag, n)


def bp3(flag: bool, n: int, m: int) -> int:
    return pow(flag, n, m)
