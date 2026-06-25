# PMAT-943 (correctness-hunt): a FLOAT with a FILL+ALIGN spec
# `[fill]<align><width>` — f"{3.0: >7}" == "    3.0", f"{3.0:*<8}" == "3.0*****",
# f"{2.5:-^9}" == "---2.5---", f"{3.0:0>7}" == "00003.0" — silently dropped the
# repr's trailing .0 ("      3" instead of "    3.0"). PMAT-807 fixed the
# align-ONLY / bare-width forms by padding the Python repr STRING (str(float),
# keeps the .0), but only peeled a LEADING align char, so a FILL char in front of
# the alignment marker wasn't recognized and the spec fell through to the bare-f64
# format! path (Rust's f64 Display drops the .0). The fill char (any char before
# `<`/`>`/`^`) is now detected and the repr string is padded with the verbatim
# [fill]<align> prefix (Rust's fill+align syntax is identical to Python's).
# Precision forms (*>8.2f) stay on the unchanged float-precision path. vs python3.


def fa_space_right() -> str:
    x = 3.0
    return f"{x: >7}"


def fa_star_left() -> str:
    x = 3.0
    return f"{x:*<8}"


def fa_dash_center() -> str:
    x = 2.5
    return f"{x:-^9}"


def fa_zero_right() -> str:
    x = 3.0
    return f"{x:0>7}"


def fa_space_neg() -> str:
    x = -3.0
    return f"{x: >7}"


def fa_star_nonwhole() -> str:
    x = 12.5
    return f"{x:*>8}"


def fa_space_left() -> str:
    x = 3.0
    return f"{x: <8}"


def fa_no_pad_needed() -> str:
    x = 123.456
    return f"{x:*>4}"


# PMAT-807 forms must still work (regression guard): align-only + bare width.
def fa_align_only() -> str:
    x = 3.0
    return f"{x:>6}"


def fa_bare_width() -> str:
    x = 3.0
    return f"{x:8}"


def labeled(x: float) -> str:
    return f"[{x: ^9}]"
