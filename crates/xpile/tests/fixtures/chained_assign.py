# PMAT-502bz (Tranche 2): chained assignment `x = y = z = <literal>`.
# Python evaluates the value once and binds it to every target; first cut is
# scalar-literal values (int/float/bool/str), each target an independent copy.
def init_sum() -> int:
    a = b = c = 0
    a = a + 5
    b = b + 3
    return a + b + c


def flags() -> int:
    x = y = 1
    return x + y
