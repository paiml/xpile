# PMAT-702: ord() requires EXACTLY one character — Python raises TypeError for a
# multi-char string (ord("ab")) or the empty string, NOT the first char's code
# point. xpile asserts a single char instead of silently taking the first.
def code(c: str) -> int:
    return ord(c)


def code_plus(c: str) -> int:
    return ord(c) + 1
