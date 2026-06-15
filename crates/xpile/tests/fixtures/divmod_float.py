# PMAT-657: divmod(float, float) → (a // b, a % b) over the float ops. The int
# path inlined a FloorDiv/Mod tuple, but a float arg fell through to an undefined
# free divmod() call (E0425). The float ops already implement CPython's float
# divmod (sign follows the divisor), so the tuple matches divmod exactly.


def dm_pos() -> float:
    q, r = divmod(7.5, 2.0)
    return q * 100.0 + r


def dm_neg_dividend() -> float:
    q, r = divmod(-7.5, 2.0)
    return q * 100.0 + r


def dm_neg_divisor() -> float:
    q, r = divmod(7.5, -2.0)
    return q * 100.0 + r
