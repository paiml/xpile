# PMAT-666: zfill/center/ljust/rjust with a NEGATIVE width. Python treats a
# width <= len (incl. negative) as "no padding" and returns the string
# unchanged. xpile cast the width via a bare `as usize`, so a negative width
# underflowed to a huge value → capacity-overflow panic. The width is now
# clamped with `.max(0)`.


def zfill_neg(s: str) -> str:
    return s.zfill(-3)


def center_neg(s: str) -> str:
    return s.center(-3)


def ljust_neg(s: str) -> str:
    return s.ljust(-3)


def rjust_neg(s: str) -> str:
    return s.rjust(-3)


def ljust_fill_neg(s: str) -> str:
    return s.ljust(-2, "*")


def zfill_pos_regression(s: str) -> str:
    return s.zfill(5)
