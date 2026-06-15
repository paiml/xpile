# PMAT-659: a float-precision f-string spec (`{:.Nf}`, `{:.N%}`) of NaN prints
# "nan" in Python but Rust's format! prints "NaN". The FormatSpec emit now guards
# NaN. inf already matches ("inf"/"-inf"); the `.`-fill str case is not a float
# precision, so it's left untouched.


def fmt_nan() -> str:
    x = float("nan")
    return f"{x:.2f}"


def fmt_inf() -> str:
    x = float("inf")
    return f"{x:.2f}"


def fmt_normal() -> str:
    x = 3.14159
    return f"{x:.2f}"


def fmt_percent_nan() -> str:
    x = float("nan")
    return f"{x:.0%}"


def fmt_sign_prec() -> str:
    x = 3.5
    return f"{x:+.2f}"
