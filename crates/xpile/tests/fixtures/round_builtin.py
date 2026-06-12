# PMAT-502ak (Tranche 2): round(x) over a float -> nearest int via banker's
# rounding (round-half-to-even), matching Python exactly. round(int) is the
# identity.
def r(x: float) -> int:
    return round(x)


def r_int(n: int) -> int:
    return round(n)
