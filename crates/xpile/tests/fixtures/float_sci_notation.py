def s(x: float) -> str:
    # CPython uses scientific notation for floats whose decimal exponent is
    # < -4 or >= 16 (e.g. 1e16 -> "1e+16", 1e-5 -> "1e-05"); Rust's `{}` spells
    # them out instead. str()/repr()/print()/f-strings all route through this.
    return str(x)
