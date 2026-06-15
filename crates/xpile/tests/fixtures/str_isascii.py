# PMAT-695: str.isascii() — True iff every char is in the ASCII range
# (U+0000..=U+007F). The empty string is True (both Python and Rust). Lowers to
# `(s).is_ascii()` — no empty guard, unlike the isdigit-family predicates.
def asc(s: str) -> bool:
    return s.isascii()


def asc_empty() -> bool:
    return "".isascii()


def still_alnum(s: str) -> bool:
    return s.isalnum()
