# PMAT-773 (HUNT-V16 #12): a negative int with a ZERO-PADDED radix f-string spec
# (`f"{-255:08x}"`) emitted `format!("{:08x}", -255)`, which zero-pads Rust's
# two's-complement (`ffffffffffffff01`) — silent-wrong. Python formats
# sign-magnitude with the sign counted in the width: `-00000ff`. The radix
# format now carries a sign-aware zero-pad width. Cross-checked vs python3.


def hexpad_neg() -> str:
    return f"{-255:08x}"


def binpad_neg() -> str:
    return f"{-5:08b}"


def octpad_neg() -> str:
    return f"{-9:06o}"


def hexpad_pos() -> str:
    return f"{255:08x}"


def bare_neg() -> str:
    return f"{-255:x}"
