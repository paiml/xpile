# PMAT-822 (HUNT-V24 SF-1): str.format() with an align/width-only spec on a float
# ("{:>6}".format(3.0)) used Rust's bare f64 Display, dropping Python's trailing
# .0 ("     3" vs "   3.0"). The float is now rendered to its Python repr string
# first (ToStr) and padded; a bare width forces right-align. Precision specs
# (.2f) keep the NaN-guarded path. Mirrors the f-string fix (PMAT-807). vs python3.


def right() -> str:
    return "{:>6}".format(3.0)


def bare() -> str:
    return "{:8}".format(2.5)


def left() -> str:
    return "{:<6}".format(2.5)


def prec() -> str:
    return "{:.2f}".format(3.14159)
