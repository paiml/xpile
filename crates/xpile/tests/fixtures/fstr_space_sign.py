# PMAT-942 (correctness-hunt): the SPACE sign flag ` ` on a numeric f-string
# field — f"{5: d}" == " 5", f"{-5: d}" == "-5", f"{3.14: .2f}" == " 3.14", and
# width/zero-pad combos f"{5: 05d}" == " 0005", f"{42: 6.1f}" == "  42.0" — were
# a clean reject ("unsupported format spec `: d`"). Python's ` ` sign option puts
# a leading SPACE before a non-negative magnitude and a `-` before a negative one;
# Rust's format! has NO space-sign flag. But Rust's `+` flag composes with width /
# zero-pad / precision IDENTICALLY to Python's, and a non-negative `+`-formatted
# value carries exactly one leading `+` (a negative carries `-`, never `+`), so the
# spec routes to the new SpaceSignStr node: render with the `+` spec, then swap the
# rendered leading `+` for a space (a no-op for negatives). An int with a float
# presentation is coerced (f"{5: .2f}" == " 5.00"); a bool delegates to int
# (f"{True: d}" == " 1"). vs python3.


def ss_d(n: int) -> str:
    return f"{n: d}"


def ss_bare(n: int) -> str:
    return f"{n: }"


def ss_f2(x: float) -> str:
    return f"{x: .2f}"


def ss_int_f(n: int) -> str:
    return f"{n: .1f}"


def ss_width(n: int) -> str:
    return f"{n: 5d}"


def ss_zeropad(n: int) -> str:
    return f"{n: 05d}"


def ss_fwidth(x: float) -> str:
    return f"{x: 8.2f}"


def ss_fzeropad(x: float) -> str:
    return f"{x: 08.2f}"


def ss_bool(b: bool) -> str:
    return f"{b: d}"


def labeled(n: int) -> str:
    return f"v={n: d}!"
