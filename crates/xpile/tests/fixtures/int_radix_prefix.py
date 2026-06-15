# PMAT-655: int(s, base) accepts a base-matching radix prefix (0x/0o/0b) and
# PEP-515 underscore grouping, like Python. Rust's from_str_radix accepts
# neither, so int("0xff", 16) and int("1_000", 16) used to panic.


def hex_prefix() -> int:
    return int("0xff", 16)


def oct_prefix() -> int:
    return int("0o17", 8)


def bin_prefix() -> int:
    return int("0b101", 2)


def neg_hex_prefix() -> int:
    return int("-0x1a", 16)


def underscore_grouping() -> int:
    return int("1_000", 16)


def no_prefix() -> int:
    # regression: a plain unprefixed digit string still parses
    return int("ff", 16)
