# PMAT-872 (HUNT-V31 #4): an f-string `{x!s:spec}` applies str() FIRST, then formats
# the resulting STRING with string-spec semantics (left-align default, fill char) —
# NOT the value's numeric spec. xpile emitted `format!("{:5}", x)` (numeric
# right-align) ignoring the `!s`. Cross-checked vs python3.


def s_width(x: int) -> str:
    return f"[{x!s:5}]"


def s_fill(x: int) -> str:
    return f"[{x!s:*<5}]"


def s_float(x: float) -> str:
    return f"[{x!s:8}]"


def num_width(x: int) -> str:
    return f"[{x:5}]"


def r_width(x: int) -> str:
    return f"[{x!r:>6}]"


def s_str(s: str) -> str:
    return f"[{s!s:5}]"
