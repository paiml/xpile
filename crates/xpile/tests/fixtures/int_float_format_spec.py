# PMAT-693: an int with a FLOAT-presentation spec (`.Nf`, `.N%`) is coerced to
# float by Python (`f"{5:.2f}"` == "5.00"). xpile casts the int to f64 and routes
# it through the float-format path. Int-presentation specs (`d`/radix/width) are
# untouched.
def fixed(n: int) -> str:
    return f"{n:.2f}"


def one_dp(n: int) -> str:
    return f"{n:.1f}"


def pct(n: int) -> str:
    return f"{n:.0%}"


def width_fixed(n: int) -> str:
    return f"{n:8.3f}"


def still_int(n: int) -> str:
    return f"{n:05d}"
