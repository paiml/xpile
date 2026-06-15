# PMAT-651: float floor-division preserves the sign of a zero result, matching
# CPython's float_divmod (which returns copysign(0.0, a/b) when the quotient is
# zero). `-0.0 // 1.0` is -0.0 in Python; xpile's fmod-based formula used to
# floor(0.0) → +0.0, dropping the sign.


def negzero_pos() -> str:
    return str(-0.0 // 1.0)


def negzero_var() -> str:
    x = 0.0
    return str(-x // 1.0)


def pos_neg() -> str:
    # +0.0 // -1.0 → -0.0 (already correct, kept as a regression guard)
    return str(0.0 // -1.0)


def poszero() -> str:
    return str(0.0 // 1.0)


def normal_neg() -> str:
    return str(-7.0 // 2.0)


def small_pos() -> str:
    # 1.0 // 5.0 → +0.0 (zero quotient with positive sign)
    return str(1.0 // 5.0)
