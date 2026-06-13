def lone_field(n: int) -> str:
    # The simplest f-string — a single field, no surrounding text or spec.
    return f"{n}"


def hex_lower(n: int) -> str:
    return f"{n:x}"


def hex_upper(n: int) -> str:
    return f"{n:X}"


def binary(n: int) -> str:
    return f"{n:b}"


def octal(n: int) -> str:
    return f"{n:o}"


def width(n: int) -> str:
    return f"[{n:5}]"


def zero_pad(n: int) -> str:
    return f"{n:05}"


def zero_pad_hex(n: int) -> str:
    return f"{n:04x}"


def zero_pad_binary(n: int) -> str:
    return f"{n:08b}"


def mixed(n: int) -> str:
    # field, text, spec'd field, text.
    return f"n={n} hex=0x{n:x}"
