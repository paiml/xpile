# PMAT-795 (HUNT-V18 BIC-01): abs() and round() over a bool emitted a bare
# `abs(b)` / `round(b)` free call (rustc E0425), where Python's bool is an int
# subtype — abs(True)==1, round(True)==1. The frontend now coerces a bool arg to
# i64 (abs goes through the i64 path, round returns the cast value).
# Cross-checked vs python3.


def abs_true() -> int:
    b = True
    return abs(b)


def abs_false() -> int:
    b = False
    return abs(b)


def round_true() -> int:
    b = True
    return round(b)
