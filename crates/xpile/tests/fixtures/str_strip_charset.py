# PMAT-691: str.strip(chars) / lstrip / rstrip with a char-SET arg — Python strips
# ANY leading/trailing char that is IN the set (not a substring). Was rejected
# ("expected exactly 0" args); the 0-arg whitespace form is unchanged.
def strip_cs(s: str) -> str:
    return s.strip(".,!")


def lstrip_cs(s: str) -> str:
    return s.lstrip("x")


def rstrip_cs(s: str) -> str:
    return s.rstrip(".")


def strip_ws(s: str) -> str:
    return s.strip()
