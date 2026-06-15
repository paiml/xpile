# PMAT-677: a float format spec combining WIDTH (and/or zero-pad, sign, align)
# WITH precision — `f"{x:8.3f}"`, `{:06.2f}`, `{:+8.2f}`, `{:>8.2f}` — is the
# most common numeric-table idiom. Rust's `{:8.3}` etc. match Python for floats.
def width(x: float) -> str:
    return f"{x:8.3f}"


def zero_pad(x: float) -> str:
    return f"{x:06.2f}"


def sign(x: float) -> str:
    return f"{x:+8.2f}"


def wide(x: float) -> str:
    return f"{x:10.4f}"


def align(x: float) -> str:
    return f"{x:>8.2f}"


def fill_align(x: float) -> str:
    return f"{x:*>8.2f}"


def via_format(x: float) -> str:
    return "{:8.3f}".format(x)


def plain(x: float) -> str:
    return f"{x:.2f}"
