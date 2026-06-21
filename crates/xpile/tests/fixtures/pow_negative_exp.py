# PMAT-865 (HUNT-V30 #13): pow(int, negative_int) is float in Python
# (pow(2, -1) == 0.5), but xpile's pow() builtin stayed integer (checked_pow) and
# a `-> float` return rejected ("body produces I64") — asymmetric with the **
# operator, which already takes the float path for a negative exponent. pow() now
# mirrors that rule. Cross-checked vs python3.


def recip() -> float:
    return pow(2, -1)


def neg_exp2() -> float:
    return pow(4, -2)


def pos_pow() -> int:
    return pow(2, 10)


def float_pow() -> float:
    return pow(2.0, 3.0)
