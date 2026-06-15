# PMAT-633: stepped string slices s[a:b:step] — full parity with list slices.
# Char-indexed (Unicode-correct), positive and unbounded-negative step.
def every_other(s: str) -> str:
    return s[::2]


def bounded_step(s: str) -> str:
    return s[1:8:2]


def every_third(s: str) -> str:
    return s[::3]


def rev_every_other(s: str) -> str:
    return s[::-2]


def unicode_step(s: str) -> str:
    return s[::2]


def still_reverse(s: str) -> str:
    return s[::-1]
