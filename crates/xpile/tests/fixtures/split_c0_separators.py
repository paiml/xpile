# PMAT-649: Python `str.split()` (no arg) splits on `str.isspace()` whitespace,
# which includes the C0 separators U+001C-1F (FS/GS/RS/US). Rust's
# `split_whitespace` excludes them, so the transpiler now uses a C0-inclusive
# predicate (matching PMAT-600's strip/isspace fix) + an empty filter.


def count_c0() -> int:
    s = "a\x1cb\x1dc\x1ed\x1fe"
    return len(s.split())


def count_mixed() -> int:
    # leading/trailing/consecutive separators (C0 + ASCII ws) collapse, no empties
    s = "  x\x1c\x1cy  z\x1f"
    return len(s.split())


def count_plain() -> int:
    return len("one two three".split())


def last_part() -> str:
    return "a\x1cb\x1dc".split()[2]
