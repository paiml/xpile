# PMAT-613: an f-string radix spec (:x/:X/:b/:o) of a possibly-negative int.
# Python formats negatives sign-magnitude (f"{-255:x}" == "-ff"), but Rust's
# format!("{:x}", n) is two's-complement (ffffffffffffff01) → silent wrong
# output. The bare radix now reuses the sign-magnitude IntRadixStr path.
def hexfmt(n: int) -> str:
    return f"{n:x}"


def hexup(n: int) -> str:
    return f"{n:X}"


def binfmt(n: int) -> str:
    return f"{n:b}"


def octfmt(n: int) -> str:
    return f"{n:o}"


def labeled(n: int) -> str:
    return f"val={n:x}!"
