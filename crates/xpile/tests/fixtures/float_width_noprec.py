# PMAT-807 (HUNT-V21 FS): a float with an align/width-ONLY spec (no precision) —
# f"{3.0:>6}", f"{3.0:8}" — used Rust's bare f64 Display ("3", no ".0"), dropping
# Python's trailing .0 (silent-wrong: "     3" vs "   3.0"); the bare-width form
# was even rejected. The float is now rendered to its Python repr string first
# (str(float)) and THAT is padded; a bare width forces right-align (Python
# right-aligns numbers). Precision specs (.2f) are unchanged. vs python3.


def bare_w() -> str:
    x = 3.0
    return f"{x:8}"


def right_a() -> str:
    x = 3.0
    return f"{x:>6}"


def left_a() -> str:
    x = 2.5
    return f"{x:<6}"


def center_a() -> str:
    x = 2.5
    return f"{x:^7}"


def prec_unchanged() -> str:
    x = 3.14159
    return f"{x:.2f}"
