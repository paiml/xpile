# PMAT-870 (HUNT-V31 #9): round(x, n) with n <= -309 returned NaN — `10f64.powi(-n)`
# overflows to +inf, so `(x / inf).round() * inf` is `0.0 * inf` = NaN. Python
# rounds to the nearest 10^|n| (== 0 for huge |n|). The emit now guards the
# overflow and returns a sign-preserving zero. Cross-checked vs python3.


def huge_neg(x: float) -> float:
    return round(x, -400)


def normal_neg(x: float) -> float:
    return round(x, -2)


def normal_pos(x: float) -> float:
    return round(x, 1)
