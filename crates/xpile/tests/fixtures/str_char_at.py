# PMAT-502cd (Tranche 2): string indexing `s[i]` → a 1-char string.
# Positive, negative (from the end), and variable int indices all work.
def first(s: str) -> str:
    return s[0]


def last(s: str) -> str:
    return s[-1]


def at(s: str, i: int) -> str:
    return s[i]
