# PMAT-502ce (Tranche 2): context-aware `and`/`or` over bool variables. The
# context-free path mis-inferred a bare Ident as int and rejected `a and b`.
def both(a: bool, b: bool) -> bool:
    return a and b


def either(a: bool, b: bool, c: bool) -> bool:
    return a or b or c


def gate(x: int, active: bool) -> int:
    if active and x > 0:
        return x
    return 0
