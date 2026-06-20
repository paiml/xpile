# PMAT-849 (HUNT-V27 #11): "%d"/"%i" over a float truncates toward zero in Python
# ("%d" % 3.7 == "3", "%d" % -2.9 == "-2"), like int(float), but it was rejected.
# The float is now cast to i64 (truncating toward zero) before the %d path.
# Cross-checked vs python3.


def pos() -> str:
    x = 3.7
    return "%d" % x


def neg() -> str:
    return "%d" % -2.9


def mixed() -> str:
    return "%d apples, %.1f kg" % (3.9, 2.5)


def int_unchanged() -> str:
    return "%d items at %.2f" % (5, 1.5)
