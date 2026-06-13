def to_hex(n: int) -> str:
    return "%x" % n


def to_hex_upper(n: int) -> str:
    return "%X" % n


def to_oct(n: int) -> str:
    return "%o" % n


def prefixed_hex(n: int) -> str:
    return "0x%x" % n
