# PMAT-596: reversed(s) over a str reverses its characters. Python yields an
# iterator of 1-char strings; xpile materializes it as list[str] (reusing
# Reversed(StrChars(s))), so the textbook idioms compose:
#   "".join(reversed(s))  -> the reversed string
#   list(reversed(s))     -> list of reversed 1-char strings
#   for c in reversed(s)  -> iterate chars in reverse
def reverse_string(s: str) -> str:
    return "".join(reversed(s))


def first_reversed(s: str) -> str:
    out: str = "?"
    for c in reversed(s):
        out = c
        break
    return out


def reversed_len(s: str) -> int:
    return len(list(reversed(s)))
