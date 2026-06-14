def sign_int(x: int) -> str:
    # `+` sign flag: always show the sign (Python `{:+}` → Rust `{:+}`).
    return f"{x:+}"


def sign_float(x: float) -> str:
    # Sign flag with explicit precision (bare float `:+` is deferred).
    return f"{x:+.2f}"


def sign_pad(x: int) -> str:
    # Sign composes with zero-pad + width.
    return f"{x:+05d}"


def sign_hex(x: int) -> str:
    # Sign composes with a radix type char, inside surrounding text.
    return f"[{x:+x}]"


def plain_d(x: int) -> str:
    # A bare `:d` (decimal) is the int default → plain field.
    return f"{x:d}"
