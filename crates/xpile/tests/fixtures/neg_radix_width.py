# PMAT-800 (HUNT-V19 FS-1): a plain-WIDTH radix format on a negative int
# (f"{-255:8x}", f"{-5:8b}") emitted Rust's two's-complement bits
# (ffffffffffffff01, width dropped) instead of Python's sign-magnitude,
# right-aligned ('     -ff'). The bare-radix and zero-pad arms already used the
# sign-magnitude IntRadixStr; the plain-width arm now builds that body and
# space-pads it to the width. A non-negative is unaffected. vs python3.


def hex_w() -> str:
    x: int = -255
    return f"{x:8x}"


def bin_w() -> str:
    x: int = -5
    return f"{x:8b}"


def upper_w() -> str:
    x: int = -255
    return f"{x:6X}"


def pos_w() -> str:
    return f"{255:8x}"
