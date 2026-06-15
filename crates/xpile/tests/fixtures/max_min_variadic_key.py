# PMAT-704: variadic max(a, b, key=fn) / min(a, b, c, key=fn) — the args ARE the
# elements (not an iterable). Was rejected ("passes keyword args to unknown
# function max"). On a key tie Python returns the FIRST argument.
def longest(a: str, b: str) -> str:
    return max(a, b, key=len)


def shortest3(a: str, b: str, c: str) -> str:
    return min(a, b, c, key=len)


def by_abs(x: int, y: int) -> int:
    return max(x, y, key=abs)


def first_on_tie(a: str, b: str) -> str:
    return max(a, b, key=len)
