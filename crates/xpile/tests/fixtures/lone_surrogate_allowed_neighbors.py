# PMAT-1086 precision neighbors: everything NEAR the refused shape stays
# allowed and CPython-exact. A comment mentioning \ud800 is not a literal.
def genuine_fffd() -> int:
    s = "�"
    return len(s)


def raw_text() -> int:
    s = r"\ud800"
    return len(s)


def escaped_backslash() -> int:
    s = "\\ud800"
    return len(s)


def boundary() -> bool:
    a = "\ud7ff"
    b = "\ue000"
    return a < b
